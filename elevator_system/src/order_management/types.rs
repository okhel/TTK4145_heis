use crate::elevator::elevio::poll::{CallButton, CallType};
use crate::types::ElevatorState;
use crate::networking::types::Msg;

#[derive(Clone, Debug, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Order {
    pub cb: CallButton,
    pub elev_idx: usize,
}

impl Order {
    pub fn is_for(&self, elev_idx: usize) -> bool {
        self.elev_idx == elev_idx || self.cb.call != CallType::Cab
    }
}

impl std::fmt::Display for Order {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} - elev {}", self.cb, self.elev_idx)
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

pub enum Event {
    StateUpdateAndShare { states: Vec<ElevatorState> },
    StateUpdate { states: Vec<ElevatorState> },
    AlivesUpdate { alive_elevs: Vec<u8> },

    RequestOrder { order: Order },
    QueueOrders { orders: Vec<Order> },
    AssignOrders { orders: Vec<Order> },
    CompleteOrder { order: Order },
    ClearOrders { orders: Vec<Order> },

    AckReceived(Msg),
    OrderTimeout { elev_idx: usize },
    IdleTimeout { elev_idx: usize },
}

impl Event {
    pub fn from_msg(msg: Msg, role: &Role) -> Option<Self> {
        match msg {
            Msg::RequestOrder { order } => if role == &Role::Master { Some(Self::RequestOrder { order }) } else { Some(Self::QueueOrders { orders: vec![order] }) },
            Msg::QueueOrders { orders }  => Some(Self::QueueOrders { orders }),
            Msg::AssignOrders { orders } => Some(Self::AssignOrders { orders }),
            Msg::CompleteOrder { order } => Some(Self::CompleteOrder { order }),
            Msg::ClearOrders { orders }  => Some(Self::ClearOrders { orders }),
            Msg::StateUpdate { states }  => match role {
                Role::Master => Some(Self::StateUpdateAndShare { states }),
                Role::Slave  => Some(Self::StateUpdate { states }),
            },
            Msg::Heartbeat => None,
        }
    }
}
