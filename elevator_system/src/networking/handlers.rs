use std::collections::HashSet;
use std::net::SocketAddr;

use crate::types::NetworkEvent;

use super::NetworkState;
use super::address::{bind, random_port_addr};
use super::pending::{remove_dead_elevators, resolve_peer};
use super::transport::send_reliable;
use super::types::Msg;
use super::{DEDUP_WINDOW, MAX_APP_RETRIES};

use tokio::time::Instant;

impl NetworkState {
    pub(super) async fn on_alive_update(&mut self, alive: Vec<u8>) {
        println!("[Rx]Online elevators: {:?}", alive);
        let alive_set = self.peers.update(&alive, self.my_id);

        for (_seq, msg) in remove_dead_elevators(&self.pending, &alive_set).await {
            let _ = self.event_tx.send(NetworkEvent::AckComplete(msg));
        }
    }

    pub(super) async fn on_outgoing(&mut self, msg: Msg) {
        let targets: Vec<u8> = if self.peers.is_master() {
            self.peers.ids().to_vec()
        } else {
            self.peers.master_id().into_iter().collect()
        };

        if targets.is_empty() {
            let _ = self.event_tx.send(NetworkEvent::AckComplete(msg));
        } else {
            let expected: HashSet<u8> = targets.iter().copied().collect();
            self.pending.lock().await.insert(self.seq, (expected, msg.clone()));
            for id in targets {
                if let Some(&addr) = self.peers.addr(id) {
                    self.spawn_send_task(msg.clone(), addr, self.seq, id, 0);
                }
            }
        }
        self.seq = self.seq.wrapping_add(1);
    }

    pub(super) async fn on_ack(&self, seq: u32, peer: u8) {
        if let Some(msg) = resolve_peer(&self.pending, seq, peer).await {
            let _ = self.event_tx.send(NetworkEvent::AckComplete(msg));
        }
    }

    pub(super) async fn on_send_failure(
        &self, seq: u32, failed_peer: u8, msg: Msg, addr: SocketAddr, retry: u32,
    ) {
        // if master dies mid-send, redirect to new master
        if !self.peers.is_master()
            && let Some(new_master) = self.peers.master_id().filter(|&m| m != failed_peer)
            && let Some(&new_addr) = self.peers.addr(new_master)
        {
            let mut map = self.pending.lock().await;
            if let Some((remaining, _)) = map.get_mut(&seq) {
                remaining.remove(&failed_peer);
                remaining.insert(new_master);
            }
            drop(map);
            self.spawn_send_task(msg, new_addr, seq, new_master, 0);
            return;
        }

        // retry if transient failure
        if retry < MAX_APP_RETRIES && self.peers.ids().contains(&failed_peer) {
            self.spawn_send_task(msg, addr, seq, failed_peer, retry + 1);
            return;
        }

        // give up
        eprintln!("Permanent send failure seq={seq} to peer {failed_peer} after {retry} retries");
        if let Some(msg) = resolve_peer(&self.pending, seq, failed_peer).await {
            let _ = self.event_tx.send(NetworkEvent::AckComplete(msg));
        }
    }

    pub(super) fn on_incoming(&mut self, msg: Msg, seq: u32, sender: SocketAddr) {
        let now = Instant::now();
        self.seen.retain(|_, t| now.duration_since(*t) < DEDUP_WINDOW);

        if self.seen.insert((seq, sender), now).is_none() {
            let _ = self.event_tx.send(NetworkEvent::Message(msg));
        }
    }

    fn spawn_send_task(&self, msg: Msg, addr: SocketAddr, seq: u32, peer_id: u8, retry: u32) {
        let my_id = self.send_ctx.my_id;
        let ack_tx = self.send_ctx.ack_tx.clone();
        let fail_tx = self.send_ctx.fail_tx.clone();

        tokio::spawn(async move {
            let socket = bind(&random_port_addr(my_id)).await;
            match send_reliable(socket, msg.clone(), addr, seq).await {
                Ok(()) => { let _ = ack_tx.send((seq, peer_id)); }
                Err(_) => { let _ = fail_tx.send((seq, peer_id, msg, addr, retry)); }
            }
        });
    }
}
