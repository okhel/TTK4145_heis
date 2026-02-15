use std::{io, env};
use tokio::sync::mpsc::unbounded_channel as uc;
use order_management::{Order as Order, Status as Status};
use elevator::elevio::poll::CallButton as CallButton;

pub mod elevator;
pub mod order_management;
pub mod networking;
pub mod process;
// use elevator::{elevio::elev::Elevio as e, NUM_FLOORS};

#[tokio::main]

async fn main() -> io::Result<()> {

    let id: u8 = env::args().last().unwrap().parse().unwrap();
    let mut ids = vec![19, 20];

    ids.retain(|x| *x !=id);
    let remote_id = ids[0];
    println!("I'm {}", id);
    println!("Connecting to {}", remote_id);


    // Channels for Elevator <-> Network
    let (call_request_tx, call_request_rx) = uc::<CallButton>();
    let (call_assign_tx, call_assign_rx) = uc::<CallButton>();
    let (update_floor_tx, update_floor_rx) = uc::<u8>();
    let (call_complete_tx, call_complete_rx) = uc::<CallButton>();
    let (call_light_assign_tx, call_light_assign_rx) = uc::<(CallButton, bool)>();

    // Channels for Network <-> Order Management
    let (order_request_tx, order_request_rx) = uc::<Order>();
    let (order_assign_tx, order_assign_rx) = uc::<Order>();
    let (update_status_tx, update_status_rx) = uc::<Status>();
    let (order_complete_tx, order_complete_rx) = uc::<Order>();
    let (order_light_assign_tx, order_light_assign_rx) = uc::<(Order, bool)>();

    // Channels for Master Detection and Position
    let (master_notify_tx, master_notify_rx) = uc::<()>();
    let (master_position_tx, master_position_rx) = uc::<u8>();

    let order_management_task = tokio::spawn(async move {
        order_management::order_management_runner(id, order_request_rx, order_assign_tx, update_status_rx, order_complete_rx, order_light_assign_tx, master_notify_rx, master_position_rx).await});
    let elevator_runner_task = tokio::spawn(async move {
        elevator::elevator_runner(id, call_request_tx, call_assign_rx, update_floor_tx, call_complete_tx, call_light_assign_rx, master_position_tx).await });
    let network_runner_task = tokio::spawn(async move {
        networking::network_runner(id, remote_id, call_request_rx, call_assign_tx, update_floor_rx, call_complete_rx, call_light_assign_tx,
        order_request_tx, order_assign_rx, update_status_tx, order_complete_tx, order_light_assign_rx, master_notify_tx).await;  
    });

    let _ = tokio::join!(order_management_task, elevator_runner_task, network_runner_task);
    
    Ok(())
}
