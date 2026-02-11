use tokio::{net::{ToSocketAddrs, UdpSocket}, select, time};
use std::{sync::Arc, time::Duration};
use serde::{Serialize, de::DeserializeOwned};
use std::net::SocketAddr;
use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx, unbounded_channel as uc};


pub async fn init_socket(local: &String) -> Arc<UdpSocket> {
    let local_addr = format!("0.0.0.0:200{}", local);
    let sock = UdpSocket::bind(local_addr).await.unwrap();
    let mysock: Arc<UdpSocket> = Arc::new(sock);

    mysock.clone()
}

pub async fn ping_alive(send_sock: Arc<UdpSocket>, mut highest: u32, remote: String){
    let remote_addr = format!("10.100.23.21:200{}", remote);
    loop {
        highest+=1;
        send_sock.send_to(&highest.to_be_bytes(), &remote_addr).await.unwrap();
        println!("Sent highest: {}", highest);
        time::sleep(Duration::from_millis(1000)).await;
    }
}


pub const MAGIC: [u8; 4] = *b"EVL1";       // tag to make sure packet is sent from us, kinda redundant might delete later 


// returns bytes sent 
pub async fn send_msg<T: Serialize>(
    sock: Arc<UdpSocket>,
    addr: &impl ToSocketAddrs,
    msg: &T,
) -> usize {
    let payload = bincode::serialize(msg).expect("bincode serialize failed");

    let mut pkt = Vec::with_capacity(4 + payload.len());
    pkt.extend_from_slice(&MAGIC);
    pkt.extend_from_slice(&payload);

    sock.send_to(&pkt, addr).await.expect("udp send_to failed")
}


pub async fn recv_msg<T: DeserializeOwned>(
    sock: Arc<UdpSocket>,
) -> (T, SocketAddr) {
    let mut buf  = [0; 1024];
    let (n, from) = sock.recv_from(&mut buf).await.expect("udp recv_from failed");
    let data = &buf[..n];

    assert!(data.len() >= 4, "packet too short");
    assert!(data[..4] == MAGIC, "bad magic");

    let msg: T = bincode::deserialize(&data[4..]).expect("bincode deserialize failed");
    (msg, from)
}

pub async fn udp_sender(socket:Arc<UdpSocket>, addr: String, mut at_floor_rx: URx<u8>) {
    loop {
        select! {
            Some(floor) = at_floor_rx.recv() => {
                send_msg::<u8>(socket.clone(), &addr, &floor).await;
            }
        }
    }
}

pub async fn udp_receiver(socket:Arc<UdpSocket>, udp_received_tx:UTx<u8>) {
    loop {
        let (msg, addr) = recv_msg::<u8>(socket.clone()).await;
        println!("{}", &msg);
        let _ = udp_received_tx.send(msg);
    }
}

pub async fn network_runner(mut at_floor_rx: URx<u8>, local: u8, remote: u8){
    let (udp_received_tx, udp_received_rx) = uc::<u8>();
    let socket = init_socket(&local.to_string()).await;
    let sender_socket = socket.clone();
    let receiver_socket = socket.clone();
    
    let udp_sender_task = tokio::spawn(async move {
        udp_sender(sender_socket, format!("localhost:200{}", remote), at_floor_rx).await});
    let udp_receiver_task = tokio::spawn(async move {
        udp_receiver(receiver_socket, udp_received_tx).await}); 

    let _ = tokio::join!(udp_sender_task, udp_receiver_task);

    loop {
        
    }
}