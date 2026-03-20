use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;

use super::address::peer_msg_addr;

pub struct Peers {
    is_master: bool,
    master_id: Option<u8>,
    ids: Vec<u8>,
    addrs: HashMap<u8, SocketAddr>,
}

impl Peers {
    pub fn new() -> Self {
        Self {
            is_master: false,
            master_id: None,
            ids: Vec::new(),
            addrs: HashMap::new(),
        }
    }

    pub fn update(&mut self, alive: &[u8], my_id: u8) -> HashSet<u8> {
        let alive_set: HashSet<u8> = alive.iter().copied().collect();
        self.master_id = alive.first().copied();
        self.is_master = self.master_id == Some(my_id);
        self.ids = alive.iter().filter(|&&id| id != my_id).copied().collect();
        self.addrs = self.ids.iter().map(|&id| (id, peer_msg_addr(id))).collect();
        alive_set
    }

    pub fn is_master(&self) -> bool { self.is_master }
    pub fn master_id(&self) -> Option<u8> { self.master_id }
    pub fn ids(&self) -> &[u8] { &self.ids }
    pub fn addr(&self, id: u8) -> Option<&SocketAddr> { self.addrs.get(&id) }
}
