mod address;
mod heartbeat;
mod pending;
pub mod transport;
pub mod types;

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::{
    broadcast::Receiver as BcRx,
    mpsc::unbounded_channel as uc,
    mpsc::UnboundedReceiver as URx,
    mpsc::UnboundedSender as UTx,
    Mutex,
};
use tokio::time::{Duration, Instant, interval};

use crate::USER;
use address::{bind, random_port_addr, local_addr, peer_msg_addr, peer_ping_addr};
use heartbeat::heartbeat_runner;
use pending::{remove_dead_elevators, resolve_peer, PendingMap};
use transport::{recv_reliable, send_reliable};
use types::Msg;

const MAX_APP_RETRIES: u32 = 20;
const DEDUP_WINDOW: Duration = Duration::from_secs(3);

struct SendCtx {
    my_id: u8,
    ack_tx: UTx<(u32, u8)>,
    fail_tx: UTx<(u32, u8, Msg, SocketAddr, u32)>,
}

fn spawn_send_task(
    ctx: &SendCtx,
    msg: Msg,
    addr: SocketAddr,
    seq: u32,
    peer_id: u8,
    retry: u32,
) {
    let my_id = ctx.my_id;
    let ack_tx = ctx.ack_tx.clone();
    let fail_tx = ctx.fail_tx.clone();

    tokio::spawn(async move {
        let socket = bind(&random_port_addr(my_id)).await;
        match send_reliable(socket, msg.clone(), addr, seq).await {
            Ok(()) => { let _ = ack_tx.send((seq, peer_id)); }
            Err(_) => { let _ = fail_tx.send((seq, peer_id, msg, addr, retry)); }
        }
    });
}


pub async fn network_runner(
    my_id: u8,
    remote_ids: Vec<u8>,
    mut inbox: URx<Msg>,
    outbox: UTx<Msg>,
    ping_tx: UTx<u8>,
    mut alive_rx: BcRx<Vec<u8>>,
    ack_complete_tx: UTx<(u32, Msg)>,
) {
    let recv_port = if USER == "MAC" { 21000 + my_id as u16 } else { 21000 };
    let recv_socket = bind(&local_addr(my_id, recv_port)).await;
    let ack_socket = bind(&random_port_addr(my_id)).await;

    let ping_addrs: HashMap<u8, SocketAddr> =
        remote_ids.iter().map(|&id| (id, peer_ping_addr(id))).collect();

    tokio::spawn(heartbeat_runner(
        my_id,
        ping_addrs,
        ping_tx,
    ));

    let mut seq: u32 = 0;
    let mut is_master = false;
    let mut master_id: Option<u8> = None;
    let mut peer_ids: Vec<u8> = Vec::new();
    let mut peer_addrs: HashMap<u8, SocketAddr> = HashMap::new();

    let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
    let (ack_tx, mut ack_rx) = uc::<(u32, u8)>();
    let (fail_tx, mut fail_rx) = uc::<(u32, u8, Msg, SocketAddr, u32)>();
    let send_ctx = SendCtx { my_id, ack_tx, fail_tx };

    let mut seen: HashMap<(u32, SocketAddr), Instant> = HashMap::new();

    loop {
        tokio::select! {
            // update alive list when new elevs
            result = alive_rx.recv() => {
                println!("[Rx]Online elevators{:?}", result);
                let alive = match result {
                    Ok(a) => a,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        match alive_rx.recv().await {
                            Ok(a) => a,
                            _ => continue,
                        }
                    }
                    Err(_) => continue,
                };

                let alive_set: HashSet<u8> = alive.iter().copied().collect();
                master_id = alive.first().copied();
                is_master = master_id == Some(my_id);
                peer_ids = alive.iter().filter(|&&id| id != my_id).copied().collect();
                peer_addrs = peer_ids.iter().map(|&id| (id, peer_msg_addr(id))).collect();

                for (seq, msg) in remove_dead_elevators(&pending, &alive_set).await {
                    let _ = ack_complete_tx.send((seq, msg));
                }
            }

            // outgoing messages
            Some(msg) = inbox.recv() => {
                let targets: Vec<u8> = if is_master {
                    peer_ids.clone()
                } else {
                    master_id.into_iter().collect()
                };

                if targets.is_empty() {
                    let _ = ack_complete_tx.send((seq, msg));
                } else {
                    let expected: HashSet<u8> = targets.iter().copied().collect();
                    pending.lock().await.insert(seq, (expected, msg.clone()));
                    for id in targets {
                        if let Some(&addr) = peer_addrs.get(&id) {
                            spawn_send_task(&send_ctx, msg.clone(), addr, seq, id, 0);
                        }
                    }
                }
                seq = seq.wrapping_add(1);
            }

            // ack received
            Some((ack_seq, peer)) = ack_rx.recv() => {
                if let Some(msg) = resolve_peer(&pending, ack_seq, peer).await {
                    let _ = ack_complete_tx.send((ack_seq, msg));
                }
            }

            // send failure/retry
            Some((fail_seq, peer, msg, addr, retry)) = fail_rx.recv() => {
                handle_send_failure(
                    &send_ctx, &pending, &ack_complete_tx,
                    &peer_addrs, &peer_ids,
                    is_master, master_id,
                    fail_seq, peer, msg, addr, retry,
                ).await;
            }

            // incoming message
            Ok((msg, msg_seq, sender)) = recv_reliable(&recv_socket, &ack_socket) => {
                let now = Instant::now();
                seen.retain(|_, t| now.duration_since(*t) < DEDUP_WINDOW);

                if seen.insert((msg_seq, sender), now).is_none() {
                    let _ = outbox.send(msg);
                }
            }
        }
    }
}



async fn handle_send_failure(
    ctx: &SendCtx,
    pending: &PendingMap,
    ack_complete_tx: &UTx<(u32, Msg)>,
    peer_addrs: &HashMap<u8, SocketAddr>,
    peer_ids: &[u8],
    is_master: bool,
    master_id: Option<u8>,
    seq: u32,
    failed_peer: u8,
    msg: Msg,
    addr: SocketAddr,
    retry: u32,
) {
    // if master dies mid-send, send to new master
    if !is_master {
        if let Some(new_master) = master_id.filter(|&m| m != failed_peer) {
            if let Some(&new_addr) = peer_addrs.get(&new_master) {
                let mut map = pending.lock().await;
                if let Some((remaining, _)) = map.get_mut(&seq) {
                    remaining.remove(&failed_peer);
                    remaining.insert(new_master);
                }
                drop(map);
                spawn_send_task(ctx, msg, new_addr, seq, new_master, 0);
                return;
            }
        }
    };

    // retry if just transient failure
    if retry < MAX_APP_RETRIES && peer_ids.contains(&failed_peer) {
        spawn_send_task(ctx, msg, addr, seq, failed_peer, retry + 1);
        return;
    }

    // give up
    eprintln!("Permanent send failure seq={seq} to peer {failed_peer} after {retry} retries");
    if let Some(msg) = resolve_peer(pending, seq, failed_peer).await {
        let _ = ack_complete_tx.send((seq, msg));
    }
}