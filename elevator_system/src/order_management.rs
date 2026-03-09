use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use tokio::sync::mpsc::{
    UnboundedReceiver as URx, UnboundedSender as UTx,
};

use crate::elevator::elevio::poll::{CallButton, CallType};
use crate::networking::types::{Direction, ElevatorState, Msg};



#[derive(Clone, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Order {
    pub cb: CallButton,
    pub elev_idx: usize,
}

struct NextOrderResult {
    next: Option<Order>,
    clear: Option<Order>,
}

const M: u8 = 3; // floors

enum Event {
    // from local elevator 
    ButtonPressed(CallButton),
    FloorReached(u8),
    OrderCompleted(CallButton),

    // from network peers
    PeerButtonPressed { from: u8, call: CallButton },
    PeerOrderCompleted { from: u8, call: CallButton },
    PeerStateUpdate(ElevatorState),
    PeerAssigned { to: u8, call: CallButton },
    AckReceived(Msg),
}

fn msg_to_event(msg: Msg) -> Option<Event> {
    match msg {
        Msg::NewHallCall { from, call }  => Some(Event::PeerButtonPressed { from, call }),
        Msg::HallCallDone { from, call } => Some(Event::PeerOrderCompleted { from, call }),
        Msg::StateUpdate(state)          => Some(Event::PeerStateUpdate(state)),
        Msg::AssignHallCall { to, call } => Some(Event::PeerAssigned { to, call }),
        Msg::WorldState { .. }           => None,
        Msg::Heartbeat                   => None,
    }
}

// TODO: add some master logic to only let master assign orders, maybe add a Role in the ManagerState
struct ManagerState {
    orders: VecDeque<Order>,
    positions: HashMap<usize, u8>,
    current_orders: HashMap<usize, Option<Order>>,
    alive_elevs: Vec<usize>,
    pending_acks: HashMap<CallButton, Order>
}

impl ManagerState {
    fn new() -> Self {
        Self {
            orders: VecDeque::with_capacity(9),
            positions: HashMap::new(),
            current_orders: HashMap::new(),
            alive_elevs: Vec::new(),
            pending_acks: HashMap::new(),
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
) {
    let local_idx = local_id as usize;
    let mut state = ManagerState::new();

    loop {
        println!("\n-\nOrders: {:?}\nCurrent orders: {:?}", state.orders, state.current_orders);

        let event = tokio::select! {
            Some(cb)       = call_request_rx.recv() => Event::ButtonPressed(cb),
            Some(floor)    = update_floor_rx.recv()  => Event::FloorReached(floor),
            Some(cb)       = call_complete_rx.recv()  => Event::OrderCompleted(cb),
            Some(msg)      = network_rx.recv()        => match msg_to_event(msg) {
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
       Event::ButtonPressed(cb) => {
            if cb.call.is_hall() {
                let order = Order { cb: cb.clone(), elev_idx: local_idx };
                state.pending_acks.insert(cb.clone(), order);
                let _ = network_tx.send(Msg::NewHallCall { from: local_id, call: cb });
            } else {
                // cab calls are local so dont need ack 
                let order = Order { cb: cb.clone(), elev_idx: local_idx };
                let _ = call_light_tx.send((cb, true));
                try_assign_new(order, state, call_assign_tx, network_tx, local_idx);
            }
        }

        // local floor update
        Event::FloorReached(floor) => {
            state.positions.insert(local_idx, floor);
            state.alive_elevs = state.positions.keys().copied().collect();
            let _ = network_tx.send(Msg::StateUpdate(ElevatorState {
                id: local_id,
                floor,
            }));
        }

        // local order complete
        Event::OrderCompleted(cb) => {
            if cb.call.is_hall() {
                let _ = network_tx.send(Msg::HallCallDone { from: local_id, call: cb.clone() });
            }
            let order = Order { cb: cb.clone(), elev_idx: local_idx };
            complete_and_reassign(order, state, call_assign_tx, call_light_tx, local_idx);
        }

        // peer pressed a hall button
        Event::PeerButtonPressed { from, call } => {
            let order = Order { cb: call, elev_idx: from as usize };
            try_assign_new(order, state, call_assign_tx, network_tx, local_idx);
        }

        // master assign call to me 
        Event::PeerAssigned { to, call } => {
            if to == local_id {
                let _ = call_assign_tx.send(call);
            }
        }

        // peer completed hall call
        Event::PeerOrderCompleted { from, call } => {
            let order = Order { cb: call.clone(), elev_idx: from as usize };
            let cab_order = Order {
                cb: CallButton { floor: call.floor, call: CallType::Cab },
                elev_idx: from as usize,
            };
            state.orders.retain(|o| o != &order);
            state.orders.retain(|o| o != &cab_order);
            state.current_orders.insert(from as usize, None);
            let _ = call_light_tx.send((call, false));
        }

        // peer state update
        Event::PeerStateUpdate(peer_state) => {
            let idx = peer_state.id as usize;
            if peer_state.floor != u8::MAX {
                // u8::MAX used to mean offline, so only update position if not offline
                state.positions.insert(idx, peer_state.floor);
            } else {
                state.positions.remove(&idx);
                if let Some(Some(order)) = state.current_orders.get(&idx) {
                    state.orders.push_front(order.clone());
                }
                state.current_orders.insert(idx, None);
            }
            state.alive_elevs = state.positions.keys().copied().collect();
        }

        // got an acked message
        Event::AckReceived(msg) => {
            if let Msg::NewHallCall { call, .. } = msg {
                if let Some(order) = state.pending_acks.remove(&call) {
                    let _ = call_light_tx.send((call, true));
                    try_assign_new(order, state, call_assign_tx, network_tx, local_idx);
                }
            }
        }
    }
}


// helpers
fn try_assign_new(
    order: Order,
    state: &mut ManagerState,
    call_assign_tx: &UTx<CallButton>,
    network_tx: &UTx<Msg>,
    local_idx: usize,
) {
    if let Some(elev_idx) = assign_new_orders(
        order.clone(),
        &mut state.orders,
        &state.positions,
        &mut state.current_orders,
        &state.alive_elevs,
    ) {
        if elev_idx == local_idx {
            let _ = call_assign_tx.send(order.cb.clone());
        } else if order.cb.call.is_hall() {
            let _ = network_tx.send(Msg::AssignHallCall {
                to: elev_idx as u8,
                call: order.cb,
            });
        }
    } else {
        println!("Could not assign new order: {:?}", order);
    }
}

fn complete_and_reassign(
    order: Order,
    state: &mut ManagerState,
    call_assign_tx: &UTx<CallButton>,
    call_light_tx: &UTx<(CallButton, bool)>,
    local_idx: usize,
) {
    let cab_order = Order {
        cb: CallButton { floor: order.cb.floor, call: CallType::Cab },
        elev_idx: order.elev_idx,
    };
    state.orders.retain(|o| o != &order);
    state.orders.retain(|o| o != &cab_order);
    state.current_orders.insert(order.elev_idx, None);
    println!("Cleared order {:?}", order);

    let result = assign_next_order(order.clone(), &mut state.orders, &mut state.current_orders);
    if let Some(ref next) = result.next {
        if next.elev_idx == local_idx {
            let _ = call_assign_tx.send(next.cb.clone());
        }
    }

    let mut to_clear: HashSet<Order> = HashSet::from([order, cab_order]);
    if let Some(clear) = result.clear {
        to_clear.insert(clear);
    }
    for cleared in &to_clear {
        let _ = call_light_tx.send((cleared.cb.clone(), false));
    }
}


// pure functions
fn assign_new_orders(
    order: Order,
    orders: &mut VecDeque<Order>,
    positions: &HashMap<usize, u8>,
    current_orders: &mut HashMap<usize, Option<Order>>,
    alive_elevs: &[usize],
) -> Option<usize> {
    if orders.iter().any(|o| o == &order) {
        return None;
    }

    *orders = rebuild_queue(orders, order.clone());
    let (busy_elevs, available_elevs) =
        designate_busy_idle(alive_elevs.to_vec(), current_orders, order.clone());

    if let Some((elev_idx, paused_order)) =
        find_order_otw(busy_elevs, &order, current_orders, positions)
    {
        println!("Found order on the way to {:?}", paused_order);
        orders.push_front(paused_order);
        orders.retain(|o| o != &order);
        current_orders.insert(elev_idx, Some(order));
        return Some(elev_idx);
    }

    if let Some(closest) = find_closest_elev(available_elevs, &order, positions) {
        current_orders.insert(closest, Some(order.clone()));
        orders.retain(|o| o != &order);
        return Some(closest);
    }

    None
}

fn assign_next_order(
    completed: Order,
    orders: &mut VecDeque<Order>,
    current_orders: &mut HashMap<usize, Option<Order>>,
) -> NextOrderResult {
    let mut result = NextOrderResult { next: None, clear: None };
    let eligible = get_eligible_orders(orders, completed.clone());

    match completed.cb.call {
        CallType::HallUp => {
            if let Some(cab) = find_cab_order(eligible.clone(), completed.cb.floor, CallType::HallUp) {
                result.next = Some(cab);
            } else {
                match should_change_direction(eligible.first().cloned(), completed.cb.floor, CallType::HallUp) {
                    Some(true) => result.next = Some(Order {
                        cb: CallButton { floor: completed.cb.floor, call: CallType::HallDown },
                        elev_idx: completed.elev_idx,
                    }),
                    Some(false) => {
                        result.next = eligible.first().cloned();
                        result.clear = Some(Order {
                            cb: CallButton { floor: completed.cb.floor, call: CallType::HallUp },
                            elev_idx: completed.elev_idx,
                        });
                    }
                    None => {}
                }
            }
        }

        CallType::HallDown => {
            if let Some(cab) = find_cab_order(eligible.clone(), completed.cb.floor, CallType::HallDown) {
                result.next = Some(cab);
            } else {
                match should_change_direction(eligible.first().cloned(), completed.cb.floor, CallType::HallDown) {
                    Some(true) => result.next = Some(Order {
                        cb: CallButton { floor: completed.cb.floor, call: CallType::HallUp },
                        elev_idx: completed.elev_idx,
                    }),
                    Some(false) => {
                        result.next = eligible.first().cloned();
                        result.clear = Some(Order {
                            cb: CallButton { floor: completed.cb.floor, call: CallType::HallDown },
                            elev_idx: completed.elev_idx,
                        });
                    }
                    None => {}
                }
            }
        }

        CallType::Cab => {
            if let Some(order) = eligible.first().cloned() {
                result.clear = Some(Order {
                    cb: CallButton {
                        floor: completed.cb.floor,
                        call: if order.cb.floor > completed.cb.floor { CallType::HallUp } else { CallType::HallDown },
                    },
                    elev_idx: completed.elev_idx,
                });
                result.next = Some(order);
            }
        }
    }

    if result.next.is_some() {
        let order = find_closest_order(
            result.next.as_ref().unwrap().clone(),
            eligible,
            completed.clone(),
        );
        orders.retain(|o| o != &order);
        current_orders.insert(completed.elev_idx, Some(order.clone()));
        result.next = Some(order);
    }

    result
}


fn elevator_may_take_order(elev_idx: usize, order: &Order) -> bool {
    order.cb.call != CallType::Cab || order.elev_idx == elev_idx
}

fn get_eligible_orders(orders: &VecDeque<Order>, completed: Order) -> Vec<Order> {
    orders
        .iter()
        .filter(|o| elevator_may_take_order(completed.elev_idx, o))
        .cloned()
        .collect()
}

fn designate_busy_idle(
    alive_elevs: Vec<usize>,
    current_orders: &HashMap<usize, Option<Order>>,
    order: Order,
) -> (Vec<usize>, Vec<usize>) {
    let busy: Vec<usize> = alive_elevs.iter().copied()
        .filter(|&i| current_orders.get(&i).and_then(|o| o.as_ref()).is_some())
        .collect();
    let idle: Vec<usize> = alive_elevs.iter().copied()
        .filter(|&i| !busy.contains(&i) && elevator_may_take_order(i, &order))
        .collect();
    (busy, idle)
}

fn rebuild_queue(orders: &mut VecDeque<Order>, order: Order) -> VecDeque<Order> {
    let mut cab_orders: VecDeque<Order> = orders.iter().filter(|o| o.cb.call == CallType::Cab).cloned().collect();
    let mut hall_orders: VecDeque<Order> = orders.iter().filter(|o| o.cb.call != CallType::Cab).cloned().collect();
    let mut new_orders = VecDeque::with_capacity(orders.len() + 1);
    new_orders.append(&mut cab_orders);
    new_orders.append(&mut hall_orders);
    new_orders.push_back(order);
    new_orders
}

fn order_on_the_way(elev_idx: usize, position: u8, curr_order: Order, new_order: Order) -> bool {
    let new_floor = new_order.cb.floor;
    let curr_floor = curr_order.cb.floor;
    let new_call = new_order.cb.call;
    let curr_call = curr_order.cb.call;

    let is_below = curr_floor <= new_floor && new_floor < position;
    let is_above = curr_floor >= new_floor && new_floor > position;
    let on_way_below = (new_call == CallType::HallDown && curr_call != CallType::HallUp) || new_call == CallType::Cab;
    let on_way_above = (new_call == CallType::HallUp && curr_call != CallType::HallDown) || new_call == CallType::Cab;

    (is_below && on_way_below && elevator_may_take_order(elev_idx, &new_order))
        || (is_above && on_way_above && elevator_may_take_order(elev_idx, &new_order))
}

fn find_closest_elev(
    candidates: Vec<usize>,
    order: &Order,
    positions: &HashMap<usize, u8>,
) -> Option<usize> {
    candidates.into_iter()
        .filter_map(|idx| positions.get(&idx).map(|&pos| (idx, pos)))
        .min_by_key(|&(_, pos)| u8::abs_diff(pos, order.cb.floor))
        .map(|(idx, _)| idx)
}

fn find_order_otw(
    busy_elevs: Vec<usize>,
    order: &Order,
    current_orders: &HashMap<usize, Option<Order>>,
    positions: &HashMap<usize, u8>,
) -> Option<(usize, Order)> {
    busy_elevs.into_iter().find_map(|idx| {
        let curr = current_orders.get(&idx)?.as_ref()?.clone();
        let &pos = positions.get(&idx)?;
        if order_on_the_way(idx, pos, curr.clone(), order.clone()) {
            Some((idx, curr))
        } else {
            None
        }
    })
}

fn find_closest_order(target: Order, eligible: Vec<Order>, completed: Order) -> Order {
    eligible.into_iter()
        .filter(|o| order_on_the_way(completed.elev_idx, completed.cb.floor, target.clone(), o.clone()))
        .min_by_key(|o| u8::abs_diff(completed.cb.floor, o.cb.floor))
        .unwrap_or(target)
}

fn find_cab_order(orders: Vec<Order>, floor: u8, dir: CallType) -> Option<Order> {
    orders.into_iter().find(|o| {
        o.cb.call == CallType::Cab && match dir {
            CallType::HallUp   => o.cb.floor > floor,
            CallType::HallDown => o.cb.floor < floor,
            CallType::Cab      => false,
        }
    })
}

fn should_change_direction(order: Option<Order>, floor: u8, dir: CallType) -> Option<bool> {
    let order = order?;
    Some(match dir {
        CallType::HallUp   => order.cb.floor < floor,
        CallType::HallDown => order.cb.floor > floor,
        CallType::Cab      => false,
    })
}