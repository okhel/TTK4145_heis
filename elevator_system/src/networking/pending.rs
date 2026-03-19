use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::networking::types::Msg;

pub type PendingMap = Arc<Mutex<HashMap<u32, (HashSet<u8>, Msg)>>>;

// remove peer from pending and return msg if this was the last peer
pub async fn resolve_peer(
    pending: &PendingMap,
    seq: u32,
    peer: u8,
) -> Option<Msg> {
    let mut map = pending.lock().await;
    
    let all_done = map
        .get_mut(&seq)
        .map(|(remaining, _)| {
            remaining.remove(&peer);
            remaining.is_empty()
        })
        .unwrap_or(false);

    if all_done {
        map.remove(&seq).map(|(_, msg)| msg)
    } else {
        None
    }
}

// remove pending entries for dead elevators and return completed msgs
pub async fn remove_dead_elevators (
    pending: &PendingMap,
    alive: &HashSet<u8>,
) -> Vec<(u32, Msg)> {
    let mut map = pending.lock().await;
    let completed: Vec<u32> = map
        .iter_mut()
        .filter_map(|(&seq, (remaining, _))| {
            remaining.retain(|id| alive.contains(id));
            remaining.is_empty().then_some(seq)
        })
        .collect();

    completed
        .into_iter()
        .filter_map(|seq| map.remove(&seq).map(|(_, msg)| (seq, msg)))
        .collect()
}