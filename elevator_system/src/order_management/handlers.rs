use colored::Colorize;

use crate::{
    elevator::elevio::poll::{CallButton, CallType}, networking::types::Msg, types::{ElevatorCommand, ElevatorState, Position}
};

use super::ManagerState;
use super::types::{Event, Order, Role};

impl ManagerState {
    pub fn handle_event(&mut self, event: Event) {
        match event {
            Event::RequestOrder { order }               => self.on_request_order(order),
            Event::AckReceived(msg)                     => self.on_ack_received(msg),
            Event::QueueOrders { orders }               => self.on_queue_orders(orders),
            Event::AssignOrders { orders }              => self.on_assign_orders(orders),
            Event::CompleteOrder { order }              => self.on_complete_order(order),
            Event::ClearOrders { orders }               => self.on_clear_orders(orders),
            Event::StateUpdate { states }               => self.on_state_update(states),
            Event::StateUpdateAndShare { states }       => self.on_state_update_and_share(states),
            Event::OrderTimeout { elev_idx }            => self.on_order_timeout(elev_idx),
            Event::IdleTimeout { elev_idx }             => self.on_idle_timeout(elev_idx),
            Event::AlivesUpdate { alive_elevs }         => self.on_alives_update(alive_elevs),
        }
    }

    // Local buttonpress
    fn on_request_order(&self, order: Order) {
        let _ = self.network_tx.send(Msg::RequestOrder { order });
    }

    // Master received acks from all on order request
    pub(super) fn on_ack_received(&mut self, msg: Msg) {
        if let Msg::RequestOrder { order } = msg
            && self.cluster.is_master()
        {
            let _ = self.network_tx.send(Msg::QueueOrders { orders: vec![order.clone()] });
            if order.is_for(self.local_idx) {
                let _ = self.elev_cmd_tx.send(ElevatorCommand::SetLight(order.cb.clone(), true));
            }

            if let Some(order_elev_idx) = self.try_assign_new_order(order.clone()) {
                self.send_order(order.clone(), Some(Order { cb: order.cb.clone(), elev_idx: order_elev_idx }));
            }
        }
    }

    // Slaves should queue the order and turn on the light
    fn on_queue_orders(&mut self, orders: Vec<Order>) {
        if self.cluster.is_master() {
            let _ = self.network_tx.send(Msg::QueueOrders { orders: orders.clone() });
        }
        for order in orders {
            self.order_list.enqueue(order.clone());
            if order.is_for(self.local_idx) {
                let _ = self.elev_cmd_tx.send(ElevatorCommand::SetLight(order.cb, true));
            }
        }
    }

    // Slave should assign the order to the elevator
    fn on_assign_orders(&self, orders: Vec<Order>) {
        for order in orders {
            if order.elev_idx == self.local_idx {
                println!("Received order: {} assigned to me", order);
                let _ = self.elev_cmd_tx.send(ElevatorCommand::AssignOrder(order.cb.clone()));
            }
        }
    }

    // Master received a complete order message, assigns next order for the elevator
    fn on_complete_order(&mut self, order: Order) {
        match self.cluster.role() {
            Role::Slave => { let _ = self.network_tx.send(Msg::CompleteOrder { order }); }
            Role::Master => {
                if let Some(current) = self.order_list.current_order(order.elev_idx).cloned() {
                    if current == order {
                        self.order_list.unassign(order.elev_idx);
                        println!("{}", format!("Elev {} completed order: {}", order.elev_idx, order).blue().bold());
                        self.idle_wd.reset(order.elev_idx);
                    } else {
                        self.order_list.push_back(current);
                    }
                }
                self.order_list.retain(|o| o != &order);

                let (next_order, clear_orders) = self.try_assign_next_order(order.clone());
                self.send_order(order.clone(), next_order);
                if !clear_orders.is_empty() {
                    self.clear_these_orders(clear_orders.clone().into_iter().collect());
                    let _ = self.network_tx.send(Msg::ClearOrders { orders: clear_orders.into_iter().collect() });
                }
            }
        }
    }

    // Slave should clear the orders
    fn on_clear_orders(&mut self, orders: Vec<Order>) {
        self.clear_these_orders(orders);
    }

    // Got a state update from another elevator
    fn on_state_update(&mut self, states: Vec<ElevatorState>) {
        for new_state in states {
            self.positions.insert(new_state.id as usize, Position { floor: new_state.floor, obstruction: new_state.obstruction });
        }
    }

    // Need to inform other elevators about the new state
    fn on_state_update_and_share(&mut self, states: Vec<ElevatorState>) {
        for new_state in &states {
            self.positions.insert(new_state.id as usize, Position { floor: new_state.floor, obstruction: new_state.obstruction });
        }
        if self.cluster.is_network_ready() {
            let _ = self.network_tx.send(Msg::StateUpdate { states: states.clone() });
        }
        if self.cluster.is_master() {
            for elev_idx in states.iter().map(|s| s.id as usize) {
                if !self.order_list.has_assignment(elev_idx) {
                    let Position { floor, .. } = *self.positions.get(&elev_idx).unwrap();
                    let pseudo_cb = CallButton { floor, call: CallType::Cab };
                    self.want_order(Order { cb: pseudo_cb, elev_idx });
                }
            }
        }
    }

    fn on_order_timeout(&mut self, elev_idx: usize) {
        if self.cluster.is_master()
            && let Some(order) = self.order_list.unassign(elev_idx)
        {
            println!("{}", format!("Order timed out, queued: {}", order).yellow().bold());
            self.order_wd.remove(elev_idx);
            self.on_ack_received(Msg::RequestOrder { order: order.clone() });
            self.want_order(order);
            self.idle_wd.reset(elev_idx);
        }
    }

    fn on_idle_timeout(&mut self, elev_idx: usize) {
        if self.cluster.is_master() {
            if let Some(&Position { floor, .. }) = self.positions.get(&elev_idx) {
                if !self.order_list.has_assignment(elev_idx) {
                    let order = Order { cb: CallButton { floor, call: CallType::Cab }, elev_idx };
                    self.want_order(order);
                    self.idle_wd.reset(elev_idx);
                } else {
                    self.idle_wd.remove(elev_idx);
                }
            } else {
                self.idle_wd.reset(elev_idx);
            }
        }
    }

    fn on_alives_update(&mut self, alive_elevs: Vec<u8>) {
        let change = self.cluster.update_membership(&alive_elevs, self.local_id);
        self.handle_membership_change(change);
    }
}
