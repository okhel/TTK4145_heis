mod elevator_control;
mod elevator_doors;
mod elevator_lights;

pub mod elevio;
use elevio::elev::Elevio;
use std::io::*;
use std::sync::{Arc, Mutex};
use crossbeam_channel as cbc;


const NUM_FLOORS: u8 = 4;

enum ElevState {
    DirUp,
    DirDown,
    Stationary
}


pub struct Elevator {
    io: Arc<Elevio>,
    elev_state: ElevState,
    door_state: bool,
    pub stop_state: bool,
    obs_state: bool,
    pub current_floor: Arc<Mutex<Option<u8>>>
}

impl Elevator {
    pub fn init(elev_io: Arc<Elevio>, stop_init: bool, obs_init: bool, floor_sensor_rx: cbc::Receiver<u8>) -> Result<Elevator> {
        
        let current_floor = Arc::new(Mutex::new(None));
        let current_floor_clone = Arc::clone(&current_floor);
        *current_floor_clone.lock().unwrap() = Some(0);
        std::thread::spawn(move || {
            while let Ok(floor) = floor_sensor_rx.recv() {
                *current_floor_clone.lock().unwrap() = Some(floor+1);
            }
        });
        Ok(Self {
            io: elev_io,
            elev_state: ElevState::Stationary,
            door_state: false,
            stop_state: false,    // Temporary, channel is established and status is read after initialization
            obs_state: false,      // Temporary, channel is established and status is read after initialization
            current_floor
        })
    }
}


