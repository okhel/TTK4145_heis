mod elevator_control;
mod elevator_doors;
mod elevator_lights;

pub mod elevio;
use elevio::elev::Elevio;
use std::io::*;
use std::sync::Arc;


const NUM_FLOORS: u8 = 4;

#[derive(PartialEq)]
enum ElevState {
    DirUp,
    DirDown,
    Stationary
}


pub struct Elevator {
    io: Arc<Elevio>,
    elev_state: ElevState,
    door_state: bool,
    stop_state: bool,
    obs_state: bool,
}

impl Elevator {
    pub fn init(elev_io: Arc<Elevio>, stop_init: bool, obs_init: bool) -> Result<Elevator> {
        Ok(Self {
            io: elev_io,
            elev_state: ElevState::Stationary,
            door_state: false,
            stop_state: stop_init,
            obs_state: obs_init,

        })
    }
}


