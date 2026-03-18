use serde::{Deserialize, Serialize};
use crate::order_management::types::Order;
use crate::types::ElevatorState;

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
