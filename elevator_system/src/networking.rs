pub mod types;
pub mod transport;
pub mod master;
pub mod slave;

pub use types::*;

use tokio::{net::TcpStream, time::{self, Duration}};
use tokio::sync::mpsc::unbounded_channel as uc;

pub async fn setup(my_id: u8) -> NetworkHandle {
    let role = elect_role(my_id).await;
    println!("[networking] id={} role={:?}", my_id, role);

    let (hall_call_done_tx, hall_call_done_rx) = uc::<HallCall>();
    let (new_hall_call_tx,  new_hall_call_rx)  = uc::<HallCall>();
    let (state_update_tx,   state_update_rx)   = uc::<ElevatorState>();
    let (assigned_hall_call_tx, assigned_hall_call_rx) = uc::<HallCall>();
    let (world_state_tx,    world_state_rx)    = uc::<Vec<(HallCall, u8)>>();

    match &role {
        Role::Master => {
            tokio::spawn(master::run_master(
                my_id,
                hall_call_done_rx,
                new_hall_call_rx,
                state_update_rx,
                assigned_hall_call_tx,
                world_state_tx,
            ));
        }
        Role::Slave { master_id } => {
            let master_id = *master_id;
            tokio::spawn(slave::run_slave(
                my_id,
                master_id,
                hall_call_done_rx,
                new_hall_call_rx,
                state_update_rx,
                assigned_hall_call_tx,
                world_state_tx,
            ));
        }
    }

    NetworkHandle {
        role,
        my_id,
        hall_call_done_tx,
        new_hall_call_tx,
        state_update_tx,
        assigned_hall_call_rx,
        world_state_rx,
    }
}

/// Lowest live ID wins: try to connect to every elevator with id < mine.
/// If any answers, I'm a slave; otherwise I'm master.
async fn elect_role(my_id: u8) -> Role {
    for candidate in 1..my_id {
        if try_connect(candidate).await.is_some() {
            println!("[networking] found master candidate id={}", candidate);
            return Role::Slave { master_id: candidate };
        }
    }
    Role::Master
}

async fn try_connect(id: u8) -> Option<TcpStream> {
    let addr = master::addr_for(id);
    match time::timeout(Duration::from_millis(500), TcpStream::connect(&addr)).await {
        Ok(Ok(stream)) => Some(stream),
        _ => None,
    }
}