use std::collections::HashMap;
use colored::Colorize;
use tokio::sync::{
    mpsc::{UnboundedSender as UTx, unbounded_channel as uc},
    broadcast::Receiver as BcRx,
};
use tokio::time::Duration;

use crate::{
    types::{Position, ElevatorCommand, ElevatorEvent, ElevatorHandle, NetworkEvent, NetworkHandle},
    networking::types::Msg,
    watchdog::WatchdogHandle,
};

use types::{Event, Order};
use order_list::OrderList;
use membership::ClusterState;

pub mod types;
mod assignment;
mod order_list;
mod membership;
mod master_order_operations;

struct ManagerState {
    order_list: OrderList,
    cluster: ClusterState,
    positions: HashMap<usize, Position>,
    local_id: u8,
    local_idx: usize,
    elev_cmd_tx: UTx<ElevatorCommand>,
    network_tx: UTx<Msg>,
    order_wd: WatchdogHandle,
    idle_wd: WatchdogHandle,
}

mod handlers;

pub async fn order_manager(
    local_id: u8,
    mut elev: ElevatorHandle,
    mut net: NetworkHandle,
    mut alive_rx: BcRx<Vec<u8>>,
) {
    let local_idx = local_id as usize;

    let (wd_expired_tx, mut wd_expired_rx) = uc::<usize>();
    let (idle_expired_tx, mut idle_expired_rx) = uc::<usize>();

    let order_wd = WatchdogHandle::spawn(Duration::from_secs(15), wd_expired_tx);
    let idle_wd = WatchdogHandle::spawn(Duration::from_secs(3), idle_expired_tx);

    let mut state = ManagerState {
        order_list: OrderList::new(),
        cluster: ClusterState::new(),
        positions: HashMap::new(),
        local_id,
        local_idx,
        elev_cmd_tx: elev.cmd_tx,
        network_tx: net.send_tx,
        order_wd,
        idle_wd,
    };

    // Wait for initial floor reading, this signifies that the elevator is ready to receive orders
    loop {
        if let Some(ElevatorEvent::StateUpdate(pos)) = elev.event_rx.recv().await {
            state.positions.insert(local_idx, pos);
            println!("{}", format!("Elev {} ready at floor {}", local_idx, pos.floor).green().bold());
            break;
        }
    }

    loop {
        let event = tokio::select! {

            // Elevator events
            Some(elev_event) = elev.event_rx.recv() => match elev_event {
                ElevatorEvent::ButtonPress(cb) => Event::RequestOrder { order: Order { cb, elev_idx: local_idx } },
                ElevatorEvent::StateUpdate(pos) => Event::StateUpdateAndShare { states: vec![crate::types::ElevatorState { id: local_id, floor: pos.floor, obstruction: pos.obstruction }] },
                ElevatorEvent::OrderComplete(cb) => Event::CompleteOrder { order: Order { cb, elev_idx: local_id as usize } },
            },

            // Network events
            Some(net_event) = net.event_rx.recv() => match net_event {
                NetworkEvent::Message(msg) => match Event::from_msg(msg, state.cluster.role()) {
                    Some(e) => e,
                    None => continue,
                },
                NetworkEvent::AckComplete(msg) => Event::AckReceived(msg),
            },

            // Discovery
            result = alive_rx.recv() => match result {
                Ok(alive_elevs) => Event::AlivesUpdate { alive_elevs },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    match alive_rx.recv().await {
                        Ok(alive_elevs) => Event::AlivesUpdate { alive_elevs },
                        _ => continue,
                    }
                }
                Err(_) => continue,
            },

            // Watchdogs
            Some(elev_idx) = wd_expired_rx.recv()   => Event::OrderTimeout { elev_idx },
            Some(elev_idx) = idle_expired_rx.recv()  => Event::IdleTimeout { elev_idx },

            else => panic!("All channels closed"),
        };

        state.handle_event(event);
    }
}
