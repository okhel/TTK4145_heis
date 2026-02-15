use tokio::{net::{ToSocketAddrs, UdpSocket}, select, time};
use std::{sync::Arc, time::Duration};
use serde::{Serialize, de::DeserializeOwned};
use std::net::SocketAddr;
use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx};

use crate::order_management::{Order as Order, Status as Status};
use crate::elevator::elevio::poll::CallButton as CallButton;

// Message type identifiers
pub const MSG_TYPE_CALL_REQUEST: u8 = 0;
pub const MSG_TYPE_CALL_ASSIGNMENT: u8 = 1;
pub const MSG_TYPE_UPDATE_FLOOR: u8 = 2;
pub const MSG_TYPE_CALL_COMPLETE: u8 = 3;
pub const MSG_TYPE_CALL_LIGHT_ASSIGNMENT: u8 = 4;

// Enum representing all possible message types
#[derive(Debug, Clone)]
pub enum NetworkMessage {
    CallRequest(CallButton),
    CallAssignment(CallButton),
    UpdateFloor(u8),
    CallComplete(CallButton),
    CallLightAssignment(CallButton, bool),
}


pub async fn init_socket(local: &String) -> Arc<UdpSocket> {
    let local_addr = format!("localhost:200{}", local);
    let sock = UdpSocket::bind(local_addr).await.unwrap();
    let mysock: Arc<UdpSocket> = Arc::new(sock);

    mysock.clone()
}

const MASTER_ALIVE: &[u8] = b"MASTER";
const SLAVE_ALIVE: &[u8] = b"SLAVE";

pub async fn ping_master_alive(send_sock: Arc<UdpSocket>, remote: &String) {
    // Master pings remote to announce it's alive
    // TODO: Consider broadcasting to all elevators if supporting more than 2 elevators
    // Currently sends to a single remote address. For multiple elevators, could use:
    // - UDP broadcast (255.255.255.255 or subnet broadcast)
    // - Or send to all known elevator addresses
    let remote_addr = format!("localhost:200{}", remote);
    loop {
        send_sock.send_to(MASTER_ALIVE, &remote_addr).await.unwrap();
        // println!("Sent Master Alive ping");
        time::sleep(Duration::from_millis(1000)).await;
    }
}

pub async fn ping_slave_alive(send_sock: Arc<UdpSocket>, remote: &String) {
    let remote_addr = format!("localhost:200{}", remote);
    loop {
        send_sock.send_to(SLAVE_ALIVE, &remote_addr).await.unwrap();
        time::sleep(Duration::from_millis(1000)).await;
    }
}


pub const MAGIC: [u8; 4] = *b"EVL1";       // tag to make sure packet is sent from us, kinda redundant might delete later 

// returns bytes sent 
pub async fn send_msg<T: Serialize>(
    sock: Arc<UdpSocket>,
    addr: &impl ToSocketAddrs,
    msg: &T,
    typ: u8,
) -> usize {
    let payload = bincode::serialize(msg).expect("bincode serialize failed");

    let mut pkt = Vec::with_capacity(4 + payload.len() + 1);
    pkt.extend_from_slice(&MAGIC);
    pkt.extend_from_slice(&payload);
    pkt.push(typ);

    sock.send_to(&pkt, addr).await.expect("udp send_to failed")
}


pub async fn recv_msg<T: DeserializeOwned>(
    sock: Arc<UdpSocket>,
) -> (T, SocketAddr, u8) {
    let mut buf  = [0; 1024];
    let (n, from) = sock.recv_from(&mut buf).await.expect("udp recv_from failed");
    let data = &buf[..n];

    assert!(data.len() >= 5, "packet too short"); // MAGIC (4) + at least 1 byte payload + type (1)
    assert!(data[..4] == MAGIC, "bad magic");

    let typ = data[n - 1]; // Last byte is the type identifier
    let msg: T = bincode::deserialize(&data[4..n-1]).expect("bincode deserialize failed");
    (msg, from, typ)
}

// Receive message and deserialize to the correct type based on the type identifier
pub async fn recv_typed_msg(
    sock: Arc<UdpSocket>,
) -> (NetworkMessage, SocketAddr, u8) {
    loop {
        let mut buf  = [0; 1024];
        let (n, from) = sock.recv_from(&mut buf).await.expect("udp recv_from failed");
        let data = &buf[..n];

        // Skip alive pings - they should be handled separately
        if data == MASTER_ALIVE || data == SLAVE_ALIVE {
            continue; // Skip and wait for next protocol message
        }

        assert!(data.len() >= 5, "packet too short"); // MAGIC (4) + at least 1 byte payload + type (1)
        assert!(data[..4] == MAGIC, "bad magic");

        let typ = data[n - 1]; // Last byte is the type identifier
        let payload = &data[4..n-1];

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
            _ => panic!("Unknown message type: {}", typ),
        };

        return (msg, from, typ);
    }
}

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

pub async fn udp_receiver(socket:Arc<UdpSocket>, order_request_tx: UTx<Order>, call_assign_tx: UTx<CallButton>, update_status_tx: UTx<Status>, order_complete_tx: UTx<Order>, call_light_assign_tx: UTx<(CallButton, bool)>) {
    loop {
        // First check for alive pings (simple text messages)
        let mut buf = [0; 1024];
        let (n, from) = socket.recv_from(&mut buf).await.expect("udp recv_from failed");
        let data = &buf[..n];
        
        // Handle alive pings separately
        if data == MASTER_ALIVE {
            // Master Alive received - already handled in network_runner election
            continue;
        }
        if data == SLAVE_ALIVE {
            // Slave Alive received
            // Extract elevator ID from port: port format is 200{id}, so extract id by subtracting 20000
            let elev_idx = (from.port() - 20000) as usize;
            // println!("Slave alive at port {}", elev_idx);
            continue;
        }
        
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
            _ => {
                println!("Unknown type: {}", typ);
            }
        }
    }
}


pub async fn network_runner(local: u8, remote: u8, call_request_rx: URx<CallButton>, call_assign_tx: UTx<CallButton>, update_floor_rx: URx<u8>, call_complete_rx: URx<CallButton>, call_light_assign_tx: UTx<(CallButton, bool)>,
order_request_tx: UTx<Order>, order_assign_rx: URx<Order>, update_status_tx: UTx<Status>, order_complete_tx: UTx<Order>, order_light_assign_rx: URx<(Order, bool)>, master_notify_tx: UTx<()>){
    let socket = init_socket(&local.to_string()).await;
    let sender_socket = socket.clone();
    let receiver_socket = socket.clone();

    // Listen for "Master Alive" for 3 seconds (don't ping anything during this time)
    let deadline = time::Instant::now() + Duration::from_secs(3);
    let mut buf = [0; 1024];
    let received_master_alive = loop {
        let remaining = deadline.saturating_duration_since(time::Instant::now());
        if remaining.is_zero() {
            break false; // Timeout - no Master Alive received
        }
        
        match time::timeout(remaining, receiver_socket.recv_from(&mut buf)).await {
            Ok(Ok((n, _))) => {
                let data = &buf[..n];
                if data == MASTER_ALIVE {
                    break true;
                }
                // Continue listening if it's not Master Alive (could be SLAVE_ALIVE)
            }
            Ok(Err(_)) => {
                // Receive error, continue trying until deadline
                continue;
            }
            Err(_) => {
                break false; // Timeout
            }
        }
    };

    let is_master = !received_master_alive;
    let ping_alive_task = if is_master {
        println!("I'm master - no Master Alive received within 3 seconds");
        // Notify order management that this elevator is master
        let _ = master_notify_tx.send(());
        let ping_socket = sender_socket.clone();
        Some(tokio::spawn(async move {
            ping_master_alive(ping_socket, &remote.to_string()).await;
        }))
    } else {
        println!("I'm slave - received Master Alive, starting to ping Slave Alive");
        let ping_socket = sender_socket.clone();
        Some(tokio::spawn(async move {
            ping_slave_alive(ping_socket, &remote.to_string()).await;
        }))
    };

    let udp_sender_task = tokio::spawn(async move {
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

    if let Some(ping_task) = ping_alive_task {
        let _ = tokio::join!(udp_sender_task, udp_receiver_task, ping_task);
    } else {
        let _ = tokio::join!(udp_sender_task, udp_receiver_task);
    }

}