use tokio::{net::{ToSocketAddrs, UdpSocket}, select, time};
use std::{collections::HashMap, sync::Arc, time::Duration};
use serde::{Serialize, de::DeserializeOwned};
use std::net::SocketAddr;
use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx, unbounded_channel as uc};

pub mod types;
pub mod transport;
pub mod master;
pub mod slave;

pub use types::*;

use tokio::{net::TcpStream};

use crate::order_management::{Order as Order, Status as Status};
use crate::elevator::elevio::poll::CallButton as CallButton;

// Message type identifiers
//pub const MSG_TYPE_CALL_REQUEST: u8 = 0;
//pub const MSG_TYPE_CALL_ASSIGNMENT: u8 = 1;
//pub const MSG_TYPE_UPDATE_FLOOR: u8 = 2;
//pub const MSG_TYPE_CALL_COMPLETE: u8 = 3;
//pub const MSG_TYPE_CALL_LIGHT_ASSIGNMENT: u8 = 4;

// Enum representing all possible message types
// #[derive(Debug, Clone)]
// pub enum NetworkMessage {
//     CallRequest(CallButton),
//     CallAssignment(CallButton),
//     UpdateFloor(u8),
//     CallComplete(CallButton),
//     CallLightAssignment(CallButton, bool),
// }


pub async fn init_socket(local_id: &String) -> Arc<UdpSocket> {
    let local_addr = format!("localhost:{}", local_id);
    let sock = UdpSocket::bind(local_addr).await.unwrap();
    let mysock: Arc<UdpSocket> = Arc::new(sock);

    mysock.clone()
}

pub async fn ping_alive_sender(send_sock: Arc<UdpSocket>, id: u8, remote_ids: Vec<u8>) {
    loop {
        for remote_id in &remote_ids {
            let remote_addr = format!("localhost:300{}", remote_id);
            send_sock.send_to(&id.to_be_bytes(), &remote_addr).await.unwrap();
            //println!("Sent id: {} to {}", id, remote_addr);
        }
        time::sleep(Duration::from_millis(1000)).await;
    }
}

pub async  fn ping_alive_receiver(recv_sock: Arc<UdpSocket>, ping_received_tx: UTx<u8>) {
    loop {
        let mut buf  = [0; 1024];
        let (n, addr) = recv_sock.recv_from(&mut buf).await.unwrap();
        let data = &buf[..n];
        let received_id = u8::from_be_bytes([data[0]]);
        //println!("Received ping from {}: {}", addr, received_id);
        let _ = ping_received_tx.send(received_id);
    }
}

pub async fn store_online_elevators(local_id: u8, elevs_alive_tx: UTx<Vec<u8>>, mut ping_received_rx: URx<u8>) {
    let mut online_elevators: HashMap<u8, time::Instant> = HashMap::new();
    let timeout_duration = Duration::from_millis(5000);
    loop {
        tokio::select! {
            Some(received_id) = ping_received_rx.recv() => {
                //println!("Received ping from elevator {}", received_id);
                let was_new = online_elevators.insert(received_id, time::Instant::now());

                if was_new.is_none() {
                    elevs_alive_tx.send(online_elevators.keys().cloned().collect()).unwrap();
                }
            }
            
            _ = time::sleep(Duration::from_millis(500)) => {
                let now = time::Instant::now();
                let before_len = online_elevators.len();
                online_elevators.insert(local_id, time::Instant::now());

                online_elevators.retain(|_id, last_seen| {
                    now.duration_since(*last_seen) < timeout_duration
                });
                if online_elevators.len() != before_len {
                    elevs_alive_tx.send(online_elevators.keys().cloned().collect()).unwrap();
                    // println!("Current online elevators: {:?}", online_elevators.keys());
                }
                // println!("Online elevators: {:?}", online_elevators.keys())
            }
        }
    }
}


// pub const MAGIC: [u8; 4] = *b"EVL1";       // tag to make sure packet is sent from us, kinda redundant might delete later 

// // returns bytes sent 
// pub async fn send_msg<T: Serialize>(
//     sock: Arc<UdpSocket>,
//     addr: &impl ToSocketAddrs,
//     msg: &T,
//     typ: u8,
// ) -> usize {
//     let payload = bincode::serialize(msg).expect("bincode serialize failed");

//     let mut pkt = Vec::with_capacity(4 + payload.len() + 1);
//     pkt.extend_from_slice(&MAGIC);
//     pkt.extend_from_slice(&payload);
//     pkt.push(typ);

//     sock.send_to(&pkt, addr).await.expect("udp send_to failed")
// }


// pub async fn recv_msg<T: DeserializeOwned>(
//     sock: Arc<UdpSocket>,
// ) -> (T, SocketAddr, u8) {
//     let mut buf  = [0; 1024];
//     let (n, from) = sock.recv_from(&mut buf).await.expect("udp recv_from failed");
//     let data = &buf[..n];

//     assert!(data.len() >= 5, "packet too short"); // MAGIC (4) + at least 1 byte payload + type (1)
//     assert!(data[..4] == MAGIC, "bad magic");

//     let typ = data[n - 1]; // Last byte is the type identifier
//     let msg: T = bincode::deserialize(&data[4..n-1]).expect("bincode deserialize failed");
//     (msg, from, typ)
// }

// // Receive message and deserialize to the correct type based on the type identifier
// pub async fn recv_typed_msg(
//     sock: Arc<UdpSocket>,
// ) -> (NetworkMessage, SocketAddr, u8) {
//     loop {
//         let mut buf  = [0; 1024];
//         let (n, from) = sock.recv_from(&mut buf).await.expect("udp recv_from failed");
//         let data = &buf[..n];

//         assert!(data.len() >= 5, "packet too short"); // MAGIC (4) + at least 1 byte payload + type (1)
//         assert!(data[..4] == MAGIC, "bad magic");

//         let typ = data[n - 1]; // Last byte is the type identifier
//         let payload = &data[4..n-1];

//         let msg = match typ {
//             MSG_TYPE_CALL_REQUEST => {
//                 let cb: CallButton = bincode::deserialize(payload).expect("bincode deserialize failed");
//                 NetworkMessage::CallRequest(cb)
//             }
//             MSG_TYPE_CALL_ASSIGNMENT => {
//                 let cb: CallButton = bincode::deserialize(payload).expect("bincode deserialize failed");
//                 NetworkMessage::CallAssignment(cb)
//             }
//             MSG_TYPE_UPDATE_FLOOR => {
//                 let floor: u8 = bincode::deserialize(payload).expect("bincode deserialize failed");
//                 NetworkMessage::UpdateFloor(floor)
//             }
//             MSG_TYPE_CALL_COMPLETE => {
//                 let cb: CallButton = bincode::deserialize(payload).expect("bincode deserialize failed");
//                 NetworkMessage::CallComplete(cb)
//             }
//             MSG_TYPE_CALL_LIGHT_ASSIGNMENT => {
//                 let (cb, on): (CallButton, bool) = bincode::deserialize(payload).expect("bincode deserialize failed");
//                 NetworkMessage::CallLightAssignment(cb, on)
//             }
//             _ => panic!("Unknown message type: {}", typ),
//         };

//         return (msg, from, typ);
//     }
// }

pub async fn udp_sender(socket:Arc<UdpSocket>, master_addr: String, slave_addr: String, mut call_request_rx: URx<CallButton>, mut order_assign_rx: URx<Order>, mut update_floor_rx: URx<u8>, mut call_complete_rx: URx<CallButton>, mut order_light_assign_rx: URx<(Order, bool)>) {
    loop {
        select! {
            Some(cb) = call_request_rx.recv() => {
                send_msg::<CallButton>(socket.clone(), &master_addr, &cb, MSG_TYPE_CALL_REQUEST).await;
            }
            Some(order) = order_assign_rx.recv() => {
                send_msg::<CallButton>(socket.clone(), &format!("localhost:200{}", order.elev_idx), &order.cb, MSG_TYPE_CALL_ASSIGNMENT).await;
            }
            Some(floor) = update_floor_rx.recv() => {
                send_msg::<u8>(socket.clone(), &master_addr, &floor, MSG_TYPE_UPDATE_FLOOR).await;
            }
            Some(cb) = call_complete_rx.recv() => {
                send_msg::<CallButton>(socket.clone(), &master_addr, &cb, MSG_TYPE_CALL_COMPLETE).await;
            }
            Some((order,on)) = order_light_assign_rx.recv() => {
                if order.cb.call != 2 {
                    send_msg::<(CallButton, bool)>(socket.clone(), &master_addr, &(order.clone().cb,on), MSG_TYPE_CALL_LIGHT_ASSIGNMENT).await;
                    send_msg::<(CallButton, bool)>(socket.clone(), &slave_addr, &(order.cb,on), MSG_TYPE_CALL_LIGHT_ASSIGNMENT).await;

                }
                else {
                    send_msg::<(CallButton, bool)>(socket.clone(), &format!("localhost:200{}", order.elev_idx), &(order.cb,on), MSG_TYPE_CALL_LIGHT_ASSIGNMENT).await;
                }
            }
            
        }
    }
}


// pub async fn network_runner(elevs_alive_tx: UTx<Vec<u8>>, mut at_floor_rx: URx<u8>, local_id: u8, remote_ids: Vec<u8>){


//     let _ = tokio::join!(ping_alive_sender_task, ping_alive_receiver_task, store_online_elevators_task);

// }

pub async fn udp_receiver(socket:Arc<UdpSocket>, order_request_tx: UTx<Order>, call_assign_tx: UTx<CallButton>, update_status_tx: UTx<Status>, order_complete_tx: UTx<Order>, call_light_assign_tx: UTx<(CallButton, bool)>) {
    loop {
        // First check for alive pings (simple text messages)
        let mut buf = [0; 1024];
        let (n, from) = socket.recv_from(&mut buf).await.expect("udp recv_from failed");
        let data = &buf[..n];
        
        
        // Not an alive ping - process as protocol message
        // We need to use recv_typed_msg, but it will recv again, so we need a different approach
        // Let's process the protocol message directly here
        assert!(data.len() >= 5, "packet too short");
        assert!(data[..4] == MAGIC, "bad magic");
        
        let typ = data[n - 1];
        let payload = &data[4..n-1];
        // Extract elevator ID from port: port format is 200{id}, so extract id by subtracting 20000
        let elev_idx = (from.port() - 20000) as usize;
        
        let msg = match typ {
            MSG_TYPE_CALL_REQUEST => {
                let cb: CallButton = bincode::deserialize(payload).expect("bincode deserialize failed");
                NetworkMessage::CallRequest(cb)
            }
            MSG_TYPE_CALL_ASSIGNMENT => {
                let cb: CallButton = bincode::deserialize(payload).expect("bincode deserialize failed");
                NetworkMessage::CallAssignment(cb)
            }
            MSG_TYPE_UPDATE_FLOOR => {
                let floor: u8 = bincode::deserialize(payload).expect("bincode deserialize failed");
                NetworkMessage::UpdateFloor(floor)
            }
            MSG_TYPE_CALL_COMPLETE => {
                let cb: CallButton = bincode::deserialize(payload).expect("bincode deserialize failed");
                NetworkMessage::CallComplete(cb)
            }
            MSG_TYPE_CALL_LIGHT_ASSIGNMENT => {
                let (cb, on): (CallButton, bool) = bincode::deserialize(payload).expect("bincode deserialize failed");
                NetworkMessage::CallLightAssignment(cb, on)
            }
            _ => {
                println!("Unknown message type: {}", typ);
                continue;
            }
        };
        
        // println!("{:?}", &msg);

        match msg {
            NetworkMessage::CallRequest(cb) => {
                // CALL REQUEST
                let _ = order_request_tx.send(Order { cb: cb, elev_idx});
            }
            NetworkMessage::CallAssignment(cb) => {
                // ORDER ASSIGNMENT
                let _ = call_assign_tx.send(cb);
            }
            NetworkMessage::UpdateFloor(floor) => {
                // UPDATE FLOOR
                let _ = update_status_tx.send(Status { floor, elev_idx});
            }
            NetworkMessage::CallComplete(cb) => {
                // CALL COMPLETE
                let _ = order_complete_tx.send(Order { cb: cb, elev_idx});
            }
            NetworkMessage::CallLightAssignment(cb, on) => {
                // ORDER LIGHT ASSIGNMENT
                let _ = call_light_assign_tx.send((cb, on));
            }
        }
    }
}


pub async fn network_runner(local: u8, remote: u8, call_request_rx: URx<CallButton>, call_assign_tx: UTx<CallButton>, update_floor_rx: URx<u8>, call_complete_rx: URx<CallButton>, call_light_assign_tx: UTx<(CallButton, bool)>,
order_request_tx: UTx<Order>, order_assign_rx: URx<Order>, update_status_tx: UTx<Status>, order_complete_tx: UTx<Order>, order_light_assign_rx: URx<(Order, bool)>, elevs_alive_tx: UTx<Vec<u8>>, mut master_notify_rx: URx<Vec<u8>>) {
    

    let (ping_received_tx, ping_received_rx) = uc::<u8>();
    let ping_socket = init_socket(&format!("300{}", local)).await;
    let sender_ping_socket = ping_socket.clone();
    let receiver_ping_socket = ping_socket.clone();
    
    let ping_alive_sender_task = tokio::spawn(async move {
        ping_alive_sender(sender_ping_socket.clone(), local, vec![remote]).await});
        let ping_alive_receiver_task = tokio::spawn(async move {
            ping_alive_receiver(receiver_ping_socket.clone(), ping_received_tx).await});
            let store_online_elevators_task = tokio::spawn(async move {
                store_online_elevators(local, elevs_alive_tx, ping_received_rx).await});
                
    let socket = init_socket(&format!("200{}", local)).await;
    let sender_socket = socket.clone();
    let receiver_socket = socket.clone();


    let udp_sender_task = tokio::spawn(async move {
        let is_master;
        let alive_ids = master_notify_rx.recv().await.unwrap();
            if alive_ids.iter().all(|&id| local <= id) {
                is_master = true;
                } else {
                    is_master = false;
                }
        // "local" for master (sends to itself), "remote" for slave (sends to master)
        let master_addr = if is_master {
            format!("localhost:200{}", local)
        } else {
            format!("localhost:200{}", remote)
        };
        let slave_addr = if is_master {
            format!("localhost:200{}", remote)
        } else {
            format!("localhost:200{}", local)
        };
        udp_sender(sender_socket, master_addr, slave_addr, call_request_rx, order_assign_rx, update_floor_rx, call_complete_rx, order_light_assign_rx).await});
    let udp_receiver_task = tokio::spawn(async move {
        udp_receiver(receiver_socket, order_request_tx, call_assign_tx, update_status_tx, order_complete_tx, call_light_assign_tx).await}); 

    let _ = tokio::join!(udp_sender_task, udp_receiver_task, ping_alive_sender_task, ping_alive_receiver_task, store_online_elevators_task);
}

pub async fn setup(my_id: u8) -> NetworkHandle {
    let role = elect_role(my_id).await;
    println!("[networking] id={} role={:?}", my_id, role);

    let (hall_call_done_tx, hall_call_done_rx) = uc::<HallCall>();
    let (new_hall_call_tx,  new_hall_call_rx)  = uc::<HallCall>();
    let (state_update_tx,   state_update_rx)   = uc::<ElevatorState>();
    let (assigned_hall_call_tx, assigned_hall_call_rx) = uc::<HallCall>();
    let (world_state_tx,    world_state_rx)    = uc::<Vec<(HallCall, u8)>>();

    match &role {
        Role::Master => {
            tokio::spawn(master::run_master(
                my_id,
                hall_call_done_rx,
                new_hall_call_rx,
                state_update_rx,
                assigned_hall_call_tx,
                world_state_tx,
            ));
        }
        Role::Slave { master_id } => {
            let master_id = *master_id;
            tokio::spawn(slave::run_slave(
                my_id,
                master_id,
                hall_call_done_rx,
                new_hall_call_rx,
                state_update_rx,
                assigned_hall_call_tx,
                world_state_tx,
            ));
        }
    }

    NetworkHandle {
        role,
        my_id,
        hall_call_done_tx,
        new_hall_call_tx,
        state_update_tx,
        assigned_hall_call_rx,
        world_state_rx,
    }
}

/// Lowest live ID wins: try to connect to every elevator with id < mine.
/// If any answers, I'm a slave; otherwise I'm master.
async fn elect_role(my_id: u8) -> Role {
    for candidate in 1..my_id {
        if try_connect(candidate).await.is_some() {
            println!("[networking] found master candidate id={}", candidate);
            return Role::Slave { master_id: candidate };
        }
    }
    Role::Master
}

async fn try_connect(id: u8) -> Option<TcpStream> {
    let addr = master::addr_for(id);
    match time::timeout(Duration::from_millis(500), TcpStream::connect(&addr)).await {
        Ok(Ok(stream)) => Some(stream),
        _ => None,
    }
}