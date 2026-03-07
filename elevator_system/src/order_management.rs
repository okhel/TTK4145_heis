use std::collections::{HashMap, HashSet, VecDeque};
use tokio::sync::mpsc::{
    UnboundedReceiver as URx, UnboundedSender as UTx, unbounded_channel as uc,
};

use crate::elevator::elevio::poll::CallButton;
use crate::networking::types::{Direction, ElevatorState, HallCall, Msg};

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

enum OrderID {
    Unconfirmed,
    Confirmed,
}

const M: u8 = 3; // number of floors

pub async fn order_management_runner(
    local_id: u8,
    // Local elevator channels (CallButton-based)
    call_request_rx: URx<CallButton>,
    call_assign_tx: UTx<CallButton>,
    update_floor_rx: URx<u8>,
    call_complete_rx: URx<CallButton>,
    call_light_assign_tx: UTx<(CallButton, bool)>,
    // Network channels (Msg-based)
    network_tx: UTx<Msg>,
    network_rx: URx<Msg>,
    ack_complete_rx: URx<(u32, Msg)>,
) {
    let local_idx = local_id as usize;

    // Internal channels between router and order_queuer/order_delegator
    let (order_request_tx, order_request_rx) = uc::<Order>();
    let (order_assign_internal_tx, order_assign_internal_rx) = uc::<Order>();
    let (update_status_tx, update_status_rx) = uc::<Status>();
    let (order_complete_tx, order_complete_rx) = uc::<Order>();
    let (order_light_internal_tx, order_light_internal_rx) = uc::<(Order, bool)>();

    let (order_confirmed_tx, order_confirmed_rx) = uc::<Order>();
    let (order_clear_tx, order_clear_rx) = uc::<HashSet<Order>>();

    // Router: bridges local elevator (CallButton) and network (Msg) with internal Order channels
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

    let order_queuing_task = tokio::spawn(async move {
        order_queuer(
            order_request_rx,
            order_clear_rx,
            order_confirmed_tx,
            order_light_internal_tx,
        )
        .await;
    });
    let order_delegator_task = tokio::spawn(async move {
        order_delegator(
            order_confirmed_rx,
            update_status_rx,
            order_complete_rx,
            order_assign_internal_tx,
            order_clear_tx,
        )
        .await;
    });
    let _ = tokio::join!(router_task, order_queuing_task, order_delegator_task);
}

async fn router(
    local_id: u8,
    local_idx: usize,
    // Local elevator
    mut call_request_rx: URx<CallButton>,
    call_assign_tx: UTx<CallButton>,
    mut update_floor_rx: URx<u8>,
    mut call_complete_rx: URx<CallButton>,
    call_light_assign_tx: UTx<(CallButton, bool)>,
    // Network
    network_tx: UTx<Msg>,
    mut network_rx: URx<Msg>,
    mut ack_complete_rx: URx<(u32, Msg)>,
    // Internal order management channels
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
                // also broadcast hall calls to network so master sees them
                if let Some(hc) = cb_to_hall_call(&cb) {
                    let _ = network_tx.send(Msg::NewHallCall { from: local_id, call: hc });
                }
            }

            Some(floor) = update_floor_rx.recv() => {
                let _ = update_status_tx.send(Status { floor: Some(floor), elev_idx: local_idx });
                let _ = network_tx.send(Msg::StateUpdate(ElevatorState {
                    id: local_id,
                    floor,
                    direction: Direction::Idle,
                    door_open: false,
                    assigned_hall_calls: vec![],
                    cab_calls: vec![],
                }));
            }

            Some(cb) = call_complete_rx.recv() => {
                let _ = order_complete_tx.send(Order { cb: cb.clone(), elev_idx: local_idx });
                if let Some(hc) = cb_to_hall_call(&cb) {
                    let _ = network_tx.send(Msg::HallCallDone { from: local_id, call: hc });
                }
            }

            // --- Network → internal ---

            Some(msg) = network_rx.recv() => {
                match msg {
                    Msg::NewHallCall { from, call } => {
                        let cb = hall_call_to_cb(&call);
                        let _ = order_request_tx.send(Order { cb, elev_idx: from as usize });
                    }
                    Msg::AssignHallCall { to, call } => {
                        if to == local_id {
                            let cb = hall_call_to_cb(&call);
                            let _ = call_assign_tx.send(cb);
                        }
                    }
                    Msg::HallCallDone { from, call } => {
                        let cb = hall_call_to_cb(&call);
                        let _ = order_complete_tx.send(Order { cb, elev_idx: from as usize });
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
                } else {
                    if let Some(hc) = cb_to_hall_call(&order.cb) {
                        let _ = network_tx.send(Msg::AssignHallCall {
                            to: order.elev_idx as u8,
                            call: hc,
                        });
                    }
                }
            }

            Some((order, on)) = order_light_rx.recv() => {
                let _ = call_light_assign_tx.send((order.cb, on));
            }

            Some((_seq, _msg)) = ack_complete_rx.recv() => {
                // All peers acknowledged the message
            }
        }
    }
}

pub async fn order_queuer(
    mut order_request_rx: URx<Order>,
    mut order_clear_rx: URx<HashSet<Order>>,
    order_confirmed_tx: UTx<Order>,
    order_light_assign_tx: UTx<(Order, bool)>,
) {
    let mut order_queue: HashMap<Order, OrderID> = HashMap::new();

    loop {
        tokio::select! {
            // TODO: Watchdog timer on changes to order_queue. No changes -> resend queue to order_delegator
            Some(order) = order_request_rx.recv() => {
                order_queue.insert(order.clone(), OrderID::Unconfirmed);

                // TODO: Send order to network

            // TODO: Add order ack from network
            // }
            // Some(order) = order_ack_rx.recv() => {
                order_queue.insert(order.clone(), OrderID::Confirmed);
                let _ = order_light_assign_tx.send((order.clone(), true));
                let _ = order_confirmed_tx.send(order);
            }

            // TODO: Send the order queue to the network
            // Some(elev_idx) = new_elev_alive_rx.recv() => {
            //     let _ = order_queue_tx.send(order_queue.clone());
            // }
            // TODO: Send the list of confirmed orders to order_delegator
            // Some(_) = im_now_master.recv() => {
            //     for order in order_queue.keys() {
            //         if order_queue.get(order).unwrap() == &OrderID::Confirmed {
            //             let _ = order_confirmed_tx.send(order.clone());
            //         }
            //     }
            // }
            Some(clear_orders) = order_clear_rx.recv() => {
                for order in clear_orders.iter() {
                    order_queue.remove(order);
                    let _ = order_light_assign_tx.send((order.clone(), false));
                }
            }
            else => {
                panic!("All channels closed");
            }
        }
    }
}

pub async fn order_delegator(
    mut order_confirmed_rx: URx<Order>,
    mut update_status_rx: URx<Status>,
    mut order_complete_rx: URx<Order>,
    order_assign_tx: UTx<Order>,
    order_clear_tx: UTx<HashSet<Order>>,
) {
    let mut orders: VecDeque<Order> = VecDeque::with_capacity(3 * M as usize); // Ring buffer of all orders
    let mut positions: HashMap<usize, u8> = HashMap::new(); // Dictionary of current positions for each elevator
    let mut current_orders: HashMap<usize, Option<Order>> = HashMap::new(); // Dictionary of current order for each elevator
    let mut alive_elevs: Vec<usize> = Vec::new(); // List of alive elevator indices

    // TODO: Watchdog timer!

    // (re)assign orders whenever a new order is received or the status of an elevator changes
    loop {
        println!("");
        println!("-");
        println!("Orders: {:?}", orders);
        println!("Current orders: {:?}", current_orders);

        tokio::select! {

            Some(order) = order_confirmed_rx.recv() => {

                // ---------- ASSIGN NEW ORDER ----------
                let new_order_found = assign_new_orders(order.clone(), &mut orders, &mut positions, &mut current_orders, &alive_elevs);
                if let Some(order_elev_idx) = new_order_found {
                    let _ = order_assign_tx.send(Order { cb: order.cb.clone(), elev_idx: order_elev_idx});
                }
                else {println!("Could not assign new order");}
            }

            Some(order) = order_complete_rx.recv() => {

                // ---------- CLEAR ORDER LOCAL ----------
                orders.retain(|item| item != &order);
                orders.retain(|item| item != &Order { cb: CallButton { floor: order.cb.floor, call: 2 }, elev_idx: order.elev_idx });
                current_orders.insert(order.elev_idx, None);
                println!("Cleared order {:?}. ", order);


                // ---------- FIND NEXT ORDER ----------
                let elev_idx = order.elev_idx;
                let (next_order, clear_call) = assign_next_order(order.clone(), &mut orders, &mut current_orders);
                if next_order.is_some() {
                    let _ = order_assign_tx.send(Order { cb: next_order.as_ref().unwrap().cb.clone(), elev_idx: elev_idx });
                }


                // ---------- CLEAR ORDER SHARED ----------
                let clear_orders: HashSet<Order> = [
                    order.clone(),
                    Order { cb: CallButton { floor: order.cb.floor, call: 2 }, elev_idx: order.elev_idx }
                ]
                .into_iter().chain(clear_call).collect();
                let _ = order_clear_tx.send(clear_orders);
            }
            Some(status) = update_status_rx.recv() => {
                if status.floor.is_some() {
                    positions.insert(status.elev_idx, status.floor.unwrap());
                }
                else {
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

// Assign order to elevator if there is no current order OR assign order on the way to the current order
fn assign_new_orders(
    order: Order,
    orders: &mut VecDeque<Order>,
    positions: &HashMap<usize, u8>,
    current_orders: &mut HashMap<usize, Option<Order>>,
    alive_elevs: &Vec<usize>,
) -> Option<usize> {
    // If the order already exists in the queue, return None
    if orders.iter().any(|o| o == &order) {
        return None;
    }

    *orders = rebuild_queue(orders, order.clone());
    let (busy_elevs, available_elevs) =
        designate_busy_idle(alive_elevs.clone(), current_orders, order.clone());

    // BUSY ELEVATORS
    // See if any elevator can take the order on the way
    if let Some((elev_idx, paused_order)) =
        find_order_otw(busy_elevs, &order, current_orders, positions)
    {
        println!("Found order on the way to {:?}", paused_order);
        orders.push_front(paused_order);
        orders.retain(|item| item != &order);
        current_orders.insert(elev_idx, Some(order));
        return Some(elev_idx);
    }

    // AVAILABLE ELEVATORS
    // Assign the order to the closest elevator
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
) -> (Option<Order>, Option<Order>) {
    let mut order_found: (Option<Order>, Option<Order>) = (None, None);
    let eligble_orders = get_eligible_orders(orders, completed_order.clone());

    match completed_order.cb.call {
        0 => 'HallUp: {
            // Find closest cab order above
            if let Some(cab_order) =
                find_cab_order(eligble_orders.clone(), completed_order.cb.floor, 0)
            {
                order_found.0 = Some(cab_order);
                break 'HallUp;
            }

            // Pick first order in the queue
            match should_change_direction(
                eligble_orders.first().cloned(),
                completed_order.cb.floor,
                0,
            ) {
                Some(true) => {
                    order_found.0 = Some(Order {
                        cb: CallButton {
                            floor: completed_order.cb.floor,
                            call: 1,
                        },
                        elev_idx: completed_order.elev_idx,
                    });
                }
                Some(false) => {
                    let order = eligble_orders.first().cloned().unwrap();
                    order_found.0 = Some(order);
                    order_found.1 = Some(Order {
                        cb: CallButton {
                            floor: completed_order.cb.floor,
                            call: 0,
                        },
                        elev_idx: completed_order.elev_idx,
                    });
                }
                None => (),
            };
        }
        1 => 'HallDown: {
            // Find closest cab order below
            if let Some(cab_order) =
                find_cab_order(eligble_orders.clone(), completed_order.cb.floor, 1)
            {
                order_found.0 = Some(cab_order);
                break 'HallDown;
            }

            // Pick first order in the queue
            match should_change_direction(
                eligble_orders.first().cloned(),
                completed_order.cb.floor,
                1,
            ) {
                Some(true) => {
                    order_found.0 = Some(Order {
                        cb: CallButton {
                            floor: completed_order.cb.floor,
                            call: 0,
                        },
                        elev_idx: completed_order.elev_idx,
                    });
                }
                Some(false) => {
                    let order = eligble_orders.first().cloned().unwrap();
                    order_found.0 = Some(order);
                    order_found.1 = Some(Order {
                        cb: CallButton {
                            floor: completed_order.cb.floor,
                            call: 1,
                        },
                        elev_idx: completed_order.elev_idx,
                    });
                }
                None => (),
            };
        }
        _ => 'Cab: {
            // Pick first eligible order in the queue
            if let Some(order) = eligble_orders.first().cloned() {
                order_found.0 = Some(order);
                if order_found.0.as_ref().unwrap().cb.floor > completed_order.cb.floor {
                    order_found.1 = Some(Order {
                        cb: CallButton {
                            floor: completed_order.cb.floor,
                            call: 0,
                        },
                        elev_idx: completed_order.elev_idx,
                    });
                    break 'Cab;
                }
                if order_found.0.as_ref().unwrap().cb.floor < completed_order.cb.floor {
                    order_found.1 = Some(Order {
                        cb: CallButton {
                            floor: completed_order.cb.floor,
                            call: 1,
                        },
                        elev_idx: completed_order.elev_idx,
                    });
                }
            }
        }
    }

    // See if there are any orders on the way to selected order
    if order_found.0.is_some() {
        let elev_idx = completed_order.elev_idx;
        let order = find_closest_order(
            order_found.0.as_ref().unwrap().clone(),
            eligble_orders.clone(),
            completed_order,
        );
        orders.retain(|item| item != &order);
        order_found.0 = Some(order.clone());
        current_orders.insert(elev_idx, Some(order));
    }
    return (order_found.0, order_found.1);
}

// ---------- PURE FUNCTIONS ----------

/// Returns true if elevator `elev_idx` may take this order.
/// Cab orders (call == 2) may only be taken by the elevator that owns them (order.elev_idx).
fn elevator_may_take_order(elev_idx: usize, order: &Order) -> bool {
    order.cb.call != 2 || order.elev_idx == elev_idx
}

fn get_eligible_orders(orders: &VecDeque<Order>, completed_order: Order) -> Vec<Order> {
    let mut eligble_orders: Vec<Order> = Vec::new();

    // Find orders that the elevator that just completed an order may take
    for order in orders.iter() {
        if elevator_may_take_order(completed_order.elev_idx, order) {
            eligble_orders.push(order.clone());
        }
    }
    return eligble_orders;
}

// Designate busy and idle elevators, based on being able to take current order
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
    return (busy_elevs, idle_elevs);
}

// Rebuild the queue with cab orders at the front, add new order to the back
fn rebuild_queue(orders: &mut VecDeque<Order>, order: Order) -> VecDeque<Order> {
    let mut cab_orders: VecDeque<Order> = VecDeque::with_capacity(orders.len());
    let mut other_orders: VecDeque<Order> = VecDeque::with_capacity(orders.len());
    for order in orders.iter() {
        if order.cb.call == 2 {
            cab_orders.push_back(order.clone());
        } else {
            other_orders.push_back(order.clone());
        }
    }
    let mut new_orders: VecDeque<Order> = VecDeque::with_capacity(orders.len());
    new_orders.extend(cab_orders);
    new_orders.extend(other_orders);
    new_orders.push_back(order.clone());
    return new_orders;
}

// Change the elevators current order if any of the following conditions are met:
// 1. The recieved order is a hall order, on the way to the elevators current order
// 2. The recieved order is a cab order, on the way to the elevators current order AND the cab order is for elev_idx
fn order_on_the_way(elev_idx: usize, position: u8, curr_order: Order, new_order: Order) -> bool {
    let new_call = new_order.cb.call;
    let new_floor = new_order.cb.floor;
    let curr_call = curr_order.cb.call;
    let curr_floor = curr_order.cb.floor;

    let is_below = curr_floor <= new_floor && new_floor < position;
    let is_above = curr_floor >= new_floor && new_floor > position;

    let on_way_below = (new_call == 1 && curr_call != 0) || new_call == 2;
    let on_way_above = (new_call == 0 && curr_call != 1) || new_call == 2;

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
    print!("Closest distance: {}", closest_distance);

    for elev_idx in elev_candidates {
        if let Some(&position) = positions.get(&elev_idx) {
            let new_closest_distance = u8::abs_diff(position, order.cb.floor);
            if new_closest_distance < closest_distance {
                closest_distance = new_closest_distance;
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
    return None;
}

fn find_closest_order(order: Order, eligble_orders: Vec<Order>, completed_order: Order) -> Order {
    let mut closest_order: Order = order.clone();
    let mut closest_distance: u8 = M + 1;

    for eligble_order in eligble_orders.iter() {
        if order_on_the_way(
            completed_order.elev_idx,
            completed_order.cb.floor,
            order.clone(),
            eligble_order.clone(),
        ) {
            let new_closest_distance =
                u8::abs_diff(completed_order.cb.floor, eligble_order.cb.floor);
            if new_closest_distance < closest_distance {
                closest_distance = new_closest_distance;
                closest_order = eligble_order.clone();
            }
        }
    }
    println!("Closest order is {:?}", closest_order);
    return closest_order;
}

// Find the next cab order in the direction of the current order
fn find_cab_order(orders: Vec<Order>, floor: u8, dir: u8) -> Option<Order> {
    for order in orders.iter() {
        match dir {
            0 => {
                if (order.cb.floor > floor) && (order.cb.call == 2) {
                    return Some(order.clone());
                }
            }
            1 => {
                if (order.cb.floor < floor) && (order.cb.call == 2) {
                    return Some(order.clone());
                }
            }
            _ => (),
        }
    }
    return None;
}

fn should_change_direction(order: Option<Order>, floor: u8, dir: u8) -> Option<bool> {
    if let Some(order) = order {
        match dir {
            0 => {
                if order.cb.floor < floor {
                    return Some(true);
                }
            }
            1 => {
                if order.cb.floor > floor {
                    return Some(true);
                }
            }
            _ => (),
        }
        return Some(false);
    }
    return None;
}

// ---------- CONVERSION HELPERS ----------

fn hall_call_to_cb(hc: &HallCall) -> CallButton {
    CallButton {
        floor: hc.floor,
        call: match hc.call {
            Direction::Up => 0,
            Direction::Down => 1,
            Direction::Idle => 2,
        },
    }
}

fn cb_to_hall_call(cb: &CallButton) -> Option<HallCall> {
    let dir = match cb.call {
        0 => Direction::Up,
        1 => Direction::Down,
        _ => return None, // cab call, not a hall call
    };
    Some(HallCall {
        floor: cb.floor,
        call: dir,
    })
}
