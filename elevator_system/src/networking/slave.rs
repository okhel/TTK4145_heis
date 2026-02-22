use tokio::{
    net::TcpStream,
    select,
    time::{self, Duration, interval},
    io::AsyncWriteExt,
};
use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx, unbounded_channel as uc};
use crate::networking::{
    types::{ElevatorState, HallCall, Msg},
    transport::{send_msg, recv_from_buf},
    HEARTBEAT_INTERVAL_MS,
};
use crate::networking::master::addr_for;

pub async fn run_slave(
    my_id: u8,
    master_id: u8,
    mut hall_call_done_rx: URx<HallCall>,
    mut new_hall_call_rx: URx<HallCall>,
    mut state_update_rx: URx<ElevatorState>,
    assigned_hall_call_tx: UTx<HallCall>,
    world_state_tx: UTx<Vec<(HallCall, u8)>>,
) {
    loop {
        println!("[slave {}] connecting to master {}", my_id, master_id);

        let mut stream = match TcpStream::connect(addr_for(master_id)).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[slave {}] connect failed: {}; retrying in 1s", my_id, e);
                time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };

        if send_msg(&mut stream, &Msg::Hello { id: my_id }).await.is_err() {
            continue;
        }

        let (read_half, mut write_half) = stream.into_split();
        let mut read_half = tokio::io::BufReader::new(read_half);
        let (outbound_tx, mut outbound_rx) = uc::<Msg>();

        // write task
        let writer = tokio::spawn(async move {
            while let Some(msg) = outbound_rx.recv().await {
                let payload = bincode::serialize(&msg).unwrap();
                let len = payload.len() as u32;
                if write_half.write_all(&len.to_be_bytes()).await.is_err() { break; }
                if write_half.write_all(&payload).await.is_err() { break; }
            }
        });

        let mut hb = interval(Duration::from_millis(HEARTBEAT_INTERVAL_MS));
        let mut connected = true;

        while connected {
            select! {
                _ = hb.tick() => {
                    if outbound_tx.send(Msg::Heartbeat).is_err() {
                        connected = false;
                    }
                }

                Some(call) = new_hall_call_rx.recv() => {
                    let _ = outbound_tx.send(Msg::NewHallCall(call));
                }

                Some(done) = hall_call_done_rx.recv() => {
                    let _ = outbound_tx.send(Msg::HallCallDone(done));
                }

                Some(state) = state_update_rx.recv() => {
                    let _ = outbound_tx.send(Msg::StateUpdate(state));
                }

                result = recv_from_buf(&mut read_half) => {
                    match result {
                        Err(_) => {
                            println!("[slave {}] lost connection to master", my_id);
                            connected = false;
                        }
                        Ok(msg) => match msg {
                            Msg::AssignHallCall(call) => {
                                println!("[slave {}] assigned {:?}", my_id, call);
                                let _ = assigned_hall_call_tx.send(call);
                            }
                            Msg::WorldState { assignments } => {
                                let _ = world_state_tx.send(assignments);
                            }
                            Msg::Heartbeat => {}
                            _ => {}
                        }
                    }
                }
            }
        }

        writer.abort();
        println!("[slave {}] disconnected, reconnecting in 500ms...", my_id);
        time::sleep(Duration::from_millis(500)).await;

        // TODO: promote to master if still unreachable after N retries
        // Check: am I now the lowest-ID live elevator?
    }
}