use std::collections::{HashMap, HashSet, VecDeque};
use colored::Colorize;
use tokio::sync::{
    mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx, unbounded_channel as uc},
    broadcast::Receiver as BcRx,
};
use tokio::time::Duration;

use crate::{
    elevator::elevio::poll::CallButton,
    networking::types::{ElevatorState, Msg},
    watchdog::watchdog_timer,
};

use types::{Event, Order, Role};
pub mod types;
pub mod assignment;


fn msg_to_event(msg: Msg, role: &Role) -> Option<Event> {
    match msg {
        Msg::RequestOrder { order }                  => if role == &Role::Master { Some(Event::RequestOrder { order }) } else { None },
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
pub(crate) struct ManagerState {
    pub orders: VecDeque<Order>,
    pub positions: HashMap<usize, u8>,
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
    pub ack_complete_tx: UTx<(u32, Msg)>,
    pub want_order_tx: UTx<Order>,
    pub wd_reset_tx: UTx<usize>,
    pub wd_remove_tx: UTx<usize>,
    pub idle_reset_tx: UTx<usize>,
    pub idle_remove_tx: UTx<usize>,
}

mod handlers;

pub async fn order_manager(
    local_id: u8,
    mut call_request_rx: URx<CallButton>,
    call_assign_tx: UTx<CallButton>,
    mut update_floor_rx: URx<u8>,
    mut call_complete_rx: URx<CallButton>,
    call_light_assign_tx: UTx<(CallButton, bool)>,
    network_inbox_tx: UTx<Msg>,
    mut network_rx: URx<Msg>,
    mut ack_complete_rx: URx<(u32, Msg)>,
    order_ack_complete_tx: UTx<(u32, Msg)>,
    mut mgmt_elevs_alive_rx: BcRx<Vec<u8>>,
) {
    let local_idx = local_id as usize;

    let (wd_reset_tx, wd_reset_rx) = uc::<usize>();
    let (wd_remove_tx, wd_remove_rx) = uc::<usize>();
    let (wd_expired_tx, mut wd_expired_rx) = uc::<usize>();
    let (want_order_tx, mut want_order_rx) = uc::<Order>();
    let (idle_reset_tx, idle_reset_rx) = uc::<usize>();
    let (idle_remove_tx, idle_remove_rx) = uc::<usize>();
    let (idle_expired_tx, mut idle_expired_rx) = uc::<usize>();

    tokio::spawn(watchdog_timer(
        Duration::from_secs(15),
        wd_reset_rx,
        wd_remove_rx,
        wd_expired_tx,
    ));

    tokio::spawn(watchdog_timer(
        Duration::from_secs(3),
        idle_reset_rx,
        idle_remove_rx,
        idle_expired_tx,
    ));

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
        call_assign_tx,
        call_light_tx: call_light_assign_tx,
        network_tx: network_inbox_tx,
        ack_complete_tx: order_ack_complete_tx,
        want_order_tx,
        wd_reset_tx,
        wd_remove_tx,
        idle_reset_tx,
        idle_remove_tx,
    };

    // Wait for initial floor reading, this signifies that the elevator is ready to receive orders
    let init_floor = update_floor_rx.recv().await.unwrap();
    state.positions.insert(local_idx, init_floor);
    println!("{}", format!("Elev {} ready at floor {}", local_idx, init_floor).green().bold());

    

    loop {
        let event = tokio::select! {

            // Local messages
            Some(cb)            = call_request_rx.recv()                    => Event::RequestOrder { order: Order { cb: cb, elev_idx: local_idx } },
            Some(floor)         = update_floor_rx.recv()                    => Event::StateUpdateAndShare { states: vec![ElevatorState {id: local_id, floor}] },
            Some(cb)            = call_complete_rx.recv()                   => Event::CompleteOrder { order: Order { cb: cb, elev_idx: local_id as usize } },
            result              = mgmt_elevs_alive_rx.recv()                => match result {
                Ok(alive_elevs) => Event::AlivesUpdate { alive_elevs },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    match mgmt_elevs_alive_rx.recv().await {
                        Ok(alive_elevs) => Event::AlivesUpdate { alive_elevs },
                        _ => continue,
                    }
                }
                Err(_) => continue,
            },
            Some(order)         = want_order_rx.recv()                      => Event::WantOrder { completed_order: order },
            Some((_seq, msg))   = ack_complete_rx.recv()                    => Event::AckReceived(msg),
            Some(elev_idx)      = wd_expired_rx.recv()                      => Event::OrderTimeout { elev_idx },
            Some(elev_idx)      = idle_expired_rx.recv()                    => Event::IdleTimeout { elev_idx },

            // Network messages
            Some(msg)           = network_rx.recv()                         => match msg_to_event(msg, &state.role) {
                Some(e) => e,
                None    => continue,
            },
            else => panic!("All channels closed"),
        };

        state.handle_event(event);
    }
}


