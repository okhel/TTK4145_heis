use std::env;
use std::io;

use tokio::sync::mpsc::unbounded_channel as uc;

use elevator::elevio::poll::CallButton;
use networking::types::Msg;

pub mod elevator;
pub mod master_slave;
pub mod networking;
pub mod order_management;
pub mod process;

#[tokio::main]
async fn main() -> io::Result<()> {
    if process::is_backup() {
        process::run_as_backup();
        return Ok(());
    }

    let local_id: u8 = env::args().last().unwrap().parse().unwrap();
    let mut ids = vec![19, 20, 21];

    ids.retain(|x| *x != local_id);
    let remote_ids = ids;
    println!("I'm {}", local_id);

    // Elevator channels (CallButton-based)
    let (call_request_tx, call_request_rx) = uc::<CallButton>();
    let (call_assign_tx, call_assign_rx) = uc::<CallButton>();
    let (update_floor_tx, update_floor_rx) = uc::<u8>();
    let (call_complete_tx, call_complete_rx) = uc::<CallButton>();
    let (call_light_assign_tx, call_light_assign_rx) = uc::<(CallButton, bool)>();

    // Network channels
    let (network_inbox_tx, network_inbox_rx) = uc::<Msg>();
    let (network_outbox_tx, network_outbox_rx) = uc::<Msg>();
    let (ping_tx, ping_rx) = uc::<u8>();
    let (ack_complete_tx, ack_complete_rx) = uc::<(u32, Msg)>();
    let (elevs_alive_tx, elevs_alive_rx) = uc::<Vec<u8>>();

    let elevator_task = tokio::spawn(async move {
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

    let network_task = tokio::spawn(async move {
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

    let order_task = tokio::spawn(async move {
        order_management::order_management_runner(
            local_id,
            call_request_rx,
            call_assign_tx,
            update_floor_rx,
            call_complete_rx,
            call_light_assign_tx,
            network_inbox_tx,
            network_outbox_rx,
            ack_complete_rx,
        )
        .await
    });

    let master_slave_task = tokio::spawn(async move {
        master_slave::master_check(local_id, ping_rx, elevs_alive_tx).await;
    });

    let _ = tokio::join!(elevator_task, network_task, order_task, master_slave_task);
    Ok(())
}
