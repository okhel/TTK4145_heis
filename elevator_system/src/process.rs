use std::{time::Duration};
use std::sync::{Arc};
use tokio::{time, net::UdpSocket};

pub async fn master_check(sock: Arc<UdpSocket>) -> u32{

    let mut buf  = [0; 4];
    let mut highest = 0;

    loop {
        if let Err(_) = time::timeout_at(time::Instant::now() + Duration::from_millis(3000), sock.recv(&mut buf)).await {
            println!("did not receive value within 3000 ms");
            return highest;
        }
        if u32::from_be_bytes(buf) > highest {
            highest = u32::from_be_bytes(buf);
            println!("Received highest: {}", highest);
        }

    }
}