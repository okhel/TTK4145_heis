use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::net::UdpSocket;
use tokio::sync::{
    Mutex, mpsc::UnboundedReceiver as URx, mpsc::UnboundedSender as UTx,
    mpsc::unbounded_channel as uc,
};
use tokio::time::{self, Duration};

use crate::networking::transport::{recv_reliable, send_reliable};
use crate::networking::types::Msg;

pub mod transport;
pub mod types;

async fn init_socket(port: u16) -> Arc<UdpSocket> {
    let addr = format!("localhost:{}", port);
    Arc::new(UdpSocket::bind(addr).await.unwrap())
}

pub async fn network_runner(
    my_id: u8,
    remote_ids: Vec<u8>,
    // message passing
    mut inbox: URx<Msg>,
    outbox: UTx<Msg>,
    // pings out to master_slave module
    ping_tx: UTx<u8>,
    // alive list from master_slave module
    mut alive_rx: URx<Vec<u8>>,
    // ack notification
    ack_complete_tx: UTx<(u32, Msg)>,
) {
    let send_socket = init_socket(20000 + my_id as u16).await;
    let recv_socket = init_socket(21000 + my_id as u16).await;
    let ping_socket = init_socket(30000 + my_id as u16).await;

    let ping_addrs: Vec<String> = remote_ids
        .iter()
        .map(|id| format!("localhost:{}", 30000 + *id as u16))
        .collect();

    let mut seq: u32 = 0;
    let mut is_master = false;
    let mut master_id: Option<u8> = None;
    let mut peer_ids: Vec<u8> = Vec::new();
    let mut peer_addrs: HashMap<u8, String> = HashMap::new();

    // ACK tracking
    let pending_acks: Arc<Mutex<HashMap<u32, (HashSet<u8>, Msg)>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let (ack_tx, mut ack_rx) = uc::<(u32, u8)>();

    let mut ping_interval = time::interval(Duration::from_millis(1000));
    let mut ping_buf = [0u8; 64];

    loop {
        tokio::select! {
            // send heartbeats
            _ = ping_interval.tick() => {
                for addr in &ping_addrs {
                    let _ = ping_socket.send_to(&my_id.to_be_bytes(), addr.as_str()).await;
                }
            }

            // receive and forward pings to master_slave
            Ok((n, _)) = ping_socket.recv_from(&mut ping_buf) => {
                let received_id = ping_buf[..n][0];
                let _ = ping_tx.send(received_id);
            }

            // receive role updates from master_slave
            Some(alive) = alive_rx.recv() => {
                master_id = alive.first().copied();
                is_master = master_id == Some(my_id);
                peer_ids = alive.iter().filter(|&&id| id != my_id).cloned().collect();
                peer_addrs = alive.iter()
                    .filter(|&&id| id != my_id)
                    .map(|&id| (id, format!("localhost:{}", 20000 + id as u16)))
                    .collect();
            }

            // route message based on role
            Some(msg) = inbox.recv() => {
                let target_ids: Vec<u8> = if is_master {
                    peer_ids.clone()
                } else if let Some(m_id) = master_id {
                    vec![m_id]
                } else {
                    vec![]
                };

                if !target_ids.is_empty() {
                    let expected: HashSet<u8> = target_ids.iter().cloned().collect();
                    pending_acks.lock().await.insert(seq, (expected, msg.clone()));

                    for id in target_ids {
                        if let Some(addr) = peer_addrs.get(&id) {
                            let socket = send_socket.clone();
                            let msg = msg.clone();
                            let ack_tx = ack_tx.clone();
                            let addr: std::net::SocketAddr = addr.parse().unwrap();
                            tokio::spawn(async move {
                                send_reliable(socket, msg, addr, seq).await;
                                let _ = ack_tx.send((seq, id));
                            });
                        }
                    }
                    seq = seq.wrapping_add(1);
                }
            }

            // track ACKs
            Some((ack_seq, peer_id)) = ack_rx.recv() => {
                let mut map = pending_acks.lock().await;
                let all_acked = if let Some((remaining, _)) = map.get_mut(&ack_seq) {
                    remaining.remove(&peer_id);
                    remaining.is_empty()
                } else {
                    false
                };
                if all_acked {
                    if let Some((_, msg)) = map.remove(&ack_seq) {
                        let _ = ack_complete_tx.send((ack_seq, msg));
                    }
                }
            }

            // route incoming messages
            Ok((msg, _, _)) = recv_reliable(&recv_socket) => {
                let _ = outbox.send(msg);
            }
        }
    }
}
