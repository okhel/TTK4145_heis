use crate::elevator::elevio::poll::CallButton;
use crate::networking::types::{ElevatorState, Msg};

#[derive(Clone, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Order {
    pub cb: CallButton,
    pub elev_idx: usize,
}

pub struct NextOrderResult {
    pub next: Option<Order>,
    pub clear: Option<Order>,
}

pub const M: u8 = 3; // floors

pub enum Event {
    // from local elevator 
    ButtonPressed(CallButton),
    FloorReached(u8),
    OrderCompleted(CallButton),

    // from network peers
    PeerButtonPressed { from: u8, call: CallButton },
    PeerOrderCompleted { from: u8, call: CallButton },
    PeerStateUpdate(ElevatorState),
    PeerAssigned { to: u8, call: CallButton },
    AckReceived(Msg),
}