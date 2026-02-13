use std::{io, env};
use serde::Serialize;
use tokio::sync::mpsc::unbounded_channel as uc;
use elevator::elevio::poll::CallButton as CallButton;
use order_management::Order as Order;

pub mod elevator;
pub mod order_management;
pub mod networking;
pub mod process;
// use elevator::{elevio::elev::Elevio as e, NUM_FLOORS};

#[tokio::main]

async fn main() -> io::Result<()> {

    let id: u8 = std::env::args().last().unwrap().parse().unwrap();
    let mut ids = vec![19, 20, 21];

    ids.retain(|x| *x !=id);
    println!("I'm {}", id);
    println!("Connecting to {}", ids[0]);


    // Create channels for module communication
    let (floor_order_tx, floor_order_rx) = uc::<CallButton>(); // Elevator sends order requests to order management
    let (floor_cmd_tx, floor_cmd_rx) = uc::<CallButton>(); // Order management sends commands to elevator
    let (floor_msg_tx, floor_msg_rx) = uc::<CallButton>(); // Elevator sends floor messages to order management
    let (elev_req_tx, elev_req_rx) = uc::<bool>(); // Order management sends requests to elevator
    let (elev_resp_tx, elev_resp_rx) = uc::<u8>(); // Elevator sends responses to order management
    let (floor_msg_light_tx, floor_msg_light_rx) = uc::<(Order, bool)>(); // Elevator sends floor messages to light handling task
    let (at_floor_tx, at_floor_rx) = uc::<u8>();
    let (udp_received_tx, udp_received_rx) = uc::<u8>();

    let order_management_task = tokio::spawn(async move {
        order_management::order_management_runner(floor_order_rx, floor_msg_rx, floor_cmd_tx, elev_req_tx, elev_resp_rx, floor_msg_light_tx).await});
    let elevator_runner_task = tokio::spawn(async move {
        elevator::elevator_runner(id, floor_order_tx, floor_msg_tx, floor_cmd_rx, elev_req_rx, elev_resp_tx, floor_msg_light_rx, at_floor_tx).await });
    let network_runner_task = tokio::spawn(async move {
        networking::network_runner(at_floor_rx, id, ids).await;
    });

    let _ = tokio::join!(order_management_task, elevator_runner_task, network_runner_task);
    
    Ok(())
}
