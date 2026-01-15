mod elevator_control;
mod elevator_doors;
mod elevator_lights;
pub mod elevio;
use elevio::elev::Elevio;
use std::io::*;


//pub use elevator_control::{move_to_floor};

const NUM_FLOORS: u8 = 4;

pub struct Elevator {
    elev_io: Elevio,
}

impl Elevator {
    pub fn init() -> Result<Elevator> {
        Ok(Self {
            elev_io: Elevio::init("localhost:15657", NUM_FLOORS)?
        })
    }
}


