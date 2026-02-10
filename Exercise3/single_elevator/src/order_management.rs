use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx};
use std::collections::VecDeque;

use crate::elevator::elevio::poll::CallButton as CallButton;

pub struct Order {
    pub call: CallButton,
    pub elevator: usize,
}

const m: u8 = 3; // number of floors
const n: u8 = 3; // number of elevators

pub async fn order_management_runner(mut floor_order_rx: URx<CallButton>, mut floor_msg_rx: URx<CallButton>, floor_cmd_tx: UTx<CallButton>, elev_req_tx: UTx<bool>, mut elev_resp_rx: URx<u8>, floor_msg_light_tx: UTx<(CallButton, bool)>) -> std::io::Result<()> {
    
    let mut orders: VecDeque<CallButton> = VecDeque::with_capacity(3*m as usize);       // Ring buffer of all orders
    let mut positions: Vec<Option<u8>> = vec![None; n as usize];                        // List of current positions for each elevator
    let mut current_orders: Vec<Option<CallButton>> = Vec::with_capacity(n as usize);   // List of current order for each elevator
    current_orders.resize_with(n as usize, || None);

    // (re)assign orders whenever a new order is received or the status of an elevator changes
    loop {
        // println!("Orders: {:?}", orders);
        tokio::select! { 
            Some(call) = URx::recv(&mut floor_order_rx) => {

                // TODO: Turn on light for new order
                let _ = floor_msg_light_tx.send((call.clone(), true));

                // ---------- REQUEST ELEVATOR POSITIONS ----------
                let _ = elev_req_tx.send(true);
                if let Some(floor) = elev_resp_rx.recv().await {
                    positions[0] = Some(floor);
                }


                // ---------- ASSIGN NEW ORDER ----------
                let new_order_found = assign_new_orders(call, &mut orders, &mut positions, &mut current_orders);
                if new_order_found {
                    let _ = floor_cmd_tx.send(current_orders[0].clone().unwrap());
                    // println!("Serving order: {:?}", current_orders[0].clone().unwrap());
                }
            }

            Some(call) = URx::recv(&mut floor_msg_rx) => {
                // println!("Arrived at floor: {}", call.floor);


                // ---------- CLEAR ORDER ----------
                // Remove cab order to current floor and optionally the hall order the elevator is completing
                if call.call != 2 {
                    orders.retain(|order| order != &call);
                    // TODO: Turn off light
                    let _ = floor_msg_light_tx.send((call.clone(), false));
                }
                orders.retain(|order| order != &CallButton { floor: call.floor, call: 2 });
                current_orders[0] = None;        
                // TODO: Turn off light
                let _ = floor_msg_light_tx.send((CallButton { floor: call.floor, call: 2 }, false));
                println!("Cleared order {:?}. Orders: {:?}", call, orders);


                // ---------- REQUEST ELEVATOR POSITIONS ----------
                let _ = elev_req_tx.send(true);
                if let Some(floor) = elev_resp_rx.recv().await {
                    positions[0] = Some(floor);
                }


                // ---------- FIND NEXT ORDER ----------
                if orders.len() > 0 {
                    let (next_order, clear_call) = assign_next_order(call.clone(), &mut orders, &mut current_orders);
                    if clear_call.is_some() {
                        orders.retain(|order| order != clear_call.as_ref().unwrap());
                        // TODO: Turn off light
                        let _ = floor_msg_light_tx.send((clear_call.unwrap().clone(), false));
                    }

                    if next_order.is_some() {

                        // ---------- REORDER QUEUE ----------
                        let _ = assign_new_orders(next_order.unwrap(), &mut orders, &mut positions, &mut current_orders);
                        println!("Serving order: {:?}", current_orders[0].clone().unwrap());
                        let _ = floor_cmd_tx.send(current_orders[0].clone().unwrap());
                    }
                    else {
                        // println!("No new order");
                        // TODO: Turn off light
                        // let _ = floor_msg_light_tx.send((call.clone(), false));
                    }
                }
            }
        }
    }
    Ok(())
}


// ---------- PURE FUNCTIONS ----------

fn assign_new_orders(call: CallButton, orders: &mut VecDeque<CallButton>, positions: &Vec<Option<u8>>,
    mut current_orders: &mut Vec<Option<CallButton>>) -> bool  {

    // Assign order to elevator if there is no current order OR assign order on the way to the current order    

    // Rebuild the queue with cab orders at the front
    let mut cab_orders: VecDeque<CallButton> = VecDeque::with_capacity(orders.len());
    let mut other_orders: VecDeque<CallButton> = VecDeque::with_capacity(orders.len());
    for order in orders.iter() {
        if order.call == 2 {
            cab_orders.push_back(order.clone());
        } else {
            other_orders.push_back(order.clone());
        }
    }
    orders.clear();
    orders.extend(cab_orders);
    orders.extend(other_orders);

    // If the order already exists, return
    if orders.iter().any(|order| order == &call) {
        return false;
    }
    else {
        orders.push_back(call.clone());
    }

    // If all elements in current_orders are None
    if current_orders.iter().all(|order| order.is_none()) {
        let mut closest_elev: usize = 0; // Default to first elevator
        let mut closest_distance: u8 = m;
        for i in 0..n as usize {
            if positions[i].is_some() {
                let new_closest_distance = u8::abs_diff(positions[i].unwrap(), call.clone().floor);
                if new_closest_distance < closest_distance {
                    closest_distance = new_closest_distance;
                    closest_elev = i;
                    break;

                }
            }
        }
        // println!("Assigning order to elevator: {}", closest_elev);
        current_orders[closest_elev] = Some(orders.pop_front().unwrap());
        return true;
    }

    // TODO: Do this for all elevators
    let curr_order = current_orders[0].as_ref().unwrap().clone();
    let mut replacement: CallButton = curr_order.clone();


    // TODO: Refactor to work for all elevators
    for call in orders.iter() {
        // If there is an order between the elevator and the destination, which is not a hall call in the opposite direction
        if (curr_order.floor < call.floor && call.floor < positions[0].unwrap()
                && call.call != 0 && curr_order.call != 0)
                || (curr_order.floor > call.floor && call.floor > positions[0].unwrap()
                && call.call != 1 && curr_order.call != 1) {
            replacement = CallButton { floor: call.floor, call: call.call };
            // println!("Order on the way, stopping");
        }
    }


    if replacement == *current_orders[0].as_ref().unwrap() {
        return false;
    }
    else {
        // Push demoted order to the front of the queue and remove the promoted order from the queue to avoid duplicates.
        orders.push_front(current_orders[0].take().unwrap());
        orders.retain(|order| order != &replacement);
        current_orders[0] = Some(replacement);
        return true;
    }

}

fn assign_next_order(call: CallButton, orders: &mut VecDeque<CallButton>,
    mut current_orders: &mut Vec<Option<CallButton>>) -> (Option<CallButton>, Option<CallButton>) {

    let mut order_found: (Option<CallButton>, Option<CallButton>) = (None, None);
    match call.call {
        0 => 'HallUp: {
            // Try to assign order above in the same direction, else just assign something

            // Hall up or cab order call above
            for order in orders.iter() {
                if order.floor > call.floor && order.call != 1 {
                    order_found = (Some(order.clone()), None);
                    break 'HallUp;
                }
            }
            // Hall down call above
            for order in orders.iter() {
                if order.floor > call.floor && order.call == 1 {
                    order_found = (Some(order.clone()), None);
                    break 'HallUp;
                }
            }

            // No order above, clear hall down order
            order_found.1 = Some(CallButton { floor: call.floor, call: 0 });
            // Changing direction, assign hall up order at the current floor (see spec)
            order_found.0 = Some(CallButton { floor: call.floor, call: 1 });
            println!("Changing direction");
        }
        1 => 'HallDown: {
            // Try to assign order below in the same direction, else just assign something

            // Hall down or cab order call below
            for order in orders.iter() {
                if order.floor < call.floor && order.call != 0 {
                    order_found = (Some(order.clone()), None);
                    break 'HallDown;
                }
            }
            // Hall up call below
            for order in orders.iter() {
                if order.floor < call.floor && order.call == 0 {
                    order_found = (Some(order.clone()), None);
                    break 'HallDown;
                }
            }

            // No order below, clear hall up order
            order_found.1 = Some(CallButton { floor: call.floor, call: 1 });
            // Changing direction, assign hall down order at the current floor (see spec)
            order_found.0 = Some(CallButton { floor: call.floor, call: 0 });
            println!("Changing direction");
        }
        _ => 'Cab: {
            // Pick the first order, see if there are any hall orders at the current floor, in the direction of the first order
            
            order_found.0 = Some(orders.pop_front().unwrap());
            if call.floor < order_found.0.as_ref().unwrap().floor{ order_found.1 = Some(CallButton { floor: call.floor, call: 0 })} // Hall up
            else if call.floor > order_found.0.as_ref().unwrap().floor{ order_found.1 = Some(CallButton { floor: call.floor, call: 1 })} // Hall down
            // TODO: Assign cab order below
            break 'Cab;
        }
    }

    current_orders[0] = order_found.0.clone();
    // println!("Next order: {:?}", current_orders[0]);
    return (order_found.0, order_found.1);

}