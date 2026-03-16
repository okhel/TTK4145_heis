use std::collections::{HashMap, HashSet, VecDeque};
use colored::Colorize;
use tokio::sync::{
    mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx, unbounded_channel as uc},
    broadcast::Receiver as BcRx,
};
use tokio::time::Duration;

use crate::{
    elevator::elevio::poll::{CallButton, CallType},
    networking::types::{ElevatorState, Msg},
    watchdog::watchdog_timer,
};

use types::{Event, Order, Role};
pub mod types;
pub mod utils;
use utils::{assign_new_order, assign_next_order, is_mine};


fn msg_to_event(msg: Msg, role: &Role) -> Option<Event> {
    match msg {
        Msg::RequestOrder { order }                  => if role == &Role::Master { Some(Event::RequestOrder { order }) } else { None },
        Msg::QueueOrders { orders }             => Some(Event::QueueOrders { orders }),
        Msg::AssignOrders { orders }                   => Some(Event::AssignOrders { orders }),
        Msg::CompleteOrder { order }                 => Some(Event::CompleteOrder { order }),
        Msg::ClearOrders { orders }             => Some(Event::ClearOrders { orders }),
        Msg::StateUpdate { states }     => match role {
            Role::Master    => Some(Event::StateUpdateAndShare { states }),
            Role::Slave     => Some(Event::StateUpdate { states }),
        },
        Msg::Heartbeat                   => None,
    }
}
struct ManagerState {
    orders: VecDeque<Order>,
    positions: HashMap<usize, u8>,
    current_orders: HashMap<usize, Option<Order>>,
    alive_elevs: HashSet<usize>,
    pending_acks: HashMap<Order, Order>,
    role: Role,
    network_ready: bool,
    wd_reset_tx: UTx<usize>,
    wd_remove_tx: UTx<usize>,
    idle_reset_tx: UTx<usize>,
    idle_remove_tx: UTx<usize>,
}

impl ManagerState {
    fn new(wd_reset_tx: UTx<usize>, wd_remove_tx: UTx<usize>, idle_reset_tx: UTx<usize>, idle_remove_tx: UTx<usize>) -> Self {
        Self {
            orders: VecDeque::with_capacity(9),
            positions: HashMap::new(),
            current_orders: HashMap::new(),
            alive_elevs: HashSet::new(),
            pending_acks: HashMap::new(),
            role: Role::Slave,
            network_ready: false,
            wd_reset_tx,
            wd_remove_tx,
            idle_reset_tx,
            idle_remove_tx,
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
    ack_complete_tx: UTx<(u32, Msg)>,
    mut mgmt_elevs_alive_rx: BcRx<Vec<u8>>,
) {
    let local_idx = local_id as usize;

    let (wd_reset_tx, wd_reset_rx) = uc::<usize>();
    let (wd_remove_tx, wd_remove_rx) = uc::<usize>();
    let (wd_expired_tx, mut wd_expired_rx) = uc::<usize>();
    let (want_order_tx, mut want_order_rx) = uc::<Order>();
    let wd_remove_tx_clone = wd_remove_tx.clone();

    let (idle_reset_tx, idle_reset_rx) = uc::<usize>();
    let (idle_remove_tx, idle_remove_rx) = uc::<usize>();
    let (idle_expired_tx, mut idle_expired_rx) = uc::<usize>();

    tokio::spawn(watchdog_timer(
        Duration::from_secs(15),
        wd_reset_rx,
        wd_remove_rx,
        wd_expired_tx,
    ));

    tokio::spawn(watchdog_timer(
        Duration::from_secs(3),
        idle_reset_rx,
        idle_remove_rx,
        idle_expired_tx,
    ));

    let mut state = ManagerState::new(wd_reset_tx, wd_remove_tx, idle_reset_tx, idle_remove_tx);

    // Wait for initial floor reading, this signifies that the elevator is ready to receive orders
    let init_floor = update_floor_rx.recv().await.unwrap();
    state.positions.insert(local_idx, init_floor);
    println!("{}", format!("Elev {} ready at floor {}", local_idx, init_floor).green().bold());

    

    loop {
        let event = tokio::select! {

            // Local messages
            Some(cb)            = call_request_rx.recv()                    => Event::RequestOrder { order: Order { cb: cb, elev_idx: local_idx } },
            Some(floor)         = update_floor_rx.recv()                    => Event::StateUpdateAndShare { states: vec![ElevatorState {id: local_id, floor}] },
            Some(cb)            = call_complete_rx.recv()                   => Event::CompleteOrder { order: Order { cb: cb, elev_idx: local_id as usize } },
            Ok(alive_elevs)     = mgmt_elevs_alive_rx.recv()                => Event::AlivesUpdate { alive_elevs },
            Some(order)         = want_order_rx.recv()                      => Event::WantOrder { completed_order: order },
            Some((_seq, msg))   = ack_complete_rx.recv()                    => Event::AckReceived(msg),
            Some(elev_idx)      = wd_expired_rx.recv()                      => Event::OrderTimeout { elev_idx },
            Some(elev_idx)      = idle_expired_rx.recv()                    => Event::IdleTimeout { elev_idx },

            // Network messages
            Some(msg)           = network_rx.recv()                         => match msg_to_event(msg, &state.role) {
                Some(e) => e,
                None    => continue,
            },
            else => panic!("All channels closed"),
        };

        handle_event(local_id, local_idx, event, &mut state, &call_assign_tx, &call_light_tx, &network_tx, &ack_complete_tx, &want_order_tx, &wd_remove_tx_clone);
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
    ack_complete_tx: &UTx<(u32, Msg)>,
    want_order_tx: &UTx<Order>,
    wd_remove_tx: &UTx<usize>,
) {
    match event {

        // local buttonpress
       Event::RequestOrder { order } => {
            let _ = network_tx.send(Msg::RequestOrder { order });
        }

        // Master received acks from all on order request
        Event::AckReceived(msg) => {
            if let Msg::RequestOrder { order } = msg {
                if state.role == Role::Master {

                    // Tell slaves to queue the order
                    let _ = network_tx.send(Msg::QueueOrders { orders: vec![order.clone()] });
                    state.pending_acks.remove(&order);
                    if is_mine(&order, local_idx) { let _ = call_light_tx.send((order.cb.clone(), true)); }

                    if let Some(order_elev_idx) = try_assign_new_order(order.clone(), state) {
                        send_order(order.clone(), Some(Order { cb: order.cb.clone(), elev_idx: order_elev_idx }), state, local_idx, call_assign_tx, network_tx);
                    }
                    else {
                        state.orders.push_back(order.clone());
                    }
                }
            }
        }

        // Slaves should queue the order and turn on the light
        Event::QueueOrders { orders } => {
            if state.role == Role::Master {
                let _ = network_tx.send(Msg::QueueOrders { orders: orders.clone() });
            }
            for order in orders {
                if !state.orders.contains(&order) {
                    state.orders.push_back(order.clone());
                }
                if is_mine(&order, local_idx) {
                    let _ = call_light_tx.send((order.cb, true));
                }
            }
        }

        // Slave should assign the order to the elevator
        Event::AssignOrders { orders } => {
            for order in orders {
                if is_mine(&order, local_idx) {
                    println!("Received order: {} assigned to me", order);
                    let _ = call_assign_tx.send(order.cb.clone());
                }
            }
        }

        // Local message in master
        Event::WantOrder { completed_order } => {
            if state.role == Role::Master {
                    let (next_order, _) = try_assign_next_order(completed_order.clone(), state);
                    send_order(completed_order.clone(), next_order, state, local_idx, call_assign_tx, network_tx);
            }
        }
        
        // Master received a complete order message, assigns next order for the elevator
        Event::CompleteOrder { order } => {
            match state.role {
                Role::Slave => { let _ = network_tx.send(Msg::CompleteOrder { order }); }
                Role::Master => {
                    if let Some(current_order) = state.current_orders.get(&order.elev_idx) {
                        if *current_order == Some(order.clone()) {
                            state.current_orders.remove(&order.elev_idx);
                            println!("{}", format!("Elev {} completed order: {}", order.elev_idx, order).blue().bold());
                            let _ = state.idle_reset_tx.send(order.elev_idx);
                        }
                        else {
                            if let Some(order_to_queue) = current_order.clone() {
                                state.orders.push_back(order_to_queue);
                            }
                        }
                    }
                    state.orders.retain(|o| o != &order);

                    // println!("Orders: {:?}", state.orders);
                    // println!("Current orders: {:?}", state.current_orders);
                    let (next_order, clear_orders) = try_assign_next_order(order.clone(), state);
                    send_order(order.clone(), next_order, state, local_idx, call_assign_tx, network_tx);
                    if clear_orders.len() > 0 {
                        clear_these_orders(clear_orders.clone().into_iter().collect(), state, local_idx, call_light_tx);
                        let _ = network_tx.send(Msg::ClearOrders { orders: clear_orders.into_iter().collect() });
                    }
                }
            }
        }

        // Slave should clear the orders
        Event::ClearOrders { orders } => {
            clear_these_orders(orders, state, local_idx, call_light_tx);
        }

        // got a state update from another elevator
        Event::StateUpdate { states } => {
            for new_state in states {
                state.positions.insert(new_state.id as usize, new_state.floor);
            }
        }

        // need to inform other elevators about the new state, either because I am master or because I received an update from a slave and need to pass it on
        Event::StateUpdateAndShare { states } => {
            for new_state in &states {
                state.positions.insert(new_state.id as usize, new_state.floor);
            }
            if state.network_ready {
                let _ = network_tx.send(Msg::StateUpdate { states: states.clone() });
            }
            if state.role == Role::Master {
                for elev_idx in states.iter().map(|s| s.id as usize) {
                    if state.current_orders.get(&elev_idx).is_none() {
                        let floor = state.positions.get(&elev_idx).unwrap().clone();
                        let pseudo_cb = CallButton { floor, call: CallType::Cab };
                        let _ = want_order_tx.send(Order { cb: pseudo_cb, elev_idx });
                    }
                }
            }
        }

        Event::OrderTimeout { elev_idx } => {
            if state.role == Role::Master {
                if let Some(Some(order)) = state.current_orders.remove(&elev_idx) {
                    println!("{}", format!("Order timed out, queued: {}", order).yellow().bold());
                    let _ = wd_remove_tx.send(elev_idx);
                    let _ = ack_complete_tx.send((0, Msg::RequestOrder { order: order.clone() }));
                }
            }
        }

        Event::IdleTimeout { elev_idx } => {
            if state.role == Role::Master {
                if let Some(&floor) = state.positions.get(&elev_idx) {
                    if state.current_orders.get(&elev_idx).is_none() {
                        println!("{}", format!("Elev {} idle for 5 seconds, requesting work", elev_idx).yellow());
                        let order = Order { cb: CallButton { floor, call: CallType::Cab }, elev_idx };
                        let _ = want_order_tx.send(order);
                    }
                }
                let _ = state.idle_reset_tx.send(elev_idx);
            }
        }

        Event::AlivesUpdate { alive_elevs } => {
            let new_set: HashSet<usize> = alive_elevs.iter().map(|id| *id as usize).collect();
            let old_set = state.alive_elevs.clone();
            let newly_alive: Vec<usize> = new_set.difference(&old_set).copied().collect();
            let lost: Vec<usize> = old_set.difference(&new_set).copied().collect();

            state.alive_elevs = new_set.clone();
            state.network_ready = true;

            let mut became_master = false;

            if alive_elevs.iter().min() == Some(&local_id) {
                state.role = Role::Master;
                became_master = old_set.is_empty() || old_set.iter().min() != new_set.iter().min();
            } else {
                state.role = Role::Slave;
                
            }


            let pseudo_cb = CallButton { floor: state.positions.get(&local_idx).unwrap().clone(), call: CallType::Cab };
            if state.role == Role::Master {
                if became_master {
                    // println!("Change of master");
                    if state.current_orders.get(&local_idx).is_none() {
                        let _ = want_order_tx.send(Order { cb: pseudo_cb.clone(), elev_idx: local_idx });
                    }
                    else {
                        let _ = state.wd_reset_tx.send(local_idx);
                    }
                }

                // Remove watchdog for disappeared elevators and re-queue orders
                for elev_idx in &lost {
                    println!("{}", format!("Elev {} lost, re-queuing orders", elev_idx).red().bold());
                    let _ = state.wd_remove_tx.send(*elev_idx);
                    let _ = state.idle_remove_tx.send(*elev_idx);
                    state.positions.remove(elev_idx);
                    if let Some(Some(order)) = state.current_orders.remove(elev_idx) {
                        println!("{}", format!("Re-queuing order: {}", order).yellow().bold());
                        let _ = ack_complete_tx.send((0, Msg::RequestOrder { order }));
                    }
                }

                for elev_idx in &newly_alive {
                    if state.current_orders.get(elev_idx).is_none() {
                        let _ = state.idle_reset_tx.send(*elev_idx);
                    }
                }
            }

            // Sync orders and state with newly alive elevators
            if !newly_alive.is_empty() {
                let orders_to_send: Vec<Order> = state.orders.iter().cloned().chain(state.current_orders.values().filter_map(|o| o.clone())).collect();
                let states_to_send: Vec<ElevatorState> = state.positions.iter().map(|(id, floor)| ElevatorState { id: *id as u8, floor: *floor }).collect();
                let _ = network_tx.send(Msg::QueueOrders { orders: orders_to_send });
                let _ = network_tx.send(Msg::StateUpdate { states: states_to_send });
            }
        }
    }
}


fn send_order(completed_order: Order, next_order: Option<Order>, state: &mut ManagerState, local_idx: usize, call_assign_tx: &UTx<CallButton>, network_tx: &UTx<Msg>) {
    let _ = state.wd_remove_tx.send(completed_order.elev_idx);
    if let Some(next) = next_order {
        update_current_orders(state, next.clone(), next.elev_idx);
        let _ = state.idle_remove_tx.send(next.elev_idx);
        if next.elev_idx == local_idx {
            let _ = call_assign_tx.send(next.cb.clone());
        } else {
            let _ = network_tx.send(Msg::AssignOrders { orders: vec![Order { cb: next.cb.clone(), elev_idx: completed_order.elev_idx }] });
        }
        println!("{}", format!("\nAssigned next order: {}", next).green().bold());
    }
}

fn update_current_orders(state: &mut ManagerState, order: Order, elev_idx: usize) {
    state.current_orders.insert(elev_idx, Some(order.clone()));
    let _ = state.wd_reset_tx.send(elev_idx);
    let _ = state.idle_remove_tx.send(elev_idx);
}

fn clear_these_orders(completed_orders: Vec<Order>, state: &mut ManagerState, local_idx: usize, call_light_tx: &UTx<(CallButton, bool)>) {
    for order in completed_orders {
        if order.cb.call == CallType::Cab {
            state.orders.retain(|o: &Order| o != &order);
        }
        else {
            state.orders.retain(|o| o.cb != order.cb);
        }
        if is_mine(&order, local_idx) { let _ = call_light_tx.send((order.cb, false)); }
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

fn try_assign_new_order(order: Order, state: &mut ManagerState) -> Option<usize> {
    let new_order_found = assign_new_order(
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


