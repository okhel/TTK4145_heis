use std::env;
use std::{io, panic};

use tokio::sync::{mpsc::unbounded_channel as uc, broadcast as bc};

use crate::types::{ElevatorCommand, ElevatorEvent, ElevatorHandle, ElevatorInternal, NetworkEvent, NetworkHandle, NetworkInternal};

pub mod elevator;
pub mod master_slave;
pub mod networking;
pub mod order_management;
pub mod types;
pub mod watchdog;

pub const USER: &str = "LAB"; // "MAC" or "LAB"

fn on_panic(_local_id: u8) {
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

    // discovery channels 
    let (ping_tx, ping_rx) = uc::<u8>();
    let (alive_tx, net_alive_rx) = bc::channel::<Vec<u8>>(16);
    let mgmt_alive_rx = alive_tx.subscribe();

    // channels for internal and external elevator communication
    let (event_tx, event_rx) = uc::<ElevatorEvent>();
    let (cmd_tx, cmd_rx) = uc::<ElevatorCommand>();
    let elev_internal = ElevatorInternal { event_tx, cmd_rx };
    let elev_handle = ElevatorHandle { event_rx, cmd_tx };

    // channels for internal and external network communication
    let (net_send_tx, net_inbox) = uc::<networking::types::Msg>();
    let (net_event_tx, net_event_rx) = uc::<NetworkEvent>();
    let net_internal = NetworkInternal { inbox: net_inbox, event_tx: net_event_tx };
    let net_handle = NetworkHandle { send_tx: net_send_tx, event_rx: net_event_rx };

    let elevator_task = tokio::spawn(async move {
        elevator::elevator_runner(local_id, elev_internal).await
    });

    let network_task = tokio::spawn(async move {
        networking::network_runner(local_id, remote_ids, net_internal, ping_tx, net_alive_rx).await;
    });

    let order_task = tokio::spawn(async move {
        order_management::order_manager(local_id, elev_handle, net_handle, mgmt_alive_rx).await
    });

    let master_slave_task = tokio::spawn(async move {
        master_slave::store_online_elevators(local_id, alive_tx, ping_rx).await;
    });

    let _ = tokio::join!(elevator_task, network_task, order_task, master_slave_task);
    Ok(())
}
