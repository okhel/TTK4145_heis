use std::env;
use std::{io, panic};

use tokio::sync::{mpsc::unbounded_channel as uc, broadcast as bc};

use elevator::elevio::poll::CallButton;
use networking::types::Msg;
use types::Position;

pub mod elevator;
pub mod master_slave;
pub mod networking;
pub mod order_management;
pub mod types;
pub mod watchdog;

pub const USER: &str = "LAB"; // "MAC" or "LAB"

fn on_panic(local_id: u8) {
    std::process::abort();
}

#[tokio::main]
async fn main() -> io::Result<()> {

    let local_id: u8 = env::args().last().unwrap().parse().unwrap();
    let mut ids = vec![19, 20, 21];

    panic::set_hook(Box::new(move |_info| {
        on_panic(local_id)
      }));

    ids.retain(|x| *x != local_id);
    let remote_ids = ids;

    // Elevator <-> Order Manager channels
    let (call_request_tx, call_request_rx) = uc::<CallButton>();
    let (call_assign_tx, call_assign_rx) = uc::<CallButton>();
    let (update_state_tx, update_state_rx) = uc::<Position>();
    let (call_complete_tx, call_complete_rx) = uc::<CallButton>();
    let (call_light_tx, call_light_rx) = uc::<(CallButton, bool)>();
    let init_call_light_tx = call_light_tx.clone();

    // Order Manager <-> Network channels
    let (network_inbox_tx, network_inbox_rx) = uc::<Msg>();
    let (network_outbox_tx, network_outbox_rx) = uc::<Msg>();
    let (ack_complete_tx, ack_complete_rx) = uc::<(u32, Msg)>();

    // Discovery channels
    let (ping_tx, ping_rx) = uc::<u8>();
    let (elevs_alive_tx, net_elevs_alive_rx) = bc::channel::<Vec<u8>>(16);
    let mgmt_elevs_alive_rx = elevs_alive_tx.subscribe();

    let elevator_task = tokio::spawn(async move {
        elevator::elevator_runner(
            local_id,
            elevator::ElevatorChannels {
                call_request_tx,
                call_assign_rx,
                update_state_tx,
                call_complete_tx,
                call_light_rx,
                call_light_tx: init_call_light_tx,
            },
        )
        .await
    });

    let network_task = tokio::spawn(async move {
        networking::network_runner(
            local_id,
            remote_ids,
            networking::NetworkChannels {
                inbox: network_inbox_rx,
                outbox: network_outbox_tx,
                ping_tx,
                alive_rx: net_elevs_alive_rx,
                ack_complete_tx,
            },
        )
        .await;
    });

    let order_task = tokio::spawn(async move {
        order_management::order_manager(
            local_id,
            order_management::OrderManagerChannels {
                call_request_rx,
                call_assign_tx,
                update_state_rx,
                call_complete_rx,
                call_light_tx,
                net_send_tx: network_inbox_tx,
                net_recv_rx: network_outbox_rx,
                ack_complete_rx,
                elevs_alive_rx: mgmt_elevs_alive_rx,
            },
        )
        .await
    });

    let master_slave_task = tokio::spawn(async move {
        master_slave::store_online_elevators(local_id, elevs_alive_tx, ping_rx).await;
    });

    let _ = tokio::join!(elevator_task, network_task, order_task, master_slave_task);
    Ok(())
}
