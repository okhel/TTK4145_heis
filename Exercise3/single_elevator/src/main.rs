use std::{io, env};
use tokio::sync::mpsc::{unbounded_channel as uc};
use elevator::elevio::poll::CallButton as CallButton;
use order_management::Order as Order;

pub mod elevator;
pub mod order_management;
pub mod networking;


// use elevator::{elevio::elev::Elevio as e, NUM_FLOORS};

#[tokio::main]

async fn main() -> io::Result<()> {

    // let id: u8 = std::env::args().last().unwrap().parse().unwrap();
    // let mut ids = vec![19, 20, 21];

    // ids.retain(|x| *x !=id);
    // println!("Connecting to {}", ids[1]);

    // networking::udptest(ids[1]).await;

    // Create channels for module communication
    let (floor_order_tx, floor_order_rx) = uc::<Order>(); // Elevator sends order requests to order management
    let (floor_cmd_tx, floor_cmd_rx) = uc::<Order>(); // Order management sends commands to elevator
    let (floor_msg_tx, floor_msg_rx) = uc::<Order>(); // Elevator sends floor messages to order management
    let (elev_req_tx, elev_req_rx) = uc::<bool>(); // Order management sends requests to elevator
    let (elev_resp_tx, elev_resp_rx) = uc::<Vec<Option<u8>>>(); // Elevator sends responses to order management
    let (floor_msg_light_tx, floor_msg_light_rx) = uc::<(Order, bool)>(); // Elevator sends floor messages to light handling task

    let order_management_task = tokio::spawn(async move {
        order_management::order_management_runner(floor_order_rx, floor_msg_rx, floor_cmd_tx, elev_req_tx, elev_resp_rx, floor_msg_light_tx).await});
    let elevator_runner_task = tokio::spawn(async move {
        elevator::elevator_runner(floor_order_tx, floor_msg_tx, floor_cmd_rx, elev_req_rx, elev_resp_tx, floor_msg_light_rx).await });

    let _ = tokio::join!(order_management_task, elevator_runner_task);
    
    Ok(())
}
