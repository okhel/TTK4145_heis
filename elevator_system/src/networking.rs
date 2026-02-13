use tokio::{net::{ToSocketAddrs, UdpSocket}, select, time};
use std::{collections::HashMap, sync::Arc, time::Duration};
use serde::{Serialize, de::DeserializeOwned};
use std::net::SocketAddr;
use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx, unbounded_channel as uc};


pub async fn init_socket(local_id: &String) -> Arc<UdpSocket> {
    let local_addr = format!("192.168.0.155:280{}", local_id);
    let sock = UdpSocket::bind(local_addr).await.unwrap();
    let mysock: Arc<UdpSocket> = Arc::new(sock);

    mysock.clone()
}

pub async fn ping_alive_sender(send_sock: Arc<UdpSocket>, id: u8, remote_ids: Vec<u8>) {
    loop {
        for remote_id in &remote_ids {
            let remote_addr = format!("192.168.0.155:280{}", remote_id);
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

pub async fn store_online_elevators(mut ping_received_rx: URx<u8>) {
    let mut online_elevators: HashMap<u8, time::Instant> = HashMap::new();
    let timeout_duration = Duration::from_millis(5000);
    loop {
        tokio::select! {
            Some(received_id) = ping_received_rx.recv() => {
                online_elevators.insert(received_id, time::Instant::now());
            }
            
            _ = time::sleep(Duration::from_millis(500)) => {
                let now = time::Instant::now();
                online_elevators.retain(|_id, last_seen| {
                    now.duration_since(*last_seen) < timeout_duration
                });
                println!("Online elevators: {:?}", online_elevators.keys())
            }
        }
    }
}

pub const MAGIC: [u8; 4] = *b"EVL1";       // tag to make sure packet is sent from us, kinda redundant might delete later 

pub async fn network_runner(mut at_floor_rx: URx<u8>, local_id: u8, remote_ids: Vec<u8>){
    let (ping_received_tx, ping_received_rx) = uc::<u8>();
    let socket = init_socket(&local_id.to_string()).await;
    let sender_socket = socket.clone();
    let receiver_socket = socket.clone();
    
    let ping_alive_sender_task = tokio::spawn(async move {
        ping_alive_sender(sender_socket.clone(), local_id, remote_ids).await});
    let ping_alive_receiver_task = tokio::spawn(async move {
        ping_alive_receiver(receiver_socket.clone(), ping_received_tx).await});
    let store_online_elevators_task = tokio::spawn(async move {
        store_online_elevators(ping_received_rx).await});

    // let udp_sender_task = tokio::spawn(async move {
    //     udp_sender(sender_socket, format!("192.168.0.155:280{}", 0), at_floor_rx).await});
    // let udp_receiver_task = tokio::spawn(async move {
    //     udp_receiver(receiver_socket, udp_received_tx).await}); 

    let _ = tokio::join!(ping_alive_sender_task, ping_alive_receiver_task, store_online_elevators_task);

}

// returns bytes sent 
// pub async fn send_msg<T: Serialize>(
//     sock: Arc<UdpSocket>,
//     addr: &impl ToSocketAddrs,
//     msg: &T,
// ) -> usize {
//     let payload = bincode::serialize(msg).expect("bincode serialize failed");

//     let mut pkt = Vec::with_capacity(4 + payload.len());
//     pkt.extend_from_slice(&MAGIC);
//     pkt.extend_from_slice(&payload);

//     sock.send_to(&pkt, addr).await.expect("udp send_to failed")
// }


// pub async fn recv_msg<T: DeserializeOwned>(
//     sock: Arc<UdpSocket>,
// ) -> (T, SocketAddr) {
//     let mut buf  = [0; 1024];
//     let (n, from) = sock.recv_from(&mut buf).await.expect("udp recv_from failed");
//     let data = &buf[..n];

//     assert!(data.len() >= 4, "packet too short");
//     assert!(data[..4] == MAGIC, "bad magic");

//     let msg: T = bincode::deserialize(&data[4..]).expect("bincode deserialize failed");
//     (msg, from)
// }

// pub async fn udp_sender(socket:Arc<UdpSocket>, addr: String, mut at_floor_rx: URx<u8>) {
//     loop {
//         select! {
//             Some(floor) = at_floor_rx.recv() => {
//                 let port = socket.local_addr().unwrap().port();
//                 println!("UDP sender task started {} with port", port);
//                 send_msg::<u8>(socket.clone(), &addr, &floor).await;
//             }
//         }
//     }
// }

// pub async fn udp_receiver(socket:Arc<UdpSocket>, udp_received_tx:UTx<u8>) {
//     loop {
//         let (msg, addr) = recv_msg::<u8>(socket.clone()).await;
//         println!("Received message {} from {}", &msg, addr);
//         let _ = udp_received_tx.send(msg);
//     }
// }
