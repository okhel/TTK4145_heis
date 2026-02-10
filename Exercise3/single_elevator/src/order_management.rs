use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx};
use std::collections::VecDeque;

use crate::elevator::elevio::poll::CallButton as CallButton;
use crate::elevator::NUM_ELEVATORS;

#[derive(Clone, Debug, PartialEq)]
pub struct Order {
    pub cb: CallButton,
    pub elev_idx: usize,
}

const m: u8 = 3; // number of floors
const n: u8 = NUM_ELEVATORS; // number of elevators

pub async fn order_management_runner(mut floor_order_rx: URx<Order>, mut floor_msg_rx: URx<Order>,
    floor_cmd_tx: UTx<Order>, elev_req_tx: UTx<bool>, mut elev_resp_rx: URx<Vec<Option<u8>>>,
    floor_msg_light_tx: UTx<(Order, bool)>) -> std::io::Result<()> {
    
    let mut orders: VecDeque<Order> = VecDeque::with_capacity(3*m as usize);       // Ring buffer of all orders
    let mut positions: Vec<Option<u8>> = vec![None; n as usize];                        // List of current positions for each elevator
    let mut current_orders: Vec<Option<Order>> = Vec::with_capacity(n as usize);   // List of current order for each elevator
    current_orders.resize_with(n as usize, || None);

    // TODO: Watchdog timer!

    // (re)assign orders whenever a new order is received or the status of an elevator changes
    loop {
        // println!("Orders: {:?}", orders);
        tokio::select! { 
            Some(order) = URx::recv(&mut floor_order_rx) => {

                // TODO: Turn on light for new order
                let _ = floor_msg_light_tx.send((order.clone(), true));

                // ---------- REQUEST ELEVATOR POSITIONS ----------
                let _ = elev_req_tx.send(true);
                if let Some(pos) = elev_resp_rx.recv().await {
                    positions = pos;
                }
                let alive_elevs: Vec<usize> = (0..n as usize).filter(|i| positions[*i].is_some()).collect();


                // ---------- ASSIGN NEW ORDER ----------
                let new_order_found = assign_new_orders(order.clone(), &mut orders, &mut positions, &mut current_orders, &alive_elevs);
                if let Some(order_elev_idx) = new_order_found {
                    let _ = floor_cmd_tx.send(Order { cb: order.cb.clone(), elev_idx: order_elev_idx});
                }
                else {println!("Could not assign new order");}
            }

            Some(order) = URx::recv(&mut floor_msg_rx) => {
                // println!("Arrived at floor: {}", call.floor);


                // ---------- CLEAR ORDER ----------
                // Remove cab order to current floor and optionally the hall order the elevator is completing
                if order.cb.call != 2 {
                    orders.retain(|item| item != &order);
                    let _ = floor_msg_light_tx.send((order.clone(), false));
                }
                orders.retain(|item| item != &order);
                current_orders[order.elev_idx] = None;        
                // TODO: Turn off light
                let _ = floor_msg_light_tx.send((order.clone(), false));
                println!("Cleared order {:?}. Orders: {:?}", order.cb, orders.clone());


                // ---------- REQUEST ELEVATOR POSITIONS ----------
                let _ = elev_req_tx.send(true);
                if let Some(pos) = elev_resp_rx.recv().await {
                    positions = pos;
                }
                let alive_elevs: Vec<usize> = (0..n as usize).filter(|i| positions[*i].is_some()).collect();

                // ---------- FIND NEXT ORDER ----------
                if orders.len() > 0 {
                    let (next_order, clear_call) = assign_next_order(order.clone(), &mut orders, &mut current_orders);
                    if clear_call.is_some() {
                        orders.retain(|item| item != &clear_call.as_ref().unwrap().clone());
                        let _ = floor_msg_light_tx.send((clear_call.unwrap().clone(), false));
                    }

                    if next_order.is_some() {

                        // ---------- ATTEMPT TO REORDER QUEUE ----------
                        let new_order_found = assign_new_orders(order.clone(), &mut orders, &mut positions, &mut current_orders, &alive_elevs);
                        if let Some(order_elev_idx) = new_order_found {
                            let _ = floor_cmd_tx.send(Order { cb: order.cb.clone(), elev_idx: order_elev_idx});
                        }
                        else {
                            let _ = floor_cmd_tx.send(Order { cb: next_order.clone().unwrap().cb.clone(), elev_idx: next_order.unwrap().elev_idx});
                        }
                    }
                    else {println!("Failed to assign next order");}
                }
            }
        }
    }
    Ok(())
}


// ---------- PURE FUNCTIONS ----------

fn order_on_the_way(elev_idx: usize, position: u8, curr_order: Order, new_order: Order) -> bool {

    // Change the elevators current order if any of the following conditions are met:
    // 1. The recieved order is a hall order, on the way to the elevators current order
    // 2. The recieved order is a cab order, on the way to the elevators current order AND the cab order is for elev_idx

    // Extract information about the new and old order
    let new_call = new_order.cb.call;
    let new_elev_idx = new_order.elev_idx;
    let new_floor = new_order.cb.floor;
    let curr_call = curr_order.cb.call;
    let curr_floor = curr_order.cb.floor;
    
    let is_below = curr_floor <= new_floor && new_floor < position;
    let is_above = curr_floor >= new_floor && new_floor > position;
    
    (is_below && ((new_call == 1 && curr_call != 0) || (new_call == 2 && elev_idx == new_elev_idx)))
    || (is_above && ((new_call == 0 && curr_call != 1) || (new_call == 2 && elev_idx == new_elev_idx)))
}

fn assign_new_orders(order: Order, orders: &mut VecDeque<Order>, positions: &Vec<Option<u8>>,
    mut current_orders: &mut Vec<Option<Order>>, alive_elevs: &Vec<usize>) -> Option<usize> {

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

    // If the order already exists, return
    if orders.iter().any(|order| order == order) {
        return None;
    }
    else {
        orders.push_back(order.clone());
    }

    // Designate working and free elevators
    let working_elevs: Vec<usize> = alive_elevs.iter().copied().filter(|&i| current_orders[i].is_some()).collect();
    let free_elevs: Vec<usize> = alive_elevs.iter().copied().filter(|&i| !working_elevs.contains(&i)).collect();


    // Working elevators - see if any elevator can take an order on the way
    for elev_idx in working_elevs {
        let curr_order = current_orders[elev_idx].clone().unwrap();

        if order_on_the_way(elev_idx, positions[elev_idx].unwrap(), curr_order, order.clone()) {

            // Push demoted order to the front of the queue and remove the promoted order from the queue to avoid duplicates.
            orders.push_front(current_orders[elev_idx].take().unwrap());
            orders.retain(|item| item != &order);
            current_orders[elev_idx] = Some(order);
            return Some(elev_idx);
        }
    }

    // Free elevators - assign the order to the closest elevator
    let mut closest_elev: Option<usize> = None;
    let mut closest_distance: u8 = m;
    for elev_idx in free_elevs {
        // alive_elevs already filters for positions that are Some, so unwrap is safe
        let new_closest_distance = u8::abs_diff(positions[elev_idx].unwrap(), order.clone().cb.floor);
        if new_closest_distance < closest_distance {
            closest_distance = new_closest_distance;
            closest_elev = Some(elev_idx);
        }
    }
    if closest_elev.is_some() {
        // println!("Assigning order to elevator: {}", closest_elev);
        current_orders[closest_elev.unwrap()] = Some(order.clone());
        return Some(closest_elev.unwrap());
    }
    else {
        return None;
    }

}

fn assign_next_order(order: Order, orders: &mut VecDeque<Order>,
    mut current_orders: &mut Vec<Option<Order>>) -> (Option<Order>, Option<Order>) {

    // REMEMBER: "order" is the completed order, the "order.elev_idx" is the elevator that has completed the order
    let mut order_found: (Option<Order>, Option<Order>) = (None, None);
    let mut eligble_orders: Vec<Order> = Vec::new();

    // Find order to assign by iterating through the queue, picking the first that is not a cab order for a different elevator
    for item in orders.iter() {
        if item.cb.call != 2 || item.elev_idx == order.elev_idx {
            eligble_orders.push(item.clone());
        }

    }

    match order.cb.call {
        0 => 'HallUp: {
            // Try to assign order above in the same direction, else just assign something

            // Hall up or cab order call above
            for item in eligble_orders.iter() {
                if (item.cb.floor > order.cb.floor) && (item.cb.call != 1){
                    order_found.0 = Some(item.clone());
                    break 'HallUp;
                }
            }
            // Hall down call above
            for item in eligble_orders.iter() {
                if item.cb.floor > order.cb.floor && order.cb.call == 1 {
                    order_found.0 = Some(item.clone());
                    break 'HallUp;
                }
            }

            // No order above, clear hall up order
            order_found.1 = Some(Order { cb: CallButton { floor: order.cb.floor, call: 0 }, elev_idx: order.elev_idx });
            // Changing direction, assign hall up order at the current floor (see spec)
            order_found.0 = Some(Order { cb: CallButton { floor: order.cb.floor, call: 1 }, elev_idx: order.elev_idx });
            println!("Changing direction");
        }
        1 => 'HallDown: {
            // Try to assign order below in the same direction, else just assign something

            // Hall down or cab order call below
            for item in eligble_orders.iter() {
                if (item.cb.floor < order.cb.floor) && (item.cb.call != 0) {
                    order_found.0 = Some(item.clone());
                    break 'HallDown;
                }
            }
            // Hall up call below
            for item in eligble_orders.iter() {
                if item.cb.floor < order.cb.floor && item.cb.call == 0 {
                    order_found.0 = Some(item.clone());
                    break 'HallDown;
                }
            }

            // No order below, clear hall down order
            order_found.1 = Some(Order { cb: CallButton { floor: order.cb.floor, call: 1 }, elev_idx: order.elev_idx });
            // Changing direction, assign hall down order at the current floor (see spec)
            order_found.0 = Some(Order { cb: CallButton { floor: order.cb.floor, call: 0 }, elev_idx: order.elev_idx });
            println!("Changing direction");
        }
        _ => 'Cab: {
            // Pick the first order, see if there are any hall orders at the current floor, in the direction of the first order
            
            order_found.0 = Some(orders.pop_front().unwrap());
            
            // If the order is above, clear hall up order
            if order.cb.floor < order_found.0.as_ref().unwrap().cb.floor{
                order_found.1 = Some(Order { cb: CallButton{floor: order.cb.floor, call: 0}, elev_idx: order.elev_idx})}
            // If the order is below, clear hall down order
            else if order.cb.floor > order_found.0.as_ref().unwrap().cb.floor{
                order_found.1 = Some(Order { cb: CallButton{floor: order.cb.floor, call: 1}, elev_idx: order.elev_idx})} // Hall down

            break 'Cab;
        }
    }

    current_orders[0] = order_found.0.clone();
    return (order_found.0, order_found.1);

}