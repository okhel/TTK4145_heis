use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use crate::USER;

use tokio::net::UdpSocket;
use tokio::sync::{
    Mutex, mpsc::UnboundedReceiver as URx, mpsc::UnboundedSender as UTx,
    mpsc::unbounded_channel as uc,
    broadcast::{Receiver as BcRx},
};
use tokio::time::{self, Duration, Instant};

use crate::networking::transport::{recv_reliable, send_reliable};
use crate::networking::types::Msg;

pub mod transport;
pub mod types;

const MAX_APP_RETRIES: u32 = 20;
const DEDUP_WINDOW: Duration = Duration::from_secs(3);

async fn init_socket(id:u8, port: u16) -> Arc<UdpSocket> {
    let addr: String;
    if USER == "MAC" {
        addr = format!("127.0.0.1:{}", port);
    } else {
        addr = format!("10.100.23.{}:{}", id, port);
    }
    Arc::new(UdpSocket::bind(addr).await.unwrap())
}

fn spawn_send_task(
    msg: Msg,
    addr: SocketAddr,
    seq: u32,
    my_id: u8,
    peer_id: u8,
    retry_count: u32,
    ack_tx: UTx<(u32, u8)>,
    fail_tx: UTx<(u32, u8, Msg, SocketAddr, u32)>,
) {
    tokio::spawn(async move {
        let socket = Arc::new( 
            if USER == "MAC" {
                UdpSocket::bind("127.0.0.1:0").await.unwrap()
            } else {
                UdpSocket::bind(format!("10.100.23.{}:0", my_id)).await.unwrap()
            }
    );
        match send_reliable(socket, msg.clone(), addr, seq).await {
            Ok(()) => { let _ = ack_tx.send((seq, peer_id)); }
            Err(_) => { let _ = fail_tx.send((seq, peer_id, msg, addr, retry_count)); }
        }
    });
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
    mut alive_rx: BcRx<Vec<u8>>,
    // ack notification
    ack_complete_tx: UTx<(u32, Msg)>,
) {
    let recv_socket:Arc<UdpSocket>;
    if USER == "MAC" {
        recv_socket = init_socket(my_id, 21000 + my_id as u16).await;
    } else {
        recv_socket = init_socket(my_id, 21000).await;
        println!("Sender success");
    }

    // Heartbeat task runs independently so message processing can never delay pings
    let ping_addrs: Vec<String>;
    if USER == "MAC" {
        ping_addrs = remote_ids
            .iter()
            .map(|id| format!("127.0.0.1:{}", 30000 + *id as u16))
            .collect();
    } else {
        ping_addrs = remote_ids
            .iter()
            .map(|id| format!("10.100.23.{}:30000", *id as u16))
            .collect();
    }
    
    tokio::spawn(heartbeat_runner(my_id, ping_addrs, ping_tx));

    let mut seq: u32 = 0;
    let mut is_master = false;
    let mut master_id: Option<u8> = None;
    let mut peer_ids: Vec<u8> = Vec::new();
    let mut peer_addrs: HashMap<u8, String> = HashMap::new();

    // ACK tracking
    let pending_acks: Arc<Mutex<HashMap<u32, (HashSet<u8>, Msg)>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let (ack_tx, mut ack_rx) = uc::<(u32, u8)>();
    let (fail_tx, mut fail_rx) = uc::<(u32, u8, Msg, SocketAddr, u32)>();

    // Deduplication: track recently seen (seq, sender_addr) to ignore retransmissions
    let mut seen_messages: HashMap<(u32, SocketAddr), Instant> = HashMap::new();

    loop {
        tokio::select! {
            // receive role updates from master_slave
            result = alive_rx.recv() => {
                let alive = match result {
                    Ok(alive) => alive,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("alive_rx lagged by {n} messages, will catch up next iteration");
                        continue;
                    }
                    Err(_) => continue,
                };
                master_id = alive.first().copied();
                is_master = master_id == Some(my_id);
                peer_ids = alive.iter().filter(|&&id| id != my_id).cloned().collect();
                if USER == "MAC" {
                    peer_addrs = alive.iter()
                        .filter(|&&id| id != my_id)
                        .map(|&id| (id, format!("127.0.0.1:{}", 21000 + id as u16)))
                        .collect();
                } else {
                    peer_addrs = alive.iter()
                        .filter(|&&id| id != my_id)
                        .map(|&id| (id, format!("10.100.23.{}:21000", id as u16)))
                        .collect();
                }
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
                            let addr: std::net::SocketAddr = addr.parse().unwrap();
                            spawn_send_task(msg.clone(), addr, seq, my_id, id, 0, ack_tx.clone(), fail_tx.clone());
                        };
                    }
                    seq = seq.wrapping_add(1);
                }
                else {
                    // No peers to send to, consider it immediately ACKed
                    let _ = ack_complete_tx.send((seq, msg));
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

            // handle send failures with app-level retry
            Some((fail_seq, peer_id, msg, addr, retry_count)) = fail_rx.recv() => {
                if !is_master && master_id.is_some() && master_id != Some(peer_id) {
                    // Master changed mid-send — redirect to new master
                    let new_master = master_id.unwrap();
                    if let Some(new_addr_str) = peer_addrs.get(&new_master) {
                        let new_addr: SocketAddr = new_addr_str.parse().unwrap();
                        let mut map = pending_acks.lock().await;
                        if let Some((remaining, _)) = map.get_mut(&fail_seq) {
                            remaining.remove(&peer_id);
                            remaining.insert(new_master);
                        }
                        spawn_send_task(
                            msg, new_addr, fail_seq, my_id, new_master, 0,
                            ack_tx.clone(), fail_tx.clone(),
                        );
                    } else {
                        let mut map = pending_acks.lock().await;
                        let all_done = if let Some((remaining, _)) = map.get_mut(&fail_seq) {
                            remaining.remove(&peer_id);
                            remaining.is_empty()
                        } else {
                            false
                        };
                        if all_done {
                            if let Some((_, msg)) = map.remove(&fail_seq) {
                                let _ = ack_complete_tx.send((fail_seq, msg));
                            }
                        }
                    }
                } else if retry_count < MAX_APP_RETRIES && peer_ids.contains(&peer_id){
                    spawn_send_task(
                        msg, addr, fail_seq, my_id, peer_id, retry_count + 1,
                        ack_tx.clone(), fail_tx.clone(),
                    );
                } else {
                    eprintln!(
                        "Permanent send failure for seq={fail_seq} to peer {peer_id} after {MAX_APP_RETRIES} app retries"
                    );
                    let mut map = pending_acks.lock().await;
                    let all_done = if let Some((remaining, _)) = map.get_mut(&fail_seq) {
                        remaining.remove(&peer_id);
                        remaining.is_empty()
                    } else {
                        false
                    };
                    if all_done {
                        if let Some((_, msg)) = map.remove(&fail_seq) {
                            let _ = ack_complete_tx.send((fail_seq, msg));
                        }
                    }
                }
            }

            // route incoming messages with deduplication
            Ok((msg, msg_seq, sender_addr)) = recv_reliable(&recv_socket) => {
                let now = Instant::now();
                seen_messages.retain(|_, t| now.duration_since(*t) < DEDUP_WINDOW);

                let key = (msg_seq, sender_addr);
                if seen_messages.contains_key(&key) {
                    continue;
                }
                seen_messages.insert(key, now);
                let _ = outbox.send(msg);
            }
        }
    }
}

async fn heartbeat_runner(my_id: u8, ping_addrs: Vec<String>, ping_tx: UTx<u8>) {
    let ping_socket: Arc<UdpSocket>;
    if USER == "MAC" {
        ping_socket = init_socket(my_id, 30000 + my_id as u16).await;
    } else {
        ping_socket = init_socket(my_id, 30000).await;
        println!("Hearbeat success");
    }
    let mut ping_interval = time::interval(Duration::from_millis(5));
    let mut ping_buf = [0u8; 64];

    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                for addr in &ping_addrs {
                    let _ = ping_socket.send_to(&my_id.to_be_bytes(), addr.as_str()).await;
                }
            }

            Ok((n, addr)) = ping_socket.recv_from(&mut ping_buf) => {
                let received_id = ping_buf[..n][0];
                let _ = ping_tx.send(received_id);
            }
        }
    }
}
