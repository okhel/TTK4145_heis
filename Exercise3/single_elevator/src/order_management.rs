use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx};
use std::collections::VecDeque;

use crate::elevator::elevio::poll::CallButton as CallButton;


pub async fn order_management_runner(mut floor_order_rx: URx<CallButton>, mut floor_msg_rx: URx<CallButton>, floor_cmd_tx: UTx<CallButton>, elev_req_tx: UTx<bool>, mut elev_resp_rx: URx<u8>) -> std::io::Result<()> {
    
    let m: u8 = 3; // number of floors
    let n: u8 = 3; // number of elevators
    let mut orders: VecDeque<CallButton> = VecDeque::with_capacity(3*m as usize);       // Ring buffer of all orders
    let mut positions: Vec<Option<u8>> = vec![None; n as usize];                        // List of current positions for each elevator
    let mut current_orders: Vec<Option<CallButton>> = Vec::with_capacity(n as usize);   // List of current order for each elevator
    current_orders.resize_with(n as usize, || None);

    // (re)assign orders whenever a new order is received or the status of an elevator changes
    loop {
        println!("Orders: {:?}", orders);
        tokio::select! { 
            Some(call) = URx::recv(&mut floor_order_rx) => {

                // ---------- REQUEST ELEVATOR POSITIONS ----------
                let _ = elev_req_tx.send(true);
                if let Some(floor) = elev_resp_rx.recv().await {
                    positions[0] = Some(floor);
                }


                // ---------- ASSIGN NEW ORDER ----------
                let new_order_found = assign_new_orders(call, &mut orders, &mut positions, &mut current_orders);
                if new_order_found {
                    let _ = floor_cmd_tx.send(current_orders[0].clone().unwrap());
                    println!("Serving order: {:?}", current_orders[0].clone().unwrap());
                }
                

            }

            Some(call) = URx::recv(&mut floor_msg_rx) => {
                println!("Arrived at floor: {}", call.floor);


                // ---------- CLEAR ORDER ----------
                // Remove cab order to current floor and optionally the hall order the elevator is completing
                if call.call != 2 {
                    orders.retain(|order| order != &call);
                    // TODO: Turn off light
                }
                orders.retain(|order| order != &CallButton { floor: call.floor, call: 2 });
                current_orders[0] = None;
                
                
                // TODO: Turn off light
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
                    }

                    if next_order.is_some() {

                        // ---------- REORDER QUEUE ----------
                        let _ = assign_new_orders(next_order.unwrap(), &mut orders, &mut positions, &mut current_orders);
                        println!("Serving order: {:?}", current_orders[0].clone().unwrap());
                        let _ = floor_cmd_tx.send(current_orders[0].clone().unwrap());
                    }
                    else {
                        println!("No new order");
                        // TODO: Turn off light
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

    // If there are orders below the hall order
    

    // TODO: Move cab orders to the front of the queue.
    // Rebuild the queue to avoid mutating while iterating.
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
        orders.push_back(call);
    }
    
    // TODO: If there are no assigned orders, just assign the first order to the closest elevator
    if current_orders[0].is_none() {
        current_orders[0] = Some(orders.pop_front().unwrap());
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
            println!("Order on the way, stopping");
        }
    }
    if replacement == *current_orders[0].as_ref().unwrap() {
        return false;
    }
    else {
        // Remove the promoted order from the queue to avoid duplicates.
        orders.retain(|order| order != &replacement);
        orders.push_front(current_orders[0].take().unwrap());
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

            println!("No orders above, sending pseudo hall down order");
            // No orders above, clear hall order in down direction, assign pseudo hall down order at current floor (see spec)
            order_found.0 = Some(CallButton { floor: call.floor, call: 1 });
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

            // No orders below, clear hall order in up direction, assign pseudo hall up order at current floor (see spec)
            order_found.0 = Some(CallButton { floor: call.floor, call: 0 });

            // for order in orders.iter() {
            //     if order.floor > call.floor && order.call != 1 {
            //         order_found = (order.clone(), Some(CallButton { floor: call.floor, call: 0 }));
            //         break 'HallDown;
            //     }
            // }
        }
        _ => 'Cab: {
            // Pick the first order, see if there are any hall orders at the current floor, in the direction of the first order
            order_found.0 = Some(orders.pop_front().unwrap());
            if call.floor < order_found.0.as_ref().unwrap().floor{ order_found.1 = Some(CallButton { floor: call.floor, call: 0 })} // Hall up
            else if call.floor > order_found.0.as_ref().unwrap().floor{ order_found.1 = Some(CallButton { floor: call.floor, call: 1 })} // Hall down
            break 'Cab;
        }
    }

    // If there are no orders
    if orders.len() == 0 {
        order_found.0 = None;
        order_found.1 = Some(call.clone());
    }

    current_orders[0] = order_found.0.clone();
    println!("Next order: {:?}", current_orders[0]);
    (order_found.0, order_found.1)
}