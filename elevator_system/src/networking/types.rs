use serde::{Deserialize, Serialize};
use crate::order_management::types::Order;

// constants

pub const BASE_PORT: u16 = 20000;
pub const NUM_ELEVATORS: u8 = 3;
pub const NUM_FLOORS: u8 = 4;

// core types

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ElevatorState {
    pub id: u8,
    pub floor: u8,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Msg {

    RequestOrder { order: Order },
    QueueOrders { orders: Vec<Order> },
    AssignOrders { orders: Vec<Order> },
    CompleteOrder { order: Order },
    ClearOrders { orders: Vec<Order> },
    StateUpdate { states: Vec<ElevatorState> },
    
    Heartbeat,
}
