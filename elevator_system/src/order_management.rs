use std::collections::{HashMap, HashSet, VecDeque};
use colored::Colorize;
use tokio::sync::{
    mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx, unbounded_channel as uc},
    broadcast::Receiver as BcRx,
};
use tokio::time::Duration;

use crate::{
    elevator::elevio::poll::CallButton,
    types::{ElevatorState, Position},
    networking::types::Msg,
    watchdog::WatchdogHandle,
};

use types::{Event, Order, Role};
pub mod types;
pub mod assignment;


fn msg_to_event(msg: Msg, role: &Role) -> Option<Event> {
    match msg {
        Msg::RequestOrder { order }                  => if role == &Role::Master { Some(Event::RequestOrder { order }) } else { Some(Event::QueueOrders { orders: vec![order] }) },
        Msg::QueueOrders { orders }             => Some(Event::QueueOrders { orders }),
        Msg::AssignOrders { orders }                   => Some(Event::AssignOrders { orders }),
        Msg::CompleteOrder { order }                 => Some(Event::CompleteOrder { order }),
        Msg::ClearOrders { orders }             => Some(Event::ClearOrders { orders }),
        Msg::StateUpdate { states }     => match role {
            Role::Master    => Some(Event::StateUpdateAndShare { states }),
            Role::Slave     => Some(Event::StateUpdate { states }),
        },
        Msg::Heartbeat                   => None,
    }
}

pub struct OrderManagerChannels {
    pub call_request_rx: URx<CallButton>,
    pub call_assign_tx: UTx<CallButton>,
    pub update_state_rx: URx<Position>,
    pub call_complete_rx: URx<CallButton>,
    pub call_light_tx: UTx<(CallButton, bool)>,
    pub net_send_tx: UTx<Msg>,
    pub net_recv_rx: URx<Msg>,
    pub ack_complete_rx: URx<(u32, Msg)>,
    pub elevs_alive_rx: BcRx<Vec<u8>>,
}

pub(crate) struct ManagerState {
    pub orders: VecDeque<Order>,
    pub positions: HashMap<usize, Position>,
    pub current_orders: HashMap<usize, Option<Order>>,
    pub alive_elevs: HashSet<usize>,
    pub pending_acks: HashMap<Order, Order>,
    pub role: Role,
    pub network_ready: bool,
    pub local_id: u8,
    pub local_idx: usize,
    pub call_assign_tx: UTx<CallButton>,
    pub call_light_tx: UTx<(CallButton, bool)>,
    pub network_tx: UTx<Msg>,
    pub order_wd: WatchdogHandle,
    pub idle_wd: WatchdogHandle,
}

mod handlers;

pub async fn order_manager(
    local_id: u8,
    mut ch: OrderManagerChannels,
) {
    let local_idx = local_id as usize;

    let (wd_expired_tx, mut wd_expired_rx) = uc::<usize>();
    let (idle_expired_tx, mut idle_expired_rx) = uc::<usize>();

    let order_wd = WatchdogHandle::spawn(Duration::from_secs(15), wd_expired_tx);
    let idle_wd = WatchdogHandle::spawn(Duration::from_secs(3), idle_expired_tx);

    let mut state = ManagerState {
        orders: VecDeque::with_capacity(9),
        positions: HashMap::new(),
        current_orders: HashMap::new(),
        alive_elevs: HashSet::new(),
        pending_acks: HashMap::new(),
        role: Role::Slave,
        network_ready: false,
        local_id,
        local_idx,
        call_assign_tx: ch.call_assign_tx,
        call_light_tx: ch.call_light_tx,
        network_tx: ch.net_send_tx,
        order_wd,
        idle_wd,
    };

    // Wait for initial floor reading, this signifies that the elevator is ready to receive orders
    let Position { floor: init_floor, obstruction: init_obstruction } = ch.update_state_rx.recv().await.unwrap();
    state.positions.insert(local_idx, Position { floor: init_floor, obstruction: init_obstruction });
    println!("{}", format!("Elev {} ready at floor {}", local_idx, init_floor).green().bold());

    loop {
        let event = tokio::select! {

            // Local messages
            Some(cb)            = ch.call_request_rx.recv()                    => Event::RequestOrder { order: Order { cb, elev_idx: local_idx } },
            Some(Position { floor, obstruction }) = ch.update_state_rx.recv()  => Event::StateUpdateAndShare { states: vec![ElevatorState {id: local_id, floor, obstruction}] },
            Some(cb)            = ch.call_complete_rx.recv()                   => Event::CompleteOrder { order: Order { cb, elev_idx: local_id as usize } },
            result              = ch.elevs_alive_rx.recv()                     => match result {
                Ok(alive_elevs) => Event::AlivesUpdate { alive_elevs },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    match ch.elevs_alive_rx.recv().await {
                        Ok(alive_elevs) => Event::AlivesUpdate { alive_elevs },
                        _ => continue,
                    }
                }
                Err(_) => continue,
            },
            Some((_seq, msg))   = ch.ack_complete_rx.recv()                    => Event::AckReceived(msg),
            Some(elev_idx)      = wd_expired_rx.recv()                         => Event::OrderTimeout { elev_idx },
            Some(elev_idx)      = idle_expired_rx.recv()                       => Event::IdleTimeout { elev_idx },

            // Network messages
            Some(msg)           = ch.net_recv_rx.recv()                        => match msg_to_event(msg, &state.role) {
                Some(e) => e,
                None    => continue,
            },
            else => panic!("All channels closed"),
        };

        state.handle_event(event);
    }
}
