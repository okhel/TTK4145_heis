use std::collections::{HashMap, HashSet, VecDeque};
use tokio::sync::mpsc::{
    UnboundedReceiver as URx, UnboundedSender as UTx,
};
use tokio::sync::broadcast::{Receiver as BcRx};

use crate::elevator::elevio::poll::{CallButton, CallType};
use crate::networking::types::{ElevatorState, Msg};
use types::{Event, Order, Role};
use colored::Colorize;

pub mod types;
pub mod utils;
use utils::{assign_new_orders, assign_next_order};


fn msg_to_event(msg: Msg, role: &Role) -> Option<Event> {
    match msg {
        Msg::RequestOrder { order }                  => if role == &Role::Master { Some(Event::RequestOrder { order }) } else { None },
        Msg::QueueOrders { orders }             => Some(Event::QueueOrders { orders }),
        Msg::AssignOrder { order }                   => Some(Event::AssignOrder { order }),
        Msg::CompleteOrder { order }                 => Some(Event::CompleteOrder { order }),
        Msg::ClearOrders { orders }             => Some(Event::ClearOrders { orders }),
        Msg::StateUpdate { states }     => match role {
            Role::Master => Some(Event::StateShare { states }),
            Role::Slave => Some(Event::StateUpdate { states }),
        },
        Msg::Heartbeat                   => None,
    }
}

// TODO: add some master logic to only let master assign orders, maybe add a Role in the ManagerState
struct ManagerState {
    orders: VecDeque<Order>,
    positions: HashMap<usize, u8>,
    current_orders: HashMap<usize, Option<Order>>,
    alive_elevs: HashSet<usize>,
    pending_acks: HashMap<Order, Order>,
    role: Role,
    network_ready: bool,
}

impl ManagerState {
    fn new() -> Self {
        Self {
            orders: VecDeque::with_capacity(9),
            positions: HashMap::new(),
            current_orders: HashMap::new(),
            alive_elevs: HashSet::new(),
            pending_acks: HashMap::new(),
            role: Role::Slave,
            network_ready: false,
        }
    }
}

pub async fn order_manager(
    local_id: u8,
    mut call_request_rx: URx<CallButton>,
    call_assign_tx: UTx<CallButton>,
    mut update_floor_rx: URx<u8>,
    mut call_complete_rx: URx<CallButton>,
    call_light_tx: UTx<(CallButton, bool)>,
    network_tx: UTx<Msg>,
    mut network_rx: URx<Msg>,
    mut ack_complete_rx: URx<(u32, Msg)>,
    mut mgmt_elevs_alive_rx: BcRx<Vec<u8>>,
) {
    let local_idx = local_id as usize;
    let mut state = ManagerState::new();

    loop {
        // if state.orders.len() == 0 { println!("Completed all orders"); }
        // println!("\n-\nOrders: {:?}\nCurrent orders: {:?}", state.orders, state.current_orders);

        let event = tokio::select! {
            Some(cb)            = call_request_rx.recv() => Event::RequestOrder { order: Order { cb: cb, elev_idx: local_idx } },
            Some(floor)         = update_floor_rx.recv()  => Event::StateShare { states: vec![ElevatorState {id: local_id, floor}] },
            Some(cb)            = call_complete_rx.recv()  => Event::CompleteOrder { order: Order { cb: cb, elev_idx: local_id as usize } },
            Ok(alive_elevs)     = mgmt_elevs_alive_rx.recv()  => Event::AlivesUpdate { alive_elevs },
            Some(msg)           = network_rx.recv()        => match msg_to_event(msg, &state.role) {
                Some(e) => e,
                None    => continue,
            },
            Some((_seq, msg)) = ack_complete_rx.recv() => Event::AckReceived(msg),
            else => panic!("All channels closed"),
        };

        handle_event(local_id, local_idx, event, &mut state, &call_assign_tx, &call_light_tx, &network_tx);
    }
}

fn handle_event(
    local_id: u8,
    local_idx: usize,
    event: Event,
    state: &mut ManagerState,
    call_assign_tx: &UTx<CallButton>,
    call_light_tx: &UTx<(CallButton, bool)>,
    network_tx: &UTx<Msg>,
) {
    match event {
        // local buttonpress
       Event::RequestOrder { order } => {
            let _ = network_tx.send(Msg::RequestOrder { order });
        }

        // Master received acks from all on order request
        Event::AckReceived(msg) => {
            if let Msg::RequestOrder { order } = msg && state.role == Role::Master {
                state.pending_acks.remove(&order);
                if is_mine(order.clone(), local_idx) { let _ = call_light_tx.send((order.clone().cb, true)); }
                if let Some(order_elev_idx) = try_assign_new_order(order.clone(), state) {
                    state.current_orders.insert(order_elev_idx, Some(order.clone()));
                    match order_elev_idx == local_idx {
                        true => { let _ = call_assign_tx.send(order.cb.clone()); }
                        false => { let _ = network_tx.send(Msg::AssignOrder { order: Order { cb: order.cb.clone(), elev_idx: order_elev_idx } }); }
                    }
                }
                else {
                    state.orders.push_back(order.clone());
                }
                let _ = network_tx.send(Msg::QueueOrders { orders: vec![order.clone()] });
            }
        }

        // Slaves queue the orders they receive
        Event::QueueOrders { orders } => {
            for order in orders {
                if is_mine(order.clone(), local_idx) { let _ = call_light_tx.send((order.clone().cb, true)); }
                state.orders.push_back(order);
            }
        }

        Event::AssignOrder { order } => {
            state.current_orders.insert(order.elev_idx, Some(order.clone()));
            if order.elev_idx == local_idx {
                println!("Elev {:?} completing order: {:?}", local_idx, order);
                let _ = call_assign_tx.send(order.cb.clone());
            }
        }

        // Master received a complete order message, assigns next order for the elevator
        Event::CompleteOrder { order } => {
            match state.role {
                Role::Slave => { let _ = network_tx.send(Msg::CompleteOrder { order }); }
                Role::Master => {
                    // NB: Does not clear other orders than "order"
                    state.current_orders.remove(&order.elev_idx);
                    // println!("\n -\n Orders: {:?}\n Current orders: {:?}\n", state.orders, state.current_orders);
                    println!("{}", format!("Elev {:?} completed order: {:?}", order.clone().elev_idx, order.clone()).blue().bold());
                    println!("{}", format!("Current orders: {:?}", state.current_orders));
                    let (next_order, clear_orders) = try_assign_next_order(order.clone(), state);
                    if next_order.is_some() {
                        state.current_orders.insert(next_order.clone().unwrap().elev_idx, next_order.clone());
                        match next_order.clone().unwrap().elev_idx == local_idx {
                            true => { let _ = call_assign_tx.send(next_order.as_ref().unwrap().cb.clone()); }
                            false => { let _ = network_tx.send(Msg::AssignOrder { order: Order { cb: next_order.as_ref().unwrap().cb.clone(), elev_idx: order.elev_idx } }); }
                        }
                        println!("{}", format!("Found new order for elevator {:?}: {:?}\n ", next_order.clone().unwrap().elev_idx, next_order.clone().unwrap()).green().bold());
                    }
                    else { println!("{}", format!("Could not assign next order after {:?}", order.clone()).red().bold()); }
                    if clear_orders.len() > 0 {
                        clear_these_orders(clear_orders.clone().into_iter().collect(), state, local_idx, call_light_tx);
                        let _ = network_tx.send(Msg::ClearOrders { orders: clear_orders.into_iter().collect() });
                    }
                }
            }
        }

        Event::ClearOrders { orders } => {
            clear_these_orders(orders, state, local_idx, call_light_tx);
        }

        // got a state update from an elevator
        Event::StateUpdate { states } => {
            for new_state in states {
                state.positions.insert(new_state.id as usize, new_state.floor);
                state.alive_elevs.insert(new_state.id as usize);
            }
        }

        Event::StateShare { states } => {
            for new_state in &states {
                state.positions.insert(new_state.id as usize, new_state.floor);
                state.alive_elevs.insert(new_state.id as usize);
            }
            if state.network_ready {
                let _ = network_tx.send(Msg::StateUpdate { states });
            }
        }

        Event::AlivesUpdate { alive_elevs } => {
            match alive_elevs.iter().min() == Some(&local_id) {
                true => { state.role = Role::Master; }
                false => { state.role = Role::Slave; }
            }
            state.network_ready = true;
            if let Some(&floor) = state.positions.get(&local_idx) {
                let _ = network_tx.send(Msg::StateUpdate {
                    states: vec![ElevatorState { id: local_id, floor }],
                });
            }
            println!("I'm {:?}", &state.role);
            // TODO: If an elevator dies
        }
    }
}


fn try_assign_next_order(order: Order, state: &mut ManagerState) -> (Option<Order>, HashSet<Order>) {
    let result = assign_next_order(order.clone(), &mut state.orders, &mut state.current_orders);
    let clear_orders: HashSet<Order> = [
        order.clone(),
        Order { cb: CallButton { floor: order.cb.floor, call: CallType::Cab }, elev_idx: order.elev_idx },
        ].into_iter().chain(result.clear).collect();

    return (result.next, clear_orders);
}

fn clear_these_orders(completed_orders: Vec<Order>, state: &mut ManagerState, local_idx: usize, call_light_tx: &UTx<(CallButton, bool)>) {
    for order in completed_orders {
        state.orders.retain(|o| o != &order);
        if is_mine(order.clone(), local_idx) { let _ = call_light_tx.send((order.cb, false)); }
    }
}

fn is_mine(order: Order, idx: usize) -> bool {
    if (order.elev_idx == idx) || order.cb.call != CallType::Cab {
        return true;
    }
    false
}

fn try_assign_new_order(order: Order, state: &mut ManagerState) -> Option<usize> {
    let new_order_found = assign_new_orders(
        order.clone(),
        &mut state.orders,
        &mut state.positions,
        &mut state.current_orders,
        &state.alive_elevs.iter().copied().collect::<Vec<_>>());

    if let Some(order_elev_idx) = new_order_found {
        return Some(order_elev_idx);
    }
    None
}


