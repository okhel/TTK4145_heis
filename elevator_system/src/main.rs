use std::env;
use std::io;

use tokio::{sync::{mpsc::unbounded_channel as uc, broadcast as bc}};

use elevator::elevio::poll::CallButton;
use networking::types::Msg;

pub mod elevator;
pub mod master_slave;
pub mod networking;
pub mod order_management;

pub const USER: &str = "MAC"; // "MAC" or "LAB"

#[tokio::main]
async fn main() -> io::Result<()> {

    let local_id: u8 = env::args().last().unwrap().parse().unwrap();
    let mut ids = vec![19, 20, 21];

    ids.retain(|x| *x != local_id);
    let remote_ids = ids;
    let restart_remote_ids = remote_ids.clone();
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


    let (elevs_alive_tx, net_elevs_alive_rx) = bc::channel::<Vec<u8>>(2);
    let mgmt_elevs_alive_rx = elevs_alive_tx.subscribe();
    let restart_elevs_alive_rx = elevs_alive_tx.subscribe();

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
            net_elevs_alive_rx,
            ack_complete_tx,
        )
        .await;
    });

    let order_task = tokio::spawn(async move {
        order_management::order_manager(
            local_id,
            call_request_rx,
            call_assign_tx,
            update_floor_rx,
            call_complete_rx,
            call_light_assign_tx,
            network_inbox_tx,
            network_outbox_rx,
            ack_complete_rx,
            mgmt_elevs_alive_rx
        )
        .await
    });

    let master_slave_task = tokio::spawn(async move {
        master_slave::store_online_elevators(local_id, elevs_alive_tx, ping_rx).await;
    });
    let restart_task = tokio::spawn(async move {
        master_slave::restart_elevators(local_id, restart_remote_ids, restart_elevs_alive_rx).await;
    });

    let _ = tokio::join!(elevator_task, network_task, order_task, master_slave_task, restart_task);
    Ok(())
}
