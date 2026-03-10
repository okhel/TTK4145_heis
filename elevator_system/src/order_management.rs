use std::collections::{HashMap, HashSet, VecDeque};
use tokio::sync::mpsc::{
    UnboundedReceiver as URx, UnboundedSender as UTx,
};

use crate::elevator::elevio::poll::{CallButton, CallType};
use crate::networking::types::{ElevatorState, Msg};
use types::{Event, Order};

pub mod types;
pub mod utils;
use utils::{assign_new_orders, assign_next_order};


fn msg_to_event(msg: Msg) -> Option<Event> {
    match msg {
        Msg::NewHallCall { from, call }  => Some(Event::PeerButtonPressed { from, call }),
        Msg::HallCallDone { from, call } => Some(Event::PeerOrderCompleted { from, call }),
        Msg::StateUpdate(state)          => Some(Event::PeerStateUpdate(state)),
        Msg::AssignHallCall { to, call } => Some(Event::PeerAssigned { to, call }),
        Msg::WorldState { .. }           => None,
        Msg::Heartbeat                   => None,
    }
}

// TODO: add some master logic to only let master assign orders, maybe add a Role in the ManagerState
struct ManagerState {
    orders: VecDeque<Order>,
    positions: HashMap<usize, u8>,
    current_orders: HashMap<usize, Option<Order>>,
    alive_elevs: Vec<usize>,
    pending_acks: HashMap<CallButton, Order>
}

impl ManagerState {
    fn new() -> Self {
        Self {
            orders: VecDeque::with_capacity(9),
            positions: HashMap::new(),
            current_orders: HashMap::new(),
            alive_elevs: Vec::new(),
            pending_acks: HashMap::new(),
        }
    }
}

pub async fn order_manager(
    local_id: u8,
    mut call_request_rx: URx<CallButton>,
    call_assign_tx: UTx<CallButton>,
    mut update_floor_rx: URx<u8>,
    mut call_complete_rx: URx<CallButton>,
    call_light_tx: UTx<(CallButton, bool)>,
    network_tx: UTx<Msg>,
    mut network_rx: URx<Msg>,
    mut ack_complete_rx: URx<(u32, Msg)>,
) {
    let local_idx = local_id as usize;
    let mut state = ManagerState::new();

    loop {
        println!("\n-\nOrders: {:?}\nCurrent orders: {:?}", state.orders, state.current_orders);

        let event = tokio::select! {
            Some(cb)       = call_request_rx.recv() => Event::ButtonPressed(cb),
            Some(floor)    = update_floor_rx.recv()  => Event::FloorReached(floor),
            Some(cb)       = call_complete_rx.recv()  => Event::OrderCompleted(cb),
            Some(msg)      = network_rx.recv()        => match msg_to_event(msg) {
                Some(e) => e,
                None    => continue,
            },
            Some((_seq, msg)) = ack_complete_rx.recv() => Event::AckReceived(msg),
            else => panic!("All channels closed"),
        };

        handle_event(local_id, local_idx, event, &mut state, &call_assign_tx, &call_light_tx, &network_tx);
    }
}

fn handle_event(
    local_id: u8,
    local_idx: usize,
    event: Event,
    state: &mut ManagerState,
    call_assign_tx: &UTx<CallButton>,
    call_light_tx: &UTx<(CallButton, bool)>,
    network_tx: &UTx<Msg>,
) {
    match event {
        // local buttonpress
       Event::ButtonPressed(cb) => {
            let order = Order { cb: cb.clone(), elev_idx: local_idx };
            state.pending_acks.insert(cb.clone(), order);
            let _ = network_tx.send(Msg::NewHallCall { from: local_id, call: cb });
        }

        // local floor update
        Event::FloorReached(floor) => {
            state.positions.insert(local_idx, floor);
            state.alive_elevs = state.positions.keys().copied().collect();
            let _ = network_tx.send(Msg::StateUpdate(ElevatorState {
                id: local_id,
                floor,
            }));
        }

        // local order complete
        Event::OrderCompleted(cb) => {
            let _ = network_tx.send(Msg::HallCallDone { from: local_id, call: cb.clone() });
            let order = Order { cb: cb.clone(), elev_idx: local_idx };
            complete_and_reassign(order, state, call_assign_tx, call_light_tx, &network_tx, local_idx);
        }

        // peer pressed a hall button
        Event::PeerButtonPressed { from, call } => {
            let order = Order { cb: call, elev_idx: from as usize };
            try_assign_new(order, state, call_assign_tx, network_tx, local_idx);
        }

        // master assign call to me 
        Event::PeerAssigned { to, call } => {
            if to == local_id {
                let _ = call_assign_tx.send(call);
            }
        }

        // peer completed hall call
        Event::PeerOrderCompleted { from, call } => {
            let order = Order { cb: call.clone(), elev_idx: from as usize };
            let cab_order = Order {
                cb: CallButton { floor: call.floor, call: CallType::Cab },
                elev_idx: from as usize,
            };
            state.orders.retain(|o| o != &order);
            state.orders.retain(|o| o != &cab_order);
            state.current_orders.insert(from as usize, None);
            let _ = call_light_tx.send((call, false));
        }

        // peer state update
        Event::PeerStateUpdate(peer_state) => {
            let idx = peer_state.id as usize;
            if peer_state.floor != u8::MAX {
                // u8::MAX used to mean offline, so only update position if not offline
                state.positions.insert(idx, peer_state.floor);
            } else {
                state.positions.remove(&idx);
                if let Some(Some(order)) = state.current_orders.get(&idx) {
                    state.orders.push_front(order.clone());
                }
                state.current_orders.insert(idx, None);
            }
            state.alive_elevs = state.positions.keys().copied().collect();
        }

        // got an acked message
        Event::AckReceived(msg) => {
            if let Msg::NewHallCall { call, .. } = msg {
                if let Some(order) = state.pending_acks.remove(&call) {
                    let _ = call_light_tx.send((call, true));
                    try_assign_new(order, state, call_assign_tx, network_tx, local_idx);
                }
            }
        }
    }
}


// helpers
fn try_assign_new(
    order: Order,
    state: &mut ManagerState,
    call_assign_tx: &UTx<CallButton>,
    network_tx: &UTx<Msg>,
    local_idx: usize,
) {
    if let Some(elev_idx) = assign_new_orders(
        order.clone(),
        &mut state.orders,
        &state.positions,
        &mut state.current_orders,
        &state.alive_elevs,
    ) {
        if elev_idx == local_idx {
            let _ = call_assign_tx.send(order.cb.clone());
        } else if order.cb.call.is_hall() {
            let _ = network_tx.send(Msg::AssignHallCall {
                to: elev_idx as u8,
                call: order.cb,
            });
        }
    } else {
        println!("Could not assign new order: {:?}", order);
    }
}

fn complete_and_reassign(
    order: Order,
    state: &mut ManagerState,
    call_assign_tx: &UTx<CallButton>,
    call_light_tx: &UTx<(CallButton, bool)>,
    network_tx: &UTx<Msg>,
    local_idx: usize,
) {
    let cab_order = Order {
        cb: CallButton { floor: order.cb.floor, call: CallType::Cab },
        elev_idx: order.elev_idx,
    };
    state.orders.retain(|o| o != &order);
    state.orders.retain(|o| o != &cab_order);
    state.current_orders.insert(order.elev_idx, None);
    println!("Cleared order {:?}", order);

    let result = assign_next_order(order.clone(), &mut state.orders, &mut state.current_orders);
    if let Some(ref next) = result.next {
        if next.elev_idx == local_idx {
            let _ = call_assign_tx.send(next.cb.clone());
        } else if next.cb.call.is_hall() {
            let _ = network_tx.send(Msg::AssignHallCall {
                to: next.elev_idx as u8,
                call: next.cb.clone(),
            });
        }
    }

    let mut to_clear: HashSet<Order> = HashSet::from([order, cab_order]);
    if let Some(clear) = result.clear {
        to_clear.insert(clear);
    }
    for cleared in &to_clear {
        let _ = call_light_tx.send((cleared.cb.clone(), false));
    }
}