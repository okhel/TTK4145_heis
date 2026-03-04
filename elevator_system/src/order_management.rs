use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx};
use std::collections::{VecDeque, HashMap};

use crate::elevator::elevio::poll::CallButton as CallButton;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Order {
    pub cb: CallButton,
    pub elev_idx: usize,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Status {
    pub floor: Option<u8>,
    pub elev_idx: usize,
}

const M: u8 = 3; // number of floors

pub async fn order_management_runner(mut order_request_rx: URx<Order>, order_assign_tx: UTx<Order>, mut update_status_rx: URx<Status>, mut order_complete_rx: URx<Order>, order_light_assign_tx: UTx<(Order, bool)>) {
        
    let mut orders: VecDeque<Order> = VecDeque::with_capacity(3*M as usize);        // Ring buffer of all orders
    let mut positions: HashMap<usize, u8> = HashMap::new();                         // Dictionary of current positions for each elevator
    let mut current_orders: HashMap<usize, Option<Order>> = HashMap::new();         // Dictionary of current order for each elevator
    let mut alive_elevs: Vec<usize> = Vec::new();

    // TODO: Watchdog timer!

    // (re)assign orders whenever a new order is received or the status of an elevator changes
    loop {
        println!(""); println!("-");
        println!("Orders: {:?}", orders);
        println!("Current orders: {:?}", current_orders);
        
        tokio::select! { 
            
            Some(order) = order_request_rx.recv() => {

                let _ = order_light_assign_tx.send((order.clone(), true));

                // ---------- ASSIGN NEW ORDER ----------
                let new_order_found = assign_new_orders(order.clone(), &mut orders, &mut positions, &mut current_orders, &alive_elevs);
                if let Some(order_elev_idx) = new_order_found {
                    let _ = order_assign_tx.send(Order { cb: order.cb.clone(), elev_idx: order_elev_idx});
                }
                else {println!("Could not assign new order");}
            }

            Some(order) = order_complete_rx.recv() => {

                // ---------- CLEAR ORDER ----------
                // Remove order the elevator is completing, if it is not a cab order
                if order.cb.call != 2 {
                    orders.retain(|item| item.cb != order.cb);
                    let _ = order_light_assign_tx.send((order.clone(), false));
                }
                // Remove cab order to current floor
                let cab_order = Order { cb: CallButton { floor: order.cb.floor, call: 2 }, elev_idx: order.elev_idx };
                orders.retain(|item| item != &cab_order);        
                let _ = order_light_assign_tx.send((cab_order.clone(), false));
                current_orders.insert(order.elev_idx, None);
                println!("Cleared order {:?}. ", order);


                // ---------- FIND NEXT ORDER ----------
                let elev_idx = order.elev_idx;
                let (next_order, clear_call) = assign_next_order(order.clone(), &mut orders, &mut current_orders);
                if next_order.is_some() {
                    let _ = order_assign_tx.send(Order { cb: next_order.as_ref().unwrap().cb.clone(), elev_idx: elev_idx });
                }
                else {
                    println!("Failed to assign next order");
                }
                if clear_call.is_some() {
                    orders.retain(|item| item.cb != clear_call.as_ref().unwrap().cb);
                    let _ = order_light_assign_tx.send((clear_call.unwrap().clone(), false));
                }
            }
            Some(status) = update_status_rx.recv() => {
                if status.floor.is_some() {
                    positions.insert(status.elev_idx, status.floor.unwrap());
                }
                else {
                    positions.remove(&status.elev_idx);
                    orders.push_front(current_orders[&status.elev_idx].as_ref().unwrap().clone());
                    current_orders.insert(status.elev_idx, None);
                }
                alive_elevs = positions.keys().copied().collect();

            }
            else => {
                println!("All channels closed, exiting order management");
            }
        }
    }
}


// Assign order to elevator if there is no current order OR assign order on the way to the current order    
fn assign_new_orders(order: Order, orders: &mut VecDeque<Order>, positions: &HashMap<usize, u8>,
    current_orders: &mut HashMap<usize, Option<Order>>, alive_elevs: &Vec<usize>) -> Option<usize> {

    // If the order already exists in the queue, return None
    if orders.iter().any(|o| o == &order) {
        return None;
    }

    *orders = rebuild_queue(orders, order.clone());
    let (busy_elevs, available_elevs) = designate_busy_idle(alive_elevs.clone(), current_orders, order.clone());


    // BUSY ELEVATORS
    // See if any elevator can take the order on the way
    if let Some((elev_idx, paused_order)) = find_order_otw(busy_elevs, &order, current_orders, positions){
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

fn assign_next_order(completed_order: Order, orders: &mut VecDeque<Order>,
    current_orders: &mut HashMap<usize, Option<Order>>) -> (Option<Order>, Option<Order>) {

    let mut order_found: (Option<Order>, Option<Order>) = (None, None);
    let eligble_orders = get_eligible_orders(orders, completed_order.clone());

    match completed_order.cb.call {
        0 => 'HallUp: {

            // Find closest cab order above
            if let Some(cab_order) = find_cab_order(eligble_orders.clone(), completed_order.cb.floor, 0) {
                order_found.0 = Some(cab_order);
                break 'HallUp;
            }

            // Pick first order in the queue
            match should_change_direction(eligble_orders.first().cloned(), completed_order.cb.floor, 0) {
                Some(true) => {
                    order_found.0 = Some(Order { cb: CallButton { floor: completed_order.cb.floor, call: 1 }, elev_idx: completed_order.elev_idx });
                }
                Some(false) => {
                    let order = eligble_orders.first().cloned().unwrap();
                    order_found.0 = Some(order);
                    order_found.1 = Some(Order { cb: CallButton { floor: completed_order.cb.floor, call: 0 }, elev_idx: completed_order.elev_idx });
                }
                None => ()
            };

        }
        1 => 'HallDown: {

            // Find closest cab order below
            if let Some(cab_order) = find_cab_order(eligble_orders.clone(), completed_order.cb.floor, 1) {
                order_found.0 = Some(cab_order);
                break 'HallDown;
            }
            
            // Pick first order in the queue
            match should_change_direction(eligble_orders.first().cloned(), completed_order.cb.floor, 1) {
                Some(true) => {
                    order_found.0 = Some(Order { cb: CallButton { floor: completed_order.cb.floor, call: 0 }, elev_idx: completed_order.elev_idx });
                }
                Some(false) => {
                    let order = eligble_orders.first().cloned().unwrap();
                    order_found.0 = Some(order);
                    order_found.1 = Some(Order { cb: CallButton { floor: completed_order.cb.floor, call: 1 }, elev_idx: completed_order.elev_idx });
                }
                None => ()
            };
        }
        _ => 'Cab: {
            // Pick first eligible order in the queue
            if let Some(order) = eligble_orders.first().cloned() {
                order_found.0 = Some(order);
                if order_found.0.as_ref().unwrap().cb.floor > completed_order.cb.floor {
                    order_found.1 = Some(Order { cb: CallButton { floor: completed_order.cb.floor, call: 0 }, elev_idx: completed_order.elev_idx });
                    break 'Cab;
                }
                if order_found.0.as_ref().unwrap().cb.floor < completed_order.cb.floor {
                    order_found.1 = Some(Order { cb: CallButton { floor: completed_order.cb.floor, call: 1 }, elev_idx: completed_order.elev_idx });
                }
            }
            
        }
    }

    // See if there are any orders on the way to selected order
    if order_found.0.is_some() {
        let elev_idx = completed_order.elev_idx;
        let order = find_closest_order(order_found.0.as_ref().unwrap().clone(), eligble_orders.clone(), completed_order);
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

fn find_closest_elev(elev_candidates: Vec<usize>, order: &Order, positions: &HashMap<usize, u8>) -> Option<usize> {
    let mut closest_elev: Option<usize> = None;
    let mut closest_distance: u8 = M+1;
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

    return closest_elev;
}

fn find_order_otw(busy_elevs: Vec<usize>, order: &Order, current_orders: &HashMap<usize, Option<Order>>, positions: &HashMap<usize, u8>) -> Option<(usize, Order)> {

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
    let mut closest_distance: u8 = M+1;

    for eligble_order in eligble_orders.iter() {
        if order_on_the_way(completed_order.elev_idx, completed_order.cb.floor, order.clone(), eligble_order.clone()) {
            let new_closest_distance = u8::abs_diff(completed_order.cb.floor, eligble_order.cb.floor);
            if new_closest_distance < closest_distance {
                closest_distance = new_closest_distance;
                closest_order = eligble_order.clone();
            }
        }
    }
    println!("Closest order is {:?}", closest_order);
    return closest_order;
}

// Designate busy and idle elevators, based on being able to take current order
fn designate_busy_idle(alive_elevs: Vec<usize>, current_orders: &HashMap<usize, Option<Order>>, order: Order) -> (Vec<usize>, Vec<usize>) {
    let busy_elevs: Vec<usize> = alive_elevs.iter().copied().filter(|&i| {
        current_orders.get(&i).and_then(|o| o.as_ref()).is_some()
    }).collect();
    let free_elevs: Vec<usize> = alive_elevs.iter().copied().filter(|&i| !busy_elevs.contains(&i)).collect();
    let idle_elevs: Vec<usize> = free_elevs.iter().copied().filter(|&i| elevator_may_take_order(i, &order)).collect();
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

// Find the next cab order in the direction of the current order
fn find_cab_order(orders: Vec<Order>, floor: u8, dir: u8) -> Option<Order> {
    for order in orders.iter() {
        match dir {
            0 => if (order.cb.floor > floor) && (order.cb.call == 2) {return Some(order.clone());}
            1 => if (order.cb.floor < floor) && (order.cb.call == 2) {return Some(order.clone());}
            _ => ()
        }
    }
    return None;
}


// fn find_cab_order(orders: Vec<Order>, floor: u8, dir: u8) -> Option<Order> {
//     let mut closest_order: Option<Order> = None;
//     let mut closest_distance: u8 = M+1;

//     for order in orders.iter() {
//         match dir {
//             0 => if (order.cb.floor > floor) && (order.cb.call == 2) {
//                 let new_closest_distance = u8::abs_diff(order.cb.floor, floor);
//                 if new_closest_distance < closest_distance {
//                     closest_distance = new_closest_distance;
//                     closest_order = Some(order.clone());
//                 }
//             }
//             1 => if (order.cb.floor < floor) && (order.cb.call == 2) {
//                 let new_closest_distance = u8::abs_diff(order.cb.floor, floor);
//                 if new_closest_distance < closest_distance {
//                     closest_distance = new_closest_distance;
//                     closest_order = Some(order.clone());
//                 }
//             }
//             _ => ()
//         }
//     }
//     return closest_order;

fn should_change_direction(order: Option<Order>, floor: u8, dir: u8) -> Option<bool> {
    if let Some(order) = order {
        match dir {
            0 => if order.cb.floor < floor {return Some(true);}
            1 => if order.cb.floor > floor {return Some(true);}
            _ => ()
        }
        return Some(false);
    }
    return None;

}