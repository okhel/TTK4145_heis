use std::collections::HashMap;
use std::sync::Arc;
use serde::{Serialize, Deserialize};
use tokio::sync::{
    mpsc::{UnboundedSender as UTx, UnboundedReceiver as URx},
    Mutex,
};

// constants

pub const BASE_PORT: u16 = 20000;
pub const NUM_ELEVATORS: u8 = 3;
pub const NUM_FLOORS: u8 = 4;
pub const HEARTBEAT_INTERVAL_MS: u64 = 200;
pub const PEER_TIMEOUT_MS: u64 = 1000;

// good structs to have 

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum Direction {
    Up,
    Down,
    Idle,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HallCall {
    pub floor: u8,
    pub call: Direction,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CabCall {
    pub floor: u8,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ElevatorState {
    pub id: u8,
    pub floor: u8,
    pub direction: Direction,
    pub door_open: bool,
    pub assigned_hall_calls: Vec<HallCall>,
    pub cab_calls: Vec<u8>,
}

// messages

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Msg {
    // handshake, first message after connect to identify sender
    Hello { id: u8 },

    // slave to master
    StateUpdate(ElevatorState),
    NewHallCall(HallCall),
    HallCallDone(HallCall),

    // master to slave
    AssignHallCall(HallCall),
    WorldState { assignments: Vec<(HallCall, u8)> },

    // both
    Heartbeat,
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    Master,
    Slave { master_id: u8 },
}

// master world state

#[derive(Default)]
pub struct WorldState {
    pub assignments: HashMap<HallCall, u8>,
    // latest states for each elevator
    pub states: HashMap<u8, ElevatorState>,
}

pub type SharedWorld = Arc<Mutex<WorldState>>;

// for the channels 

pub struct NetworkHandle {
    pub role: Role,
    pub my_id: u8,

    pub hall_call_done_tx: UTx<HallCall>,

    pub new_hall_call_tx: UTx<HallCall>,

    pub state_update_tx: UTx<ElevatorState>,
    // receive hall call assignments from master
    pub assigned_hall_call_rx: URx<HallCall>,
    // receive full world state for syncing button lights
    pub world_state_rx: URx<Vec<(HallCall, u8)>>,
}