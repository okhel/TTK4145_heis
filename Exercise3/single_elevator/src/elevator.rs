mod elevator_control;
mod elevator_doors;
mod elevator_lights;

pub mod elevio;
use elevio::elev::Elevio;
use std::io::*;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;


pub const NUM_FLOORS: u8 = 4;

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
    pub stop_state: bool,
    obs_state: bool,
    pub current_floor: Arc<Mutex<Option<u8>>>
}

impl Elevator {
    pub fn init(elev_io: Arc<Elevio>, stop_init: bool, obs_init: bool, mut floor_sensor_rx: mpsc::UnboundedReceiver<u8>) -> Result<Elevator> {
        
        let current_floor = Arc::new(Mutex::new(None));
        let current_floor_clone = Arc::clone(&current_floor);
        *current_floor_clone.lock().unwrap() = Some(0);
        tokio::spawn(async move {
            while let Some(floor) = floor_sensor_rx.recv().await {
                *current_floor_clone.lock().unwrap() = Some(floor+1);
            }   
        });
        Ok(Self {
            io: elev_io,
            elev_state: ElevState::Stationary,
            door_state: false,
            stop_state: stop_init,
            obs_state: obs_init,
            current_floor
        })
    }
}


