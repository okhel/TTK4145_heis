use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    Master,
    Slave { master_id: u8 },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Msg {
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

// communication with order_management, master emits networkevents, order_mgmt sends networkcommands

#[derive(Debug)]
pub enum NetworkEvent {
    NewHallCall { from: u8, call: HallCall },
    HallCallDone { from: u8, call: HallCall },
    StateUpdate(ElevatorState),
    PeerConnected(u8),
    PeerDisconnected(u8),
}

#[derive(Debug)]
pub enum NetworkCommand {
    AssignHallCall { to: u8, call: HallCall },
    BroadcastWorldState(Vec<(HallCall, u8)>),
}
