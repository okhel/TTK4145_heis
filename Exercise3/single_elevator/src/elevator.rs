mod elevator_control;
mod elevator_doors;
mod elevator_lights;
pub mod elevio;
use elevio::elev::Elevio;
use std::io::*;


const NUM_FLOORS: u8 = 4;

enum EStates {
    DirUp,
    DirDown,
    Stationary,
}

pub struct Elevator {
    elev_io: Elevio,
    state: EStates,
}

impl Elevator {
    pub fn init() -> Result<Elevator> {
        Ok(Self {
            elev_io: Elevio::init("localhost:15657", NUM_FLOORS)?,
            state: EStates::Stationary,
        })
    }
}


