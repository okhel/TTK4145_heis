use tokio::{net::UdpSocket, time};
use std::{time::Duration, sync::Arc};

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
