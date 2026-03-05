use elevator::elevio::poll::CallButton;
use networking::types::Msg;
use order_management::{Order, Status};
use std::{env, io};
use tokio::sync::mpsc::unbounded_channel as uc;

pub mod elevator;
pub mod networking;
pub mod order_management;
pub mod process;

#[tokio::main]

async fn main() -> io::Result<()> {
    let local_id: u8 = env::args().last().unwrap().parse().unwrap();
    let mut ids = vec![19, 20, 21];

    ids.retain(|x| *x != local_id);
    let remote_ids = ids;
    println!("I'm {}", local_id);

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

    let (network_inbox_tx, network_inbox_rx) = uc::<Msg>();
    let (network_outbox_tx, network_outbox_rx) = uc::<Msg>();
    let (ping_tx, ping_rx) = uc::<u8>();
    let (ack_complete_tx, ack_complete_rx) = uc::<(u32, Msg)>();

    // Channels for Master Detection and Position
    let (elevs_alive_tx, elevs_alive_rx) = uc::<Vec<u8>>();
    let (master_notify_tx, master_notify_rx) = uc::<Vec<u8>>();

    let order_management_task = tokio::spawn(async move {
        order_management::order_management_runner(
            order_request_rx,
            order_assign_tx,
            update_status_rx,
            order_complete_rx,
            order_light_assign_tx,
        )
        .await
    });
    let elevator_runner_task = tokio::spawn(async move {
        elevator::elevator_runner(
            local_id,
            call_request_tx,
            call_assign_rx,
            update_floor_tx,
            call_complete_tx,
            call_light_assign_rx,
        )
        .await
    });

    let network_runner_task = tokio::spawn(async move {
        networking::network_runner(
            local_id,
            remote_ids,
            network_inbox_rx,
            network_outbox_tx,
            ping_tx,
            elevs_alive_rx,
            ack_complete_tx,
        )
        .await;
    });

    let _ = tokio::join!(
        order_management_task,
        elevator_runner_task,
        network_runner_task
    );

    Ok(())
}
