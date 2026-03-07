use std::collections::{HashMap, HashSet, VecDeque};
use tokio::sync::mpsc::{
    UnboundedReceiver as URx, UnboundedSender as UTx, unbounded_channel as uc,
};

use crate::elevator::elevio::poll::{CallButton, CallType};
use crate::networking::types::{Direction, ElevatorState, Msg};

#[derive(Clone, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Order {
    pub cb: CallButton,
    pub elev_idx: usize,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Status {
    pub floor: Option<u8>,
    pub elev_idx: usize,
}

struct NextOrderResult {
    next: Option<Order>,
    clear: Option<Order>,
}

const M: u8 = 3; // number of floors

pub async fn order_management_runner(
    local_id: u8,
    // local elevator channels (CallButton-based)
    call_request_rx: URx<CallButton>,
    call_assign_tx: UTx<CallButton>,
    update_floor_rx: URx<u8>,
    call_complete_rx: URx<CallButton>,
    call_light_assign_tx: UTx<(CallButton, bool)>,
    // network channels (Msg-based)
    network_tx: UTx<Msg>,
    network_rx: URx<Msg>,
    ack_complete_rx: URx<(u32, Msg)>,
) {
    let local_idx = local_id as usize;

    // internal channels between router and order_manager
    let (order_request_tx, order_request_rx) = uc::<Order>();
    let (order_assign_internal_tx, order_assign_internal_rx) = uc::<Order>();
    let (update_status_tx, update_status_rx) = uc::<Status>();
    let (order_complete_tx, order_complete_rx) = uc::<Order>();
    let (order_light_internal_tx, order_light_internal_rx) = uc::<(Order, bool)>();

    // router: bridges local elevator (CallButton) and network (Msg) with internal Order channels
    let router_task = tokio::spawn(async move {
        router(
            local_id,
            local_idx,
            call_request_rx,
            call_assign_tx,
            update_floor_rx,
            call_complete_rx,
            call_light_assign_tx,
            network_tx,
            network_rx,
            ack_complete_rx,
            order_request_tx,
            order_assign_internal_rx,
            update_status_tx,
            order_complete_tx,
            order_light_internal_rx,
        )
        .await;
    });

    let order_manager_task = tokio::spawn(async move {
        order_manager(
            order_request_rx,
            update_status_rx,
            order_complete_rx,
            order_assign_internal_tx,
            order_light_internal_tx,
        )
        .await;
    });

    let _ = tokio::join!(router_task, order_manager_task);
}

async fn router(
    local_id: u8,
    local_idx: usize,
    // local elevator
    mut call_request_rx: URx<CallButton>,
    call_assign_tx: UTx<CallButton>,
    mut update_floor_rx: URx<u8>,
    mut call_complete_rx: URx<CallButton>,
    call_light_assign_tx: UTx<(CallButton, bool)>,
    // network
    network_tx: UTx<Msg>,
    mut network_rx: URx<Msg>,
    mut ack_complete_rx: URx<(u32, Msg)>,
    // internal order management channels
    order_request_tx: UTx<Order>,
    mut order_assign_rx: URx<Order>,
    update_status_tx: UTx<Status>,
    order_complete_tx: UTx<Order>,
    mut order_light_rx: URx<(Order, bool)>,
) {
    loop {
        tokio::select! {
            // --- Local elevator → internal ---

            Some(cb) = call_request_rx.recv() => {
                let order = Order { cb: cb.clone(), elev_idx: local_idx };
                let _ = order_request_tx.send(order);
                if cb.call.is_hall() {
                    let _ = network_tx.send(Msg::NewHallCall { from: local_id, call: cb });
                }
            }

            Some(floor) = update_floor_rx.recv() => {
                let _ = update_status_tx.send(Status { floor: Some(floor), elev_idx: local_idx });
                let _ = network_tx.send(Msg::StateUpdate(ElevatorState {
                    id: local_id,
                    floor,
                    direction: Direction::Idle,
                    door_open: false,
                    cab_calls: vec![],
                }));
            }

            Some(cb) = call_complete_rx.recv() => {
                let _ = order_complete_tx.send(Order { cb: cb.clone(), elev_idx: local_idx });
                if cb.call.is_hall() {
                    let _ = network_tx.send(Msg::HallCallDone { from: local_id, call: cb });
                }
            }

            // --- Network → internal ---

            Some(msg) = network_rx.recv() => {
                match msg {
                    Msg::NewHallCall { from, call } => {
                        let _ = order_request_tx.send(Order { cb: call, elev_idx: from as usize });
                    }
                    Msg::AssignHallCall { to, call } => {
                        if to == local_id {
                            let _ = call_assign_tx.send(call);
                        }
                    }
                    Msg::HallCallDone { from, call } => {
                        let _ = order_complete_tx.send(Order { cb: call, elev_idx: from as usize });
                    }
                    Msg::StateUpdate(state) => {
                        let _ = update_status_tx.send(Status {
                            floor: Some(state.floor),
                            elev_idx: state.id as usize,
                        });
                    }
                    Msg::WorldState { .. } => {}
                    Msg::Heartbeat => {}
                }
            }

            // --- Internal → local elevator / network ---

            Some(order) = order_assign_rx.recv() => {
                if order.elev_idx == local_idx {
                    let _ = call_assign_tx.send(order.cb);
                } else if order.cb.call.is_hall() {
                    let _ = network_tx.send(Msg::AssignHallCall {
                        to: order.elev_idx as u8,
                        call: order.cb,
                    });
                }
            }

            Some((order, on)) = order_light_rx.recv() => {
                let _ = call_light_assign_tx.send((order.cb, on));
            }

            Some((_seq, _msg)) = ack_complete_rx.recv() => {
                // all peers acknowledged the message
            }
        }
    }
}

// manages order queue, assignment, and lights
async fn order_manager(
    mut order_request_rx: URx<Order>,
    mut update_status_rx: URx<Status>,
    mut order_complete_rx: URx<Order>,
    order_assign_tx: UTx<Order>,
    order_light_tx: UTx<(Order, bool)>,
) {
    let mut orders: VecDeque<Order> = VecDeque::with_capacity(3 * M as usize);
    let mut positions: HashMap<usize, u8> = HashMap::new();
    let mut current_orders: HashMap<usize, Option<Order>> = HashMap::new();
    let mut alive_elevs: Vec<usize> = Vec::new();

    // TODO: Watchdog timer

    loop {
        println!("");
        println!("-");
        println!("Orders: {:?}", orders);
        println!("Current orders: {:?}", current_orders);

        tokio::select! {
            Some(order) = order_request_rx.recv() => {
                // TODO: When network ack logic is added, orders should start as
                // unconfirmed and only be assigned after ack from peers.
                let _ = order_light_tx.send((order.clone(), true));

                let new_order_found = assign_new_orders(order.clone(), &mut orders, &mut positions, &mut current_orders, &alive_elevs);
                if let Some(order_elev_idx) = new_order_found {
                    let _ = order_assign_tx.send(Order { cb: order.cb.clone(), elev_idx: order_elev_idx });
                } else {
                    println!("Could not assign new order");
                }
            }

            Some(order) = order_complete_rx.recv() => {
                // clear completed order and its cab counterpart
                let cab_order = Order { cb: CallButton { floor: order.cb.floor, call: CallType::Cab }, elev_idx: order.elev_idx };
                orders.retain(|item| item != &order);
                orders.retain(|item| item != &cab_order);
                current_orders.insert(order.elev_idx, None);
                println!("Cleared order {:?}. ", order);

                // find next order for this elevator
                let elev_idx = order.elev_idx;
                let result = assign_next_order(order.clone(), &mut orders, &mut current_orders);
                if let Some(ref next) = result.next {
                    let _ = order_assign_tx.send(Order { cb: next.cb.clone(), elev_idx });
                }

                // turn off lights for cleared orders
                let mut clear_orders: HashSet<Order> = HashSet::new();
                clear_orders.insert(order.clone());
                clear_orders.insert(cab_order);
                if let Some(clear) = result.clear {
                    clear_orders.insert(clear);
                }
                for cleared in &clear_orders {
                    let _ = order_light_tx.send((cleared.clone(), false));
                }
            }

            Some(status) = update_status_rx.recv() => {
                if let Some(floor) = status.floor {
                    positions.insert(status.elev_idx, floor);
                } else {
                    positions.remove(&status.elev_idx);
                    if let Some(Some(order)) = current_orders.get(&status.elev_idx) {
                        orders.push_front(order.clone());
                    }
                    current_orders.insert(status.elev_idx, None);
                }
                alive_elevs = positions.keys().copied().collect();
            }

            else => {
                panic!("All channels closed");
            }
        }
    }
}

// assign order to elevator if there is no current order OR assign order on the way to the current order
fn assign_new_orders(
    order: Order,
    orders: &mut VecDeque<Order>,
    positions: &HashMap<usize, u8>,
    current_orders: &mut HashMap<usize, Option<Order>>,
    alive_elevs: &Vec<usize>,
) -> Option<usize> {
    if orders.iter().any(|o| o == &order) {
        return None;
    }

    *orders = rebuild_queue(orders, order.clone());
    let (busy_elevs, available_elevs) =
        designate_busy_idle(alive_elevs.clone(), current_orders, order.clone());

    // See if any busy elevator can take the order on the way
    if let Some((elev_idx, paused_order)) =
        find_order_otw(busy_elevs, &order, current_orders, positions)
    {
        println!("Found order on the way to {:?}", paused_order);
        orders.push_front(paused_order);
        orders.retain(|item| item != &order);
        current_orders.insert(elev_idx, Some(order));
        return Some(elev_idx);
    }

    // Assign the order to the closest available elevator
    if let Some(closest_elev) = find_closest_elev(available_elevs, &order, positions) {
        current_orders.insert(closest_elev, Some(order.clone()));
        orders.retain(|item| item != &order);
        return Some(closest_elev);
    }
    None
}

fn assign_next_order(
    completed_order: Order,
    orders: &mut VecDeque<Order>,
    current_orders: &mut HashMap<usize, Option<Order>>,
) -> NextOrderResult {
    let mut result = NextOrderResult { next: None, clear: None };
    let eligible_orders = get_eligible_orders(orders, completed_order.clone());

    match completed_order.cb.call {
        CallType::HallUp => 'hall_up: {
            if let Some(cab_order) = find_cab_order(eligible_orders.clone(), completed_order.cb.floor, CallType::HallUp) {
                result.next = Some(cab_order);
                break 'hall_up;
            }

            match should_change_direction(eligible_orders.first().cloned(), completed_order.cb.floor, CallType::HallUp) {
                Some(true) => {
                    result.next = Some(Order {
                        cb: CallButton { floor: completed_order.cb.floor, call: CallType::HallDown },
                        elev_idx: completed_order.elev_idx,
                    });
                }
                Some(false) => {
                    result.next = eligible_orders.first().cloned();
                    result.clear = Some(Order {
                        cb: CallButton { floor: completed_order.cb.floor, call: CallType::HallUp },
                        elev_idx: completed_order.elev_idx,
                    });
                }
                None => (),
            };
        }
        CallType::HallDown => 'hall_down: {
            if let Some(cab_order) = find_cab_order(eligible_orders.clone(), completed_order.cb.floor, CallType::HallDown) {
                result.next = Some(cab_order);
                break 'hall_down;
            }

            match should_change_direction(eligible_orders.first().cloned(), completed_order.cb.floor, CallType::HallDown) {
                Some(true) => {
                    result.next = Some(Order {
                        cb: CallButton { floor: completed_order.cb.floor, call: CallType::HallUp },
                        elev_idx: completed_order.elev_idx,
                    });
                }
                Some(false) => {
                    result.next = eligible_orders.first().cloned();
                    result.clear = Some(Order {
                        cb: CallButton { floor: completed_order.cb.floor, call: CallType::HallDown },
                        elev_idx: completed_order.elev_idx,
                    });
                }
                None => (),
            };
        }
        CallType::Cab => {
            if let Some(order) = eligible_orders.first().cloned() {
                result.next = Some(order.clone());
                if order.cb.floor > completed_order.cb.floor {
                    result.clear = Some(Order {
                        cb: CallButton { floor: completed_order.cb.floor, call: CallType::HallUp },
                        elev_idx: completed_order.elev_idx,
                    });
                } else if order.cb.floor < completed_order.cb.floor {
                    result.clear = Some(Order {
                        cb: CallButton { floor: completed_order.cb.floor, call: CallType::HallDown },
                        elev_idx: completed_order.elev_idx,
                    });
                }
            }
        }
    }

    // see if there are any orders on the way to selected order
    if result.next.is_some() {
        let elev_idx = completed_order.elev_idx;
        let order = find_closest_order(
            result.next.as_ref().unwrap().clone(),
            eligible_orders.clone(),
            completed_order,
        );
        orders.retain(|item| item != &order);
        result.next = Some(order.clone());
        current_orders.insert(elev_idx, Some(order));
    }
    result
}

// ---------- PURE FUNCTIONS ----------

fn elevator_may_take_order(elev_idx: usize, order: &Order) -> bool {
    order.cb.call != CallType::Cab || order.elev_idx == elev_idx
}

fn get_eligible_orders(orders: &VecDeque<Order>, completed_order: Order) -> Vec<Order> {
    orders
        .iter()
        .filter(|order| elevator_may_take_order(completed_order.elev_idx, order))
        .cloned()
        .collect()
}

fn designate_busy_idle(
    alive_elevs: Vec<usize>,
    current_orders: &HashMap<usize, Option<Order>>,
    order: Order,
) -> (Vec<usize>, Vec<usize>) {
    let busy_elevs: Vec<usize> = alive_elevs
        .iter()
        .copied()
        .filter(|&i| current_orders.get(&i).and_then(|o| o.as_ref()).is_some())
        .collect();
    let free_elevs: Vec<usize> = alive_elevs
        .iter()
        .copied()
        .filter(|&i| !busy_elevs.contains(&i))
        .collect();
    let idle_elevs: Vec<usize> = free_elevs
        .iter()
        .copied()
        .filter(|&i| elevator_may_take_order(i, &order))
        .collect();
    (busy_elevs, idle_elevs)
}

fn rebuild_queue(orders: &mut VecDeque<Order>, order: Order) -> VecDeque<Order> {
    let mut cab_orders: VecDeque<Order> = VecDeque::with_capacity(orders.len());
    let mut other_orders: VecDeque<Order> = VecDeque::with_capacity(orders.len());
    for order in orders.iter() {
        if order.cb.call == CallType::Cab {
            cab_orders.push_back(order.clone());
        } else {
            other_orders.push_back(order.clone());
        }
    }
    let mut new_orders: VecDeque<Order> = VecDeque::with_capacity(orders.len());
    new_orders.extend(cab_orders);
    new_orders.extend(other_orders);
    new_orders.push_back(order);
    new_orders
}

fn order_on_the_way(elev_idx: usize, position: u8, curr_order: Order, new_order: Order) -> bool {
    let new_call = new_order.cb.call;
    let new_floor = new_order.cb.floor;
    let curr_call = curr_order.cb.call;
    let curr_floor = curr_order.cb.floor;

    let is_below = curr_floor <= new_floor && new_floor < position;
    let is_above = curr_floor >= new_floor && new_floor > position;

    let on_way_below = (new_call == CallType::HallDown && curr_call != CallType::HallUp) || new_call == CallType::Cab;
    let on_way_above = (new_call == CallType::HallUp && curr_call != CallType::HallDown) || new_call == CallType::Cab;

    (is_below && on_way_below && elevator_may_take_order(elev_idx, &new_order))
        || (is_above && on_way_above && elevator_may_take_order(elev_idx, &new_order))
}

fn find_closest_elev(
    elev_candidates: Vec<usize>,
    order: &Order,
    positions: &HashMap<usize, u8>,
) -> Option<usize> {
    let mut closest_elev: Option<usize> = None;
    let mut closest_distance: u8 = M + 1;

    for elev_idx in elev_candidates {
        if let Some(&position) = positions.get(&elev_idx) {
            let dist = u8::abs_diff(position, order.cb.floor);
            if dist < closest_distance {
                closest_distance = dist;
                closest_elev = Some(elev_idx);
            }
        }
    }

    closest_elev
}

fn find_order_otw(
    busy_elevs: Vec<usize>,
    order: &Order,
    current_orders: &HashMap<usize, Option<Order>>,
    positions: &HashMap<usize, u8>,
) -> Option<(usize, Order)> {
    for elev_idx in busy_elevs {
        if let Some(Some(curr_order)) = current_orders.get(&elev_idx).cloned() {
            if let Some(&position) = positions.get(&elev_idx) {
                if order_on_the_way(elev_idx, position, curr_order.clone(), order.clone()) {
                    return Some((elev_idx, curr_order));
                }
            }
        }
    }
    None
}

fn find_closest_order(order: Order, eligible_orders: Vec<Order>, completed_order: Order) -> Order {
    let mut closest_order: Order = order.clone();
    let mut closest_distance: u8 = M + 1;

    for eligible_order in eligible_orders.iter() {
        if order_on_the_way(
            completed_order.elev_idx,
            completed_order.cb.floor,
            order.clone(),
            eligible_order.clone(),
        ) {
            let dist = u8::abs_diff(completed_order.cb.floor, eligible_order.cb.floor);
            if dist < closest_distance {
                closest_distance = dist;
                closest_order = eligible_order.clone();
            }
        }
    }
    println!("Closest order is {:?}", closest_order);
    closest_order
}

fn find_cab_order(orders: Vec<Order>, floor: u8, dir: CallType) -> Option<Order> {
    for order in orders.iter() {
        let is_cab = order.cb.call == CallType::Cab;
        match dir {
            CallType::HallUp => {
                if order.cb.floor > floor && is_cab {
                    return Some(order.clone());
                }
            }
            CallType::HallDown => {
                if order.cb.floor < floor && is_cab {
                    return Some(order.clone());
                }
            }
            CallType::Cab => (),
        }
    }
    None
}

fn should_change_direction(order: Option<Order>, floor: u8, dir: CallType) -> Option<bool> {
    let order = order?;
    match dir {
        CallType::HallUp => {
            if order.cb.floor < floor {
                return Some(true);
            }
        }
        CallType::HallDown => {
            if order.cb.floor > floor {
                return Some(true);
            }
        }
        CallType::Cab => (),
    }
    Some(false)
}
