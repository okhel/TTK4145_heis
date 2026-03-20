use std::collections::{HashMap, VecDeque};
use crate::elevator::elevio::poll::CallType;
use super::types::Order;

pub struct OrderList {
    queue: VecDeque<Order>,
    current_orders: HashMap<usize, Order>,
}

impl OrderList {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::with_capacity(9),
            current_orders: HashMap::new(),
        }
    }

    pub fn enqueue(&mut self, order: Order) {
        if !self.contains(&order) {
            self.queue.push_back(order);
        }
    }

    pub fn contains(&self, order: &Order) -> bool {
        let all = self.queue.iter().chain(self.current_orders.values());
        if order.cb.call == CallType::Cab {
            all.into_iter().any(|o| o == order)
        } else {
            all.into_iter().any(|o| o.cb == order.cb)
        }
    }

    pub fn dequeue(&mut self, order: &Order) {
        if order.cb.call == CallType::Cab {
            self.queue.retain(|o| o != order);
        } else {
            self.queue.retain(|o| o.cb != order.cb);
        }
    }

    pub fn push_front(&mut self, order: Order) {
        self.queue.push_front(order);
    }

    pub fn push_back(&mut self, order: Order) {
        self.queue.push_back(order);
    }

    pub fn retain(&mut self, f: impl FnMut(&Order) -> bool) {
        self.queue.retain(f);
    }

    pub fn set_queue(&mut self, queue: VecDeque<Order>) {
        self.queue = queue;
    }

    pub fn queue(&self) -> &VecDeque<Order> {
        &self.queue
    }

    pub fn assign(&mut self, elev_idx: usize, order: Order) {
        self.current_orders.insert(elev_idx, order);
    }

    pub fn unassign(&mut self, elev_idx: usize) -> Option<Order> {
        self.current_orders.remove(&elev_idx)
    }

    pub fn current_order(&self, elev_idx: usize) -> Option<&Order> {
        self.current_orders.get(&elev_idx)
    }

    pub fn has_assignment(&self, elev_idx: usize) -> bool {
        self.current_orders.contains_key(&elev_idx)
    }

    pub fn all_orders(&self) -> Vec<Order> {
        self.queue.iter().cloned()
            .chain(self.current_orders.values().cloned())
            .collect()
    }
}
