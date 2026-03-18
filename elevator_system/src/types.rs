use serde::{Deserialize, Serialize};

pub const NUM_FLOORS: u8 = 4;
pub const NUM_ELEVATORS: u8 = 3;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Position {
    pub floor: u8,
    pub obstruction: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ElevatorState {
    pub id: u8,
    pub floor: u8,
    pub obstruction: bool,
}
