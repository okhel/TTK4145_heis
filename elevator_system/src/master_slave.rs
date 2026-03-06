use std::collections::HashMap;

use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx};
use tokio::time::{self, Duration, Instant};

const PEER_TIMEOUT: Duration = Duration::from_millis(3000);

pub async fn master_check(local_id: u8, mut ping_rx: URx<u8>, elevs_alive_tx: UTx<Vec<u8>>) {
    let mut peer_last_seen: HashMap<u8, Instant> = HashMap::new();
    let mut check_interval = time::interval(Duration::from_millis(1000));

    loop {
        tokio::select! {
            Some(peer_id) = ping_rx.recv() => {
                peer_last_seen.insert(peer_id, Instant::now());
                let alive = store_alive_elevators(local_id, &peer_last_seen);
                let _ = elevs_alive_tx.send(alive);
            }
            _ = check_interval.tick() => {
                let before = peer_last_seen.len();
                peer_last_seen.retain(|_, last| last.elapsed() < PEER_TIMEOUT);
                if peer_last_seen.len() != before {
                    let alive = store_alive_elevators(local_id, &peer_last_seen);
                    let _ = elevs_alive_tx.send(alive);
                }
            }
        }
    }
}

fn store_alive_elevators(local_id: u8, peers: &HashMap<u8, Instant>) -> Vec<u8> {
    let mut alive: Vec<u8> = vec![local_id];
    alive.extend(
        peers
            .iter()
            .filter(|(_, last)| last.elapsed() < PEER_TIMEOUT)
            .map(|(&id, _)| id),
    );
    alive.sort();
    alive
}
