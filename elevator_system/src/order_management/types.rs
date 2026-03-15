use crate::elevator::elevio::poll::CallButton;
use crate::networking::types::{ElevatorState, Msg};

#[derive(Clone, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Order {
    pub cb: CallButton,
    pub elev_idx: usize,
}

impl std::fmt::Display for Order {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}, elev {}", self.cb, self.elev_idx)
    }
}

#[derive(Eq, PartialEq, Debug)]
pub enum Role {
    Master,
    Slave
}

pub struct NextOrderResult {
    pub next: Option<Order>,
    pub clear: Option<Order>,
}

pub const M: u8 = 3; // floors

pub enum Event {
    StateUpdateAndShare { states: Vec<ElevatorState> },
    StateUpdate { states: Vec<ElevatorState> },
    RequestOrder { order: Order },
    WantOrder { completed_order: Order },
    QueueOrders { orders: Vec<Order> },
    AssignOrders { orders: Vec<Order> },
    CompleteOrder { order: Order },
    ClearOrders { orders: Vec<Order> },
    AlivesUpdate { alive_elevs: Vec<u8> },
    
    AckReceived(Msg),
    OrderTimeout { elev_idx: usize },
}