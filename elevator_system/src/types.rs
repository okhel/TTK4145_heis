use serde::{Deserialize, Serialize};

use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx};

use crate::{elevator::elevio::poll::CallButton, networking::types::Msg};

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

#[derive(Debug)]
pub enum ElevatorEvent {
    ButtonPress(CallButton),
    StateUpdate(Position),
    OrderComplete(CallButton),
}

#[derive(Debug)]
pub enum ElevatorCommand {
    AssignOrder(CallButton),
    SetLight(CallButton, bool),
}

pub struct ElevatorInternal {
    pub event_tx: UTx<ElevatorEvent>,
    pub cmd_rx: URx<ElevatorCommand>,
}

pub struct ElevatorHandle {
    pub event_rx: URx<ElevatorEvent>,
    pub cmd_tx: UTx<ElevatorCommand>,
}

#[derive(Debug)]
pub enum NetworkEvent {
    Message(Msg),
    AckComplete(Msg),
}

pub struct NetworkInternal {
    pub inbox: URx<Msg>,
    pub event_tx: UTx<NetworkEvent>,
}
pub struct NetworkHandle {
    pub send_tx: UTx<Msg>,
    pub event_rx: URx<NetworkEvent>,
}