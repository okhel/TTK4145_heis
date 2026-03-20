use std::collections::HashSet;
use super::types::Role;

pub struct MembershipChange {
    pub newly_alive: Vec<usize>,
    pub lost: Vec<usize>,
    pub became_master: bool,
}

pub struct MembershipState {
    alive_elevs: HashSet<usize>,
    role: Role,
    network_ready: bool,
}

impl MembershipState {
    pub fn new() -> Self {
        Self {
            alive_elevs: HashSet::new(),
            role: Role::Slave,
            network_ready: false,
        }
    }

    pub fn update_membership(&mut self, alive_ids: &[u8], local_id: u8) -> MembershipChange {
        let new_set: HashSet<usize> = alive_ids.iter().map(|id| *id as usize).collect();
        let old_set = self.alive_elevs.clone();
        let newly_alive: Vec<usize> = new_set.difference(&old_set).copied().collect();
        let lost: Vec<usize> = old_set.difference(&new_set).copied().collect();

        self.alive_elevs = new_set.clone();
        self.network_ready = true;

        let mut became_master = false;

        if alive_ids.iter().min() == Some(&local_id) {
            self.role = Role::Master;
            became_master = old_set.is_empty() || old_set.iter().min() != new_set.iter().min();
        } else {
            self.role = Role::Slave;
        }

        MembershipChange { newly_alive, lost, became_master }
    }

    pub fn is_master(&self) -> bool {
        self.role == Role::Master
    }

    pub fn role(&self) -> &Role {
        &self.role
    }

    pub fn is_network_ready(&self) -> bool {
        self.network_ready
    }

    pub fn alive(&self) -> &HashSet<usize> {
        &self.alive_elevs
    }

    pub fn alive_vec(&self) -> Vec<usize> {
        self.alive_elevs.iter().copied().collect()
    }
}
