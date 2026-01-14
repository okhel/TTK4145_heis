use tokio::{net::UdpSocket, time};
use std::{io, time::Duration, sync::Arc};

#[tokio::main]
async fn main() -> io::Result<()> {
    let remote_addr = "10.100.23.11:20011";
    let local_addr = "0.0.0.0:20011";
    let sock = UdpSocket::bind(local_addr).await?;
    let mysock: Arc<UdpSocket> = Arc::new(sock);
    let recv_sock: Arc<UdpSocket> = mysock.clone();
    let send_sock: Arc<UdpSocket> = mysock.clone();


    // receiving
    let recv_task = tokio::spawn( async move {
        loop {
            let mut buf= [0; 1024];
            let (n, _addr) = recv_sock.recv_from(&mut buf).await.unwrap();
            println!("Received message: {:?}", String::from_utf8_lossy(&buf[..n]));
        }
    });

    // sending
    let send_task = tokio::spawn( async move {
        loop {
            send_sock.send_to(b"This is a test message", remote_addr).await.unwrap();
            println!("Sent message");
            time::sleep(Duration::from_millis(1000)).await;
        }
    });

    let _ = tokio::join!(send_task, recv_task);
    Ok(())
}
