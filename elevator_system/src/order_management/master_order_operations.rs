use std::collections::HashSet;
use colored::Colorize;

use crate::{
    elevator::elevio::poll::{CallButton, CallType}, networking::types::Msg, types::{ElevatorCommand, ElevatorState, Position}
};

use super::ManagerState;
use super::types::Order;
use super::membership::MembershipChange;
use super::assignment::{assign_new_order, assign_next_order};

impl ManagerState {
    // Request next order for an idle elevator
    pub(super) fn want_order(&mut self, completed_order: Order) {
        if self.membership.is_master() && !self.order_list.has_assignment(completed_order.elev_idx) {
            let pseudo_order = Order { cb: CallButton { floor: completed_order.cb.floor, call: CallType::Cab }, elev_idx: completed_order.elev_idx };
            let (next_order, _) = self.try_assign_next_order(pseudo_order.clone());
            self.send_order(completed_order.clone(), next_order);
        }
    }

    pub(super) fn send_order(&mut self, completed_order: Order, next_order: Option<Order>) {
        self.order_wd.reset(completed_order.elev_idx);
        if let Some(next) = next_order {
            self.order_list.assign(next.elev_idx, next.clone());
            self.order_wd.reset(next.elev_idx);
            self.idle_wd.remove(next.elev_idx);
            if next.elev_idx == self.local_idx {
                let _ = self.elev_cmd_tx.send(ElevatorCommand::AssignOrder(next.cb.clone()));
            } else {
                let _ = self.network_tx.send(Msg::AssignOrders { orders: vec![next.clone()] });
            }
            println!("{}", format!("\nAssigned next order: {}", next).green().bold());
        }
    }

    pub(super) fn kickstart_idle_elevator(&mut self, elev_idx: usize) {
        if self.order_list.has_assignment(elev_idx) {
            println!("Elev {} already has work, resetting watchdog", elev_idx);
            self.order_wd.reset(elev_idx);
            return;
        }
        if let Some(&Position { floor, .. }) = self.positions.get(&elev_idx) {
            println!("Kicking elev {} with work request", elev_idx);
            let pseudo_cb = CallButton { floor, call: CallType::Cab };
            self.want_order(Order { cb: pseudo_cb, elev_idx });
            self.idle_wd.reset(elev_idx);
        } else {
            println!("No floor reading for elev {}, waiting for state update to kickstart", elev_idx);
            self.idle_wd.reset(elev_idx);
        }
    }

    pub(super) fn clear_these_orders(&mut self, completed_orders: Vec<Order>) {
        for order in completed_orders {
            self.order_list.dequeue(&order);
            if order.is_for(self.local_idx) {
                let _ = self.elev_cmd_tx.send(ElevatorCommand::SetLight(order.cb, false));
            }
        }
    }

    pub(super) fn try_assign_next_order(&mut self, order: Order) -> (Option<Order>, HashSet<Order>) {
        let result = assign_next_order(order.clone(), &mut self.order_list);
        let clear_orders: HashSet<Order> = [
            order.clone(),
            Order { cb: CallButton { floor: order.cb.floor, call: CallType::Cab }, elev_idx: order.elev_idx },
        ].into_iter().chain(result.clear).collect();

        (result.next, clear_orders)
    }

    pub(super) fn try_assign_new_order(&mut self, order: Order) -> Option<usize> {
        assign_new_order(
            order,
            &mut self.order_list,
            &self.positions,
            &self.membership.alive_vec(),
        )
    }

    pub(super) fn handle_membership_change(&mut self, change: MembershipChange) {
        if self.membership.is_master() {
            if change.became_master {
                let alive: Vec<usize> = self.membership.alive().iter().copied().collect();
                for elev_idx in alive {
                    self.kickstart_idle_elevator(elev_idx);
                }
            } else {
                for &elev_idx in &change.newly_alive {
                    self.kickstart_idle_elevator(elev_idx);
                }
            }

            for elev_idx in &change.lost {
                println!("{}", format!("Elev {} lost, re-queuing orders", elev_idx).red().bold());
                self.order_wd.remove(*elev_idx);
                self.idle_wd.remove(*elev_idx);
                self.positions.remove(elev_idx);
                if let Some(order) = self.order_list.unassign(*elev_idx) {
                    println!("{}", format!("Re-queuing order: {}", order).yellow().bold());
                    self.on_ack_received(Msg::RequestOrder { order });
                }
            }
        }

        if !change.newly_alive.is_empty() {
            let orders_to_send = self.order_list.all_orders();
            let states_to_send: Vec<ElevatorState> = self.positions.iter()
                .map(|(id, pos)| ElevatorState { id: *id as u8, floor: pos.floor, obstruction: pos.obstruction })
                .collect();
            let _ = self.network_tx.send(Msg::QueueOrders { orders: orders_to_send });
            let _ = self.network_tx.send(Msg::StateUpdate { states: states_to_send });
        }
    }
}
