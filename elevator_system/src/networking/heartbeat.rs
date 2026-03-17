use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;

use tokio::sync::{
    mpsc::UnboundedSender as UTx,
};
use tokio::time::{self, Duration};

use crate::USER;
use crate::networking::address::{bind, random_port_addr, local_addr};

pub async fn heartbeat_runner(
    my_id: u8,
    all_addrs: HashMap<u8, SocketAddr>,
    ping_tx: UTx<u8>,
) {
    let recv_port = if USER == "MAC" { 30000 + my_id as u16 } else { 30000 };
    let recv_socket = bind(&local_addr(my_id, recv_port)).await;
    let send_socket = bind(&random_port_addr(my_id)).await;

    let addrs = all_addrs.clone();
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_millis(10));
        let payload = my_id.to_be_bytes();

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    for (&id, &addr) in &addrs {
                        let _ = send_socket.send_to(&payload, addr).await;
                    }
                }
            }
        }
    });

    let mut buf = [0u8; 64];
    loop {
        match recv_socket.recv_from(&mut buf).await {
            Ok((n, _)) if n > 0 => { let _ = ping_tx.send(buf[0]); }
            _ => continue,
        }
    }
}