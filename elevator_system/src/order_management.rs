use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx};
use std::collections::{VecDeque, HashMap};

use crate::elevator::elevio::poll::CallButton as CallButton;
use crate::elevator::NUM_ELEVATORS;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Order {
    pub cb: CallButton,
    pub elev_idx: usize,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Status {
    pub floor: u8,
    pub elev_idx: usize,
}

const m: u8 = 3; // number of floors
const n: u8 = NUM_ELEVATORS; // number of elevators

pub async fn order_management_runner(master: u8, mut order_request_rx: URx<Order>, order_assign_tx: UTx<Order>, mut update_status_rx: URx<Status>, mut order_complete_rx: URx<Order>, order_light_assign_tx: UTx<(Order, bool)>, mut master_notify_rx: URx<()>, mut master_position_rx: URx<u8>) -> std::io::Result<()> {
    
    println!("Waiting for master notification...");
    // Wait for master notification before starting
    master_notify_rx.recv().await.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "Master notification channel closed"))?;
    
    println!("Waiting for master position...");
    // Wait for master position before starting
    let master_floor = master_position_rx.recv().await.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "Master position channel closed"))?;
    
    println!("Starting order management");
    let mut orders: VecDeque<Order> = VecDeque::with_capacity(3*m as usize);       // Ring buffer of all orders
    let mut positions: HashMap<usize, u8> = HashMap::new();                        // Dictionary of current positions for each elevator
    let mut current_orders: HashMap<usize, Option<Order>> = HashMap::new();       // Dictionary of current order for each elevator
    let mut alive_elevs: Vec<usize> = Vec::new();

    // Start by adding the master elevator to the list of alive elevators and positions
    positions.insert(master as usize, master_floor);
    alive_elevs.push(master as usize);
    println!("Alive elevators: {:?}", alive_elevs);

    // TODO: Watchdog timer!

    // (re)assign orders whenever a new order is received or the status of an elevator changes
    loop {
        println!(""); println!("-");
        let busy_elevs: Vec<usize> = alive_elevs.iter().copied().filter(|&i| {
            current_orders.get(&i).and_then(|o| o.as_ref()).is_some()
        }).collect();
        let idle_elevs: Vec<usize> = alive_elevs.iter().copied().filter(|&i| !busy_elevs.contains(&i)).collect();
        println!("Orders: {:?}", orders);
        println!("Current elevators: {:?}", current_orders);
        println!("Busy elevators: {:?}", busy_elevs);
        println!("Idle elevators: {:?}", idle_elevs);
        
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
                // println!("Arrived at floor: {}", call.floor);


                // ---------- CLEAR ORDER ----------
                // Remove cab order to current floor and optionally the hall order the elevator is completing
                if order.cb.call != 2 {
                    orders.retain(|item| item != &order);
                    let _ = order_light_assign_tx.send((order.clone(), false));
                }
                orders.retain(|item| item.cb != order.cb);
                current_orders.insert(order.elev_idx, None);        
                // TODO: Turn off light
                let _ = order_light_assign_tx.send((order.clone(), false));
                println!("Cleared order {:?}. Orders: {:?}", order.cb, orders.clone());


                // ---------- FIND NEXT ORDER ----------
                if orders.len() > 0 {
                    let (next_order, clear_call) = assign_next_order(order.clone(), &mut orders, &mut current_orders);
                    if clear_call.is_some() {
                        orders.retain(|item| item != &clear_call.as_ref().unwrap().clone());
                        let _ = order_light_assign_tx.send((clear_call.unwrap().clone(), false));
                    }

                    if next_order.is_some() {
                        let next_order_val = next_order.unwrap();
                        
                        // ---------- ATTEMPT TO REORDER QUEUE ----------
                        // Try to assign the next order using assign_new_orders to see if any working elevator
                        // can take it on the way, otherwise assign it to the elevator that just completed an order
                        let new_order_found = assign_new_orders(next_order_val.clone(), &mut orders, &mut positions, &mut current_orders, &alive_elevs);
                        if let Some(order_elev_idx) = new_order_found {
                            let _ = order_assign_tx.send(Order { cb: next_order_val.cb.clone(), elev_idx: order_elev_idx});
                        }
                        else {
                            // assign_new_orders didn't assign it, so assign it to the elevator that just completed an order
                            let _ = order_assign_tx.send(Order { cb: next_order_val.cb.clone(), elev_idx: next_order_val.elev_idx});
                        }
                    }
                    else {println!("Failed to assign next order");}
                }
            }
            Some(status) = update_status_rx.recv() => {
                positions.insert(status.elev_idx, status.floor);
                // Collect all elevator indices that have positions (not just 0..n)
                alive_elevs = positions.keys().copied().collect();
            }
            else => {
                println!("All channels closed, exiting order management");
            }
        }
    }
    Ok(())
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

fn assign_new_orders(order: Order, orders: &mut VecDeque<Order>, positions: &HashMap<usize, u8>,
    mut current_orders: &mut HashMap<usize, Option<Order>>, alive_elevs: &Vec<usize>) -> Option<usize> {

    // Assign order to elevator if there is no current order OR assign order on the way to the current order    

    // Rebuild the queue with cab orders at the front
    let mut cab_orders: VecDeque<Order> = VecDeque::with_capacity(orders.len());
    let mut other_orders: VecDeque<Order> = VecDeque::with_capacity(orders.len());
    for order in orders.iter() {
        if order.cb.call == 2 {
            cab_orders.push_back(order.clone());
        } else {
            other_orders.push_back(order.clone());
        }
    }
    orders.clear();
    orders.extend(cab_orders);
    orders.extend(other_orders);

    // If the order already exists in the queue, return None
    if orders.iter().any(|o| o == &order) {
        return None;
    }
    // Add the new order to the queue
    orders.push_back(order.clone());

    // Designate busy and idle elevators
    let busy_elevs: Vec<usize> = alive_elevs.iter().copied().filter(|&i| {
        current_orders.get(&i).and_then(|o| o.as_ref()).is_some()
    }).collect();
    let idle_elevs: Vec<usize> = alive_elevs.iter().copied().filter(|&i| !busy_elevs.contains(&i)).collect();
    println!("Busy elevators: {:?}", busy_elevs);
    println!("Idle elevators: {:?}", idle_elevs);


    // Busy elevators - see if any elevator can take an order on the way
    for elev_idx in busy_elevs {
        if let Some(Some(curr_order)) = current_orders.get(&elev_idx).cloned() {
            if let Some(&position) = positions.get(&elev_idx) {
                if order_on_the_way(elev_idx, position, curr_order.clone(), order.clone()) {
                    println!("Order {:?} is on the way for elevator {} (at floor {}, going to {:?})", 
                             order.cb, elev_idx, position, curr_order.cb);
                    // Push demoted order to the front of the queue and remove the promoted order from the queue to avoid duplicates.
                    orders.push_front(curr_order);
                    orders.retain(|item| item != &order);
                    current_orders.insert(elev_idx, Some(order));
                    return Some(elev_idx);
                }
            }
        }
    }

    // Idle elevators - assign the order to the closest elevator (only those that may take this order)
    let idle_candidates: Vec<usize> = idle_elevs.iter().copied().filter(|&i| elevator_may_take_order(i, &order)).collect();

    let mut closest_elev: Option<usize> = None;
    let mut closest_distance: u8 = m;

    for elev_idx in idle_candidates {
        if let Some(&position) = positions.get(&elev_idx) {
            let new_closest_distance = u8::abs_diff(position, order.clone().cb.floor);
            if new_closest_distance < closest_distance {
                closest_distance = new_closest_distance;
                closest_elev = Some(elev_idx);
            }
        }
    }
    if let Some(elev_idx) = closest_elev {
        current_orders.insert(elev_idx, Some(order.clone()));
        orders.retain(|item| item != &order);
        return Some(elev_idx);
    }
    None

}

fn assign_next_order(completed_order: Order, orders: &mut VecDeque<Order>,
    mut current_orders: &mut HashMap<usize, Option<Order>>) -> (Option<Order>, Option<Order>) {

    let mut order_found: (Option<Order>, Option<Order>) = (None, None);
    let mut eligble_orders: Vec<Order> = Vec::new();

    // Find orders that the elevator that just completed an order may take
    // TODO: check that all these orders are eligible
    for order in orders.iter() {
        if elevator_may_take_order(completed_order.elev_idx, order) {
            eligble_orders.push(order.clone());
        }
    }

    match completed_order.cb.call {
        0 => 'HallUp: {
            // Try to assign order above in the same direction, else just assign something

            // Hall up or cab order call above
            for order in eligble_orders.iter() {
                if (order.cb.floor > completed_order.cb.floor) && (order.cb.call != 1){
                    order_found.0 = Some(order.clone());
                    break 'HallUp;
                }
            }
            // Hall down call above
            for order in eligble_orders.iter() {
                if order.cb.floor > completed_order.cb.floor && completed_order.cb.call == 1 {
                    order_found.0 = Some(order.clone());
                    break 'HallUp;
                }
            }

            // No order above, clear hall up order
            order_found.1 = Some(Order { cb: CallButton { floor: completed_order.cb.floor, call: 0 }, elev_idx: completed_order.elev_idx });
            // Changing direction, assign hall up order at the current floor (see spec)
            order_found.0 = Some(Order { cb: CallButton { floor: completed_order.cb.floor, call: 1 }, elev_idx: completed_order.elev_idx });
            println!("Changing direction");
        }
        1 => 'HallDown: {
            // Try to assign order below in the same direction, else just assign something

            // Hall down or cab order call below
            for order in eligble_orders.iter() {
                if (order.cb.floor < completed_order.cb.floor) && (order.cb.call != 0) {
                    order_found.0 = Some(order.clone());
                    break 'HallDown;
                }
            }
            // Hall up call below
            for order in eligble_orders.iter() {
                if order.cb.floor < completed_order.cb.floor && order.cb.call == 0 {
                    order_found.0 = Some(order.clone());
                    break 'HallDown;
                }
            }

            // No order below, clear hall down order
            order_found.1 = Some(Order { cb: CallButton { floor: completed_order.cb.floor, call: 1 }, elev_idx: completed_order.elev_idx });
            // Changing direction, assign hall down order at the current floor (see spec)
            order_found.0 = Some(Order { cb: CallButton { floor: completed_order.cb.floor, call: 0 }, elev_idx: completed_order.elev_idx });
            println!("Changing direction");
        }
        _ => 'Cab: {
            // Pick the first order, see if there are any hall orders at the current floor, in the direction of the first order
            
            order_found.0 = Some(orders.pop_front().unwrap());
            
            // If the order is above, clear hall up order
            if completed_order.cb.floor < order_found.0.as_ref().unwrap().cb.floor{
                order_found.1 = Some(Order { cb: CallButton{floor: completed_order.cb.floor, call: 0}, elev_idx: completed_order.elev_idx})}
            // If the order is below, clear hall down order
            else if completed_order.cb.floor > order_found.0.as_ref().unwrap().cb.floor{
                order_found.1 = Some(Order { cb: CallButton{floor: completed_order.cb.floor, call: 1}, elev_idx: completed_order.elev_idx})} // Hall down

            break 'Cab;
        }
    }

    // Store the next order for the elevator that completed the previous order
    current_orders.insert(completed_order.elev_idx, order_found.0.clone());
    orders.retain(|item| item != order_found.0.as_ref().unwrap());
    return (order_found.0, order_found.1);

}