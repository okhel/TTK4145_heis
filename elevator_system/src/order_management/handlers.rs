use std::collections::HashSet;
use colored::Colorize;

use crate::{
    elevator::elevio::poll::{CallButton, CallType},
    networking::types::{ElevatorState, Msg},
};

use super::ManagerState;
use super::types::{Event, Order, Role};
use super::assignment::{assign_new_order, assign_next_order, is_mine};

impl ManagerState {
    pub fn handle_event(&mut self, event: Event) {
        match event {
            Event::RequestOrder { order }               => self.on_request_order(order),
            Event::AckReceived(msg)                     => self.on_ack_received(msg),
            Event::QueueOrders { orders }               => self.on_queue_orders(orders),
            Event::AssignOrders { orders }              => self.on_assign_orders(orders),
            Event::WantOrder { completed_order }        => self.on_want_order(completed_order),
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
    fn on_ack_received(&mut self, msg: Msg) {
        if let Msg::RequestOrder { order } = msg {
            if self.role == Role::Master {
                let _ = self.network_tx.send(Msg::QueueOrders { orders: vec![order.clone()] });
                self.pending_acks.remove(&order);
                if is_mine(&order, self.local_idx) {
                    let _ = self.call_light_tx.send((order.cb.clone(), true));
                }

                if let Some(order_elev_idx) = self.try_assign_new_order(order.clone()) {
                    self.send_order(order.clone(), Some(Order { cb: order.cb.clone(), elev_idx: order_elev_idx }));
                }
                // if try_assign_new_order returns None, the order is already
                // in self.orders from rebuild_queue inside assign_new_order
            }
        }
    }

    // Slaves should queue the order and turn on the light
    fn on_queue_orders(&mut self, orders: Vec<Order>) {
        if self.role == Role::Master {
            let _ = self.network_tx.send(Msg::QueueOrders { orders: orders.clone() });
        }
        for order in orders {
            if !self.orders.contains(&order) {
                self.orders.push_back(order.clone());
            }
            if is_mine(&order, self.local_idx) {
                let _ = self.call_light_tx.send((order.cb, true));
            }
        }
    }

    // Slave should assign the order to the elevator
    fn on_assign_orders(&self, orders: Vec<Order>) {
        for order in orders {
            if order.elev_idx == self.local_idx {
                println!("Received order: {} assigned to me", order);
                let _ = self.call_assign_tx.send(order.cb.clone());
            }
        }
    }

    // Local message in master
    fn on_want_order(&mut self, completed_order: Order) {
        if self.role == Role::Master && self.current_orders.get(&completed_order.elev_idx).is_none(){
            let pseudo_order = Order { cb: CallButton { floor: completed_order.cb.floor, call: CallType::Cab }, elev_idx: completed_order.elev_idx };
            let (next_order, _) = self.try_assign_next_order(pseudo_order.clone());
            self.send_order(completed_order.clone(), next_order);
        }
    }

    // Master received a complete order message, assigns next order for the elevator
    fn on_complete_order(&mut self, order: Order) {
        match self.role {
            Role::Slave => { let _ = self.network_tx.send(Msg::CompleteOrder { order }); }
            Role::Master => {
                if let Some(current_order) = self.current_orders.get(&order.elev_idx) {
                    if *current_order == Some(order.clone()) {
                        self.current_orders.remove(&order.elev_idx);
                        println!("{}", format!("Elev {} completed order: {}", order.elev_idx, order).blue().bold());
                        let _ = self.idle_reset_tx.send(order.elev_idx);
                    } else {
                        if let Some(order_to_queue) = current_order.clone() {
                            self.orders.push_back(order_to_queue);
                        }
                    }
                }
                self.orders.retain(|o| o != &order);

                let (next_order, clear_orders) = self.try_assign_next_order(order.clone());
                self.send_order(order.clone(), next_order);
                if clear_orders.len() > 0 {
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
            self.positions.insert(new_state.id as usize, new_state.floor);
        }
    }

    // Need to inform other elevators about the new state
    fn on_state_update_and_share(&mut self, states: Vec<ElevatorState>) {
        for new_state in &states {
            self.positions.insert(new_state.id as usize, new_state.floor);
        }
        if self.network_ready {
            let _ = self.network_tx.send(Msg::StateUpdate { states: states.clone() });
        }
        if self.role == Role::Master {
            for elev_idx in states.iter().map(|s| s.id as usize) {
                if self.current_orders.get(&elev_idx).is_none() {
                    let floor = *self.positions.get(&elev_idx).unwrap();
                    let pseudo_cb = CallButton { floor, call: CallType::Cab };
                    let _ = self.want_order_tx.send(Order { cb: pseudo_cb, elev_idx });
                }
            }
        }
    }

    fn on_order_timeout(&mut self, elev_idx: usize) {
        if self.role == Role::Master {
            if let Some(Some(order)) = self.current_orders.remove(&elev_idx) {
                println!("{}", format!("Order timed out, queued: {}", order).yellow().bold());
                let _ = self.wd_remove_tx.send(elev_idx);
                let _ = self.ack_complete_tx.send((0, Msg::RequestOrder { order: order.clone() }));
                let _ = self.want_order_tx.send(order);
                let _ = self.idle_reset_tx.send(elev_idx);
            }
        }
    }

    fn on_idle_timeout(&mut self, elev_idx: usize) {
        if self.role == Role::Master {
            if let Some(&floor) = self.positions.get(&elev_idx) {
                if self.current_orders.get(&elev_idx).is_none() {
                    println!("{}", format!("Elev {} idle for 5 seconds, requesting work", elev_idx).yellow());
                    let order = Order { cb: CallButton { floor, call: CallType::Cab }, elev_idx };
                    let _ = self.want_order_tx.send(order);
                    let _ = self.idle_reset_tx.send(elev_idx);
                } else {
                    let _ = self.idle_remove_tx.send(elev_idx);
                }
            } else {
                let _ = self.idle_reset_tx.send(elev_idx);
            }
        }
    }

    fn on_alives_update(&mut self, alive_elevs: Vec<u8>) {
        let new_set: HashSet<usize> = alive_elevs.iter().map(|id| *id as usize).collect();
        let old_set = self.alive_elevs.clone();
        let newly_alive: Vec<usize> = new_set.difference(&old_set).copied().collect();
        let lost: Vec<usize> = old_set.difference(&new_set).copied().collect();

        self.alive_elevs = new_set.clone();
        self.network_ready = true;

        let mut became_master = false;

        if alive_elevs.iter().min() == Some(&self.local_id) {
            self.role = Role::Master;
            became_master = old_set.is_empty() || old_set.iter().min() != new_set.iter().min();
        } else {
            self.role = Role::Slave;
        }

        if self.role == Role::Master {
            if became_master {
                // Kickstart all elevators when becoming master
                for elev_idx in new_set.iter().copied() {
                    self.kickstart_idle_elevator(elev_idx);
                }
            } else {
                // Kickstart only newly alive elevators
                for &elev_idx in &newly_alive {
                    self.kickstart_idle_elevator(elev_idx);
                }
            }

            // Remove watchdog for disappeared elevators and re-queue orders
            for elev_idx in &lost {
                println!("{}", format!("Elev {} lost, re-queuing orders", elev_idx).red().bold());
                let _ = self.wd_remove_tx.send(*elev_idx);
                let _ = self.idle_remove_tx.send(*elev_idx);
                self.positions.remove(elev_idx);
                if let Some(Some(order)) = self.current_orders.remove(elev_idx) {
                    println!("{}", format!("Re-queuing order: {}", order).yellow().bold());
                    let _ = self.ack_complete_tx.send((0, Msg::RequestOrder { order }));
                }
            }
        }

        // Sync orders and state with newly alive elevators
        if !newly_alive.is_empty() {
            let orders_to_send: Vec<Order> = self.orders.iter().cloned().chain(self.current_orders.values().filter_map(|o| o.clone())).collect();
            let states_to_send: Vec<ElevatorState> = self.positions.iter().map(|(id, floor)| ElevatorState { id: *id as u8, floor: *floor }).collect();
            let _ = self.network_tx.send(Msg::QueueOrders { orders: orders_to_send });
            let _ = self.network_tx.send(Msg::StateUpdate { states: states_to_send });
        }
    }

    // --- Helper methods ---

    fn send_order(&mut self, completed_order: Order, next_order: Option<Order>) {
        let _ = self.wd_reset_tx.send(completed_order.elev_idx);
        if let Some(next) = next_order {
            self.update_current_orders(next.clone(), next.elev_idx);
            let _ = self.idle_remove_tx.send(next.elev_idx);
            if next.elev_idx == self.local_idx {
                let _ = self.call_assign_tx.send(next.cb.clone());
            } else {
                let _ = self.network_tx.send(Msg::AssignOrders { orders: vec![next.clone()] });
            }
            println!("{}", format!("\nAssigned next order: {}", next).green().bold());
        }
    }

    fn kickstart_idle_elevator(&self, elev_idx: usize) {
        if self.current_orders.get(&elev_idx).is_some() {
            println!("Elev {} already has work, resetting watchdog", elev_idx);
            let _ = self.wd_reset_tx.send(elev_idx);
            return;
        }
        if let Some(&floor) = self.positions.get(&elev_idx) {
            println!("Kicking elev {} with work request", elev_idx);
            let pseudo_cb = CallButton { floor, call: CallType::Cab };
            let _ = self.want_order_tx.send(Order { cb: pseudo_cb, elev_idx });
            let _ = self.idle_reset_tx.send(elev_idx);
        } else {
            println!("No floor reading for elev {}, waiting for state update to kickstart", elev_idx);
            let _ = self.idle_reset_tx.send(elev_idx);
        }
    }

    fn update_current_orders(&mut self, order: Order, elev_idx: usize) {
        self.current_orders.insert(elev_idx, Some(order.clone()));
        let _ = self.wd_reset_tx.send(elev_idx);
        let _ = self.idle_remove_tx.send(elev_idx);
    }

    fn clear_these_orders(&mut self, completed_orders: Vec<Order>) {
        for order in completed_orders {
            if order.cb.call == CallType::Cab {
                self.orders.retain(|o: &Order| o != &order);
            } else {
                self.orders.retain(|o| o.cb != order.cb);
            }
            if is_mine(&order, self.local_idx) {
                let _ = self.call_light_tx.send((order.cb, false));
            }
        }
    }

    fn try_assign_next_order(&mut self, order: Order) -> (Option<Order>, HashSet<Order>) {
        let result = assign_next_order(order.clone(), &mut self.orders, &mut self.current_orders);
        let clear_orders: HashSet<Order> = [
            order.clone(),
            Order { cb: CallButton { floor: order.cb.floor, call: CallType::Cab }, elev_idx: order.elev_idx },
        ].into_iter().chain(result.clear).collect();

        (result.next, clear_orders)
    }

    fn try_assign_new_order(&mut self, order: Order) -> Option<usize> {
        assign_new_order(
            order.clone(),
            &mut self.orders,
            &mut self.positions,
            &mut self.current_orders,
            &self.alive_elevs.iter().copied().collect::<Vec<_>>(),
        )
    }
}
