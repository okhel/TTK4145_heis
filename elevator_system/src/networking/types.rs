use serde::{Deserialize, Serialize};

use crate::elevator::elevio::poll::CallButton;

// constants

pub const BASE_PORT: u16 = 20000;
pub const NUM_ELEVATORS: u8 = 3;
pub const NUM_FLOORS: u8 = 4;

// core types

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum Direction {
    Up,
    Down,
    Idle,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ElevatorState {
    pub id: u8,
    pub floor: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    Master,
    Slave { master_id: u8 },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Msg {
    // slave to master
    StateUpdate(ElevatorState),
    NewHallCall { from: u8, call: CallButton },
    HallCallDone { from: u8, call: CallButton },

    // master to slave
    AssignHallCall { to: u8, call: CallButton },
    WorldState { assignments: Vec<(CallButton, u8)> },

    // both
    Heartbeat,
}
