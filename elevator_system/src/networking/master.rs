use std::{sync::Arc, collections::HashMap};
use tokio::{
    net::TcpListener,
    select,
    time::{self, Duration, interval},
    io::AsyncWriteExt,
};
use tokio::sync::{
    mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx, unbounded_channel as uc},
    Mutex,
};
use crate::networking::{
    types::{Direction, ElevatorState, HallCall, Msg, SharedWorld, WorldState},
    transport::recv_from_buf,
    HEARTBEAT_INTERVAL_MS, PEER_TIMEOUT_MS,
};

pub fn addr_for(id: u8) -> String {
    use crate::networking::BASE_PORT;
    format!("localhost:{}", BASE_PORT + id as u16)
}


pub async fn run_master(
    my_id: u8,
    mut hall_call_done_rx: URx<HallCall>,
    mut new_hall_call_rx: URx<HallCall>,
    mut state_update_rx: URx<ElevatorState>,
    assigned_hall_call_tx: UTx<HallCall>,
    world_state_tx: UTx<Vec<(HallCall, u8)>>,
) {
    let world: SharedWorld = Arc::new(Mutex::new(WorldState::default()));
    let slave_txs: Arc<Mutex<HashMap<u8, UTx<Msg>>>> = Arc::new(Mutex::new(HashMap::new()));

    // accept incoming slave connections in a separate task
    let listener = TcpListener::bind(addr_for(my_id)).await
        .expect("master: bind failed");
    println!("[master] listening on {}", addr_for(my_id));

    {
        let world = world.clone();
        let slave_txs = slave_txs.clone();
        let ahtx = assigned_hall_call_tx.clone();
        let wstx = world_state_tx.clone();
        tokio::spawn(async move {
            loop {
                let Ok((stream, peer)) = listener.accept().await else { continue };
                println!("[master] connection from {}", peer);
                tokio::spawn(handle_slave_connection(
                    my_id, stream,
                    world.clone(), slave_txs.clone(),
                    ahtx.clone(), wstx.clone(),
                ));
            }
        });
    }

    let mut heartbeat = interval(Duration::from_millis(HEARTBEAT_INTERVAL_MS));

    loop {
        select! {
            _ = heartbeat.tick() => {
                let assignments = {
                    let w = world.lock().await;
                    w.assignments.iter().map(|(k, v)| (k.clone(), *v)).collect::<Vec<_>>()
                };
                
                let _ = world_state_tx.send(assignments.clone());
                // broadcast to slaves
                let slaves = slave_txs.lock().await;
                for tx in slaves.values() {
                    let _ = tx.send(Msg::WorldState { assignments: assignments.clone() });
                    let _ = tx.send(Msg::Heartbeat);
                }
            }

            Some(call) = new_hall_call_rx.recv() => {
                let mut w = world.lock().await;
                if !w.assignments.contains_key(&call) {
                    let best = pick_best_elevator(&w, my_id, &call);
                    println!("[master] assigning {:?} → elevator {}", call, best);
                    w.assignments.insert(call.clone(), best);
                    drop(w);
                    if best == my_id {
                        let _ = assigned_hall_call_tx.send(call);
                    } else {
                        let slaves = slave_txs.lock().await;
                        if let Some(tx) = slaves.get(&best) {
                            let _ = tx.send(Msg::AssignHallCall(call));
                        }
                    }
                }
            }

            Some(done) = hall_call_done_rx.recv() => {
                world.lock().await.assignments.remove(&done);
                println!("[master] hall call done: {:?}", done);
            }

            Some(state) = state_update_rx.recv() => {
                world.lock().await.states.insert(state.id, state);
            }
        }
    }
}


async fn handle_slave_connection(
    master_id: u8,
    mut stream: tokio::net::TcpStream,
    world: SharedWorld,
    slave_txs: Arc<Mutex<HashMap<u8, UTx<Msg>>>>,
    _assigned_hall_call_tx: UTx<HallCall>,
    _world_state_tx: UTx<Vec<(HallCall, u8)>>,
) {
    let slave_id = match crate::networking::transport::recv_msg(&mut stream).await {
        Ok(Msg::Hello { id }) => id,
        _ => { eprintln!("[master] expected Hello, dropping connection"); return; }
    };
    println!("[master] slave {} connected", slave_id);

    let (to_slave_tx, mut to_slave_rx) = uc::<Msg>();
    slave_txs.lock().await.insert(slave_id, to_slave_tx);

    let (read_half, mut write_half) = stream.into_split();
    let mut read_half = tokio::io::BufReader::new(read_half);

    // write task 
    tokio::spawn(async move {
        while let Some(msg) = to_slave_rx.recv().await {
            let payload = bincode::serialize(&msg).unwrap();
            let len = payload.len() as u32;
            if write_half.write_all(&len.to_be_bytes()).await.is_err() { break; }
            if write_half.write_all(&payload).await.is_err() { break; }
        }
    });

    loop {
        match time::timeout(
            Duration::from_millis(PEER_TIMEOUT_MS),
            recv_from_buf(&mut read_half),
        ).await {
            Err(_) => {
                println!("[master] slave {} timed out", slave_id);
                break;
            }
            Ok(Err(e)) => {
                println!("[master] slave {} disconnected: {}", slave_id, e);
                break;
            }
            Ok(Ok(msg)) => {
                match msg {
                    Msg::StateUpdate(state) => {
                        world.lock().await.states.insert(slave_id, state);
                    }
                    Msg::NewHallCall(call) => {
                        println!("[master] slave {} reported hall call {:?}", slave_id, call);
                        let mut w = world.lock().await;
                        if !w.assignments.contains_key(&call) {
                            // TODO: connect to elevator assignment algo
                            let best = pick_best_elevator(&w, master_id, &call);
                            w.assignments.insert(call.clone(), best);
                            // assignemtn is delivered on the next heartbeat broadcast
                        }
                    }
                    Msg::HallCallDone(call) => {
                        world.lock().await.assignments.remove(&call);
                        println!("[master] slave {} done with {:?}", slave_id, call);
                    }
                    Msg::Heartbeat => {}
                    _ => {}
                }
            }
        }
    }

    slave_txs.lock().await.remove(&slave_id);
    println!("[master] removed slave {} from active connections", slave_id);
}
