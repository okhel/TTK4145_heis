mod address;
mod heartbeat;
mod pending;
mod transport;
pub mod types;
mod peers;
mod handlers;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::{
    broadcast::Receiver as BcRx,
    mpsc::unbounded_channel as uc,
    mpsc::UnboundedSender as UTx,
    Mutex,
};
use tokio::time::{Duration, Instant};

use crate::USER;
use crate::types::{NetworkEvent, NetworkInternal};

use address::{bind, random_port_addr, local_addr, peer_ping_addr};
use heartbeat::heartbeat_runner;
use pending::PendingMap;
use peers::Peers;
use types::Msg;

const MAX_APP_RETRIES: u32 = 10000;
const DEDUP_WINDOW: Duration = Duration::from_secs(3);

struct SendCtx {
    my_id: u8,
    ack_tx: UTx<(u32, u8)>,
    fail_tx: UTx<(u32, u8, Msg, SocketAddr, u32)>,
}

struct NetworkState {
    my_id: u8,
    peers: Peers,
    pending: PendingMap,
    send_ctx: SendCtx,
    event_tx: UTx<NetworkEvent>,
    seq: u32,
    seen: HashMap<(u32, SocketAddr), Instant>,
}

pub async fn network_runner(
    my_id: u8,
    remote_ids: Vec<u8>,
    internal: NetworkInternal,
    ping_tx: UTx<u8>,
    mut alive_rx: BcRx<Vec<u8>>,
) {
    let NetworkInternal { mut inbox, event_tx } = internal;
    let recv_port = if USER == "MAC" { 21000 + my_id as u16 } else { 21000 };
    let recv_socket = bind(&local_addr(my_id, recv_port)).await;
    let ack_socket = bind(&random_port_addr(my_id)).await;

    let ping_addrs: HashMap<u8, SocketAddr> =
        remote_ids.iter().map(|&id| (id, peer_ping_addr(id))).collect();

    tokio::spawn(heartbeat_runner(my_id, ping_addrs, ping_tx));

    let (ack_tx, mut ack_rx) = uc::<(u32, u8)>();
    let (fail_tx, mut fail_rx) = uc::<(u32, u8, Msg, SocketAddr, u32)>();

    let mut state = NetworkState {
        my_id,
        peers: Peers::new(),
        pending: Arc::new(Mutex::new(HashMap::new())),
        send_ctx: SendCtx { my_id, ack_tx, fail_tx },
        event_tx,
        seq: 0,
        seen: HashMap::new(),
    };

    loop {
        tokio::select! {
            result = alive_rx.recv() => {
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
                state.on_alive_update(alive).await;
            }

            Some(msg) = inbox.recv() => state.on_outgoing(msg).await,

            Some((seq, peer)) = ack_rx.recv() => state.on_ack(seq, peer).await,

            Some((seq, peer, msg, addr, retry)) = fail_rx.recv() => {
                state.on_send_failure(seq, peer, msg, addr, retry).await;
            }

            Ok((msg, msg_seq, sender)) = transport::recv_reliable(&recv_socket, &ack_socket) => {
                state.on_incoming(msg, msg_seq, sender);
            }
        }
    }
}
