use std::collections::{HashMap, HashSet, VecDeque};
use colored::Colorize;
use tokio::sync::{
    mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx, unbounded_channel as uc},
    broadcast::Receiver as BcRx,
};
use tokio::time::{Duration, Instant};

use crate::{
    elevator::elevio::poll::{CallButton, CallType},
    networking::types::{ElevatorState, Msg},
    watchdog::watchdog_timer,
};

use types::{Event, Order, Role};
pub mod types;
pub mod utils;
use utils::{assign_new_orders, assign_next_order};


fn msg_to_event(msg: Msg, role: &Role) -> Option<Event> {
    match msg {
        Msg::RequestOrder { order }                  => if role == &Role::Master { Some(Event::RequestOrder { order }) } else { None },
        Msg::QueueOrders { orders }             => Some(Event::QueueOrders { orders }),
        Msg::AssignOrders { orders }                   => Some(Event::AssignOrders { orders }),
        Msg::CompleteOrder { order }                 => Some(Event::CompleteOrder { order }),
        Msg::ClearOrders { orders }             => Some(Event::ClearOrders { orders }),
        Msg::StateUpdate { states }     => match role {
            Role::Master => Some(Event::StateShare { states }),
            Role::Slave => Some(Event::StateUpdate { states }),
        },
        Msg::Heartbeat                   => None,
    }
}
struct ManagerState {
    orders: VecDeque<Order>,
    positions: HashMap<usize, u8>,
    current_orders: HashMap<usize, Option<Order>>,
    alive_elevs: HashSet<usize>,
    just_spawned: bool,
    pending_acks: HashMap<Order, Order>,
    role: Role,
    network_ready: bool,
    wd_reset_tx: UTx<usize>,
    wd_remove_tx: UTx<usize>,
}

impl ManagerState {
    fn new(wd_reset_tx: UTx<usize>, wd_remove_tx: UTx<usize>) -> Self {
        Self {
            orders: VecDeque::with_capacity(9),
            positions: HashMap::new(),
            current_orders: HashMap::new(),
            alive_elevs: HashSet::new(),
            just_spawned: true,
            pending_acks: HashMap::new(),
            role: Role::Slave,
            network_ready: false,
            wd_reset_tx,
            wd_remove_tx,
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

    tokio::spawn(watchdog_timer(
        Duration::from_secs(15),
        wd_reset_rx,
        wd_remove_rx,
        wd_expired_tx,
    ));

    let mut state = ManagerState::new(wd_reset_tx, wd_remove_tx);

    let init_floor = update_floor_rx.recv().await
        .expect("Failed to receive initial floor from elevator");
    state.positions.insert(local_idx, init_floor);
    println!("{}", format!("Elev {} ready at floor {}", local_idx, init_floor).green().bold());

    let idle_timeout = Duration::from_secs(60);
    let mut idle_deadline = Instant::now() + idle_timeout;

    loop {
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
            Some(elev_idx)      = wd_expired_rx.recv() => Event::OrderTimeout { elev_idx },
            _ = tokio::time::sleep_until(idle_deadline) => {
                idle_deadline = Instant::now() + idle_timeout;
                if let Some(&floor) = state.positions.get(&local_idx) {
                    println!("{}", "Idle for 60 seconds -> kickstarting with cab completion".yellow());
                    Event::CompleteOrder {
                        order: Order { cb: CallButton { floor, call: CallType::Cab }, elev_idx: local_idx },
                    }
                } else {
                    continue;
                }
            },
            else => panic!("All channels closed"),
        };

        handle_event(local_id, local_idx, event, &mut state, &call_assign_tx, &call_light_tx, &network_tx, &ack_complete_tx, &mut idle_deadline, idle_timeout);
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
    idle_deadline: &mut Instant,
    idle_timeout: Duration,
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
                    state.pending_acks.remove(&order);
                    if is_mine(order.clone(), local_idx) { let _ = call_light_tx.send((order.clone().cb, true)); }
                    if let Some(order_elev_idx) = try_assign_new_order(order.clone(), state) {
                        update_current_orders(state, order.clone(), order_elev_idx);
                        // state.current_orders.insert(order_elev_idx, Some(order.clone()));
                        match order_elev_idx == local_idx {
                            true => { let _ = call_assign_tx.send(order.cb.clone()); *idle_deadline = Instant::now() + idle_timeout; }
                            false => { let _ = network_tx.send(Msg::AssignOrders { orders: vec![Order { cb: order.cb.clone(), elev_idx: order_elev_idx }] }); }
                        }
                    }
                    else {
                        state.orders.push_back(order.clone());
                    }
                    let _ = network_tx.send(Msg::QueueOrders { orders: vec![order.clone()] });
                }
            }
        }

        Event::QueueOrders { orders } => {
            for order in orders {
                // println!("Queueing order: {}", order);
                if !state.orders.contains(&order) {
                    state.orders.push_back(order.clone());
                }
                if is_mine(order.clone(), local_idx) {
                    let _ = call_light_tx.send((order.cb, true));
                }
            }
        }

        Event::AssignOrders { orders } => {
            for order in orders {
                // state.current_orders.insert(order.elev_idx, Some(order.clone()));
                if order.elev_idx == local_idx {
                    println!("Elev {} assigned order: {}", local_idx, order);
                    let _ = call_assign_tx.send(order.cb.clone());
                    *idle_deadline = Instant::now() + idle_timeout;
                }
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
                    let _ = state.wd_remove_tx.send(order.elev_idx);
                    let (next_order, clear_orders) = try_assign_next_order(order.clone(), state);
                    if next_order.is_some() {
                        update_current_orders(state, next_order.clone().unwrap(), next_order.clone().unwrap().elev_idx);
                        // state.current_orders.insert(next_order.clone().unwrap().elev_idx, next_order.clone());
                        match next_order.clone().unwrap().elev_idx == local_idx {
                            true => { let _ = call_assign_tx.send(next_order.as_ref().unwrap().cb.clone()); *idle_deadline = Instant::now() + idle_timeout; }
                            false => { let _ = network_tx.send(Msg::AssignOrders { orders: vec![Order { cb: next_order.as_ref().unwrap().cb.clone(), elev_idx: order.elev_idx }] }); }
                        }
                        println!("{}", format!("\nAssigned next order: {}", next_order.as_ref().unwrap()).green().bold());
                    }
                    if clear_orders.len() > 0 {
                        clear_these_orders(clear_orders.clone().into_iter().collect(), state, local_idx, call_light_tx);
                        let _ = network_tx.send(Msg::ClearOrders { orders: clear_orders.into_iter().collect() });
                    }
                }
            }
        }

        Event::ClearOrders { orders } => {
            clear_these_orders(orders, state, local_idx, call_light_tx);
            // println!("Orders in queue: [{}]", state.orders.iter().map(|o| format!("{}", o)).collect::<Vec<_>>().join(", "));
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

        Event::OrderTimeout { elev_idx } => {
            if state.role == Role::Master {
                println!("{}", format!("Elev {} failed to complete in time", elev_idx).red().bold());
                if let Some(Some(order)) = state.current_orders.remove(&elev_idx) {
                    println!("{}", format!("Order timed out, queued: {}", order).yellow().bold());
                    let _ = ack_complete_tx.send((0, Msg::RequestOrder { order: order.clone() }));
                }
            }
        }

        Event::AlivesUpdate { alive_elevs } => {
            let new_set: HashSet<usize> = alive_elevs.iter().map(|id| *id as usize).collect();
            let old_set = state.alive_elevs.clone();
            let newly_alive: Vec<usize> = new_set.difference(&old_set).copied().collect();
            let lost: Vec<usize> = old_set.difference(&new_set).copied().collect();

            state.alive_elevs = new_set.clone();
            state.network_ready = true;

            let master_died = old_set.iter().min() < new_set.iter().min();
            match alive_elevs.iter().min() == Some(&local_id) {
                true => { state.role = Role::Master; }
                false => { state.role = Role::Slave; }
            }
            println!("I'm {:?}", &state.role);


            let pseudo_cb = CallButton { floor: state.positions.get(&local_idx).unwrap().clone(), call: CallType::Cab };
            if state.role == Role::Master {
                if master_died {
                    // Send pseudo cab order to new master
                    println!("Master died");

                    // Assign pseudo cab order to all elevators to kickstart them
                    for &elev_idx in state.alive_elevs.iter() {
                        if elev_idx == local_idx {
                            let _ = call_assign_tx.send(pseudo_cb.clone());
                            *idle_deadline = Instant::now() + idle_timeout;
                        }
                        if elev_idx != local_idx {
                            let _ = network_tx.send(Msg::AssignOrders { orders: vec![Order { cb: pseudo_cb.clone(), elev_idx }] });
                        }
                    }
                }

                // Remove watchdog for disappeared elevators and re-queue orders
                for elev_idx in &lost {
                    println!("{}", format!("Elev {} lost, re-queuing orders", elev_idx).red().bold());
                    let _ = state.wd_remove_tx.send(*elev_idx);
                    state.positions.remove(elev_idx);
                    if let Some(Some(order)) = state.current_orders.remove(elev_idx) {
                        println!("{}", format!("Re-queuing order: {}", order).yellow().bold());
                        let _ = ack_complete_tx.send((0, Msg::RequestOrder { order }));
                    }
                }
            }

            if state.just_spawned {
                state.just_spawned = false;
                let _ = network_tx.send(Msg::CompleteOrder { order: Order { cb: pseudo_cb, elev_idx: local_idx } });
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

fn update_current_orders(state: &mut ManagerState, order: Order, elev_idx: usize) {
    state.current_orders.insert(elev_idx, Some(order.clone()));
    let _ = state.wd_reset_tx.send(elev_idx);
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
        if order.cb.call == CallType::Cab {
            state.orders.retain(|o: &Order| o != &order);
        }
        else {
            state.orders.retain(|o| o.cb != order.cb);
        }
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


