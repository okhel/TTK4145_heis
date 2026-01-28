mod elevator_control;
mod elevator_doors;
mod elevator_lights;

pub mod elevio;
use elevio::elev::Elevio;

use tokio::sync::mpsc;
use std::{io::*, time::*, sync::{Arc, Mutex}};


pub const NUM_FLOORS: u8 = 4;

#[derive(PartialEq)]
enum ElevState {
    DirUp,
    DirDown,
    Stationary
}


pub struct Elevator {
    io: Elevio,
    elev_state: ElevState,
    door_state: bool,
    pub last_floor: Arc<Mutex<Option<u8>>>,

    floor_rx: Mutex<mpsc::UnboundedReceiver<u8>>,
}

impl Elevator {
    pub async fn init() -> Result<Elevator> {

        // Initialize Elevator IO
        let elev_io: Elevio = Elevio::init("localhost:15657", NUM_FLOORS)?;


        // Define channels, receiving messages from elevator IO concurrently
        let poll_period = Duration::from_millis(25);

        let (call_button_tx, call_button_rx) = mpsc::unbounded_channel::<elevio::poll::CallButton>();{
            let elevator = elev_io.clone();
            tokio::spawn(async move {
                elevio::poll::call_buttons(elevator, call_button_tx, poll_period).await;
            });}

        let (floor_sensor_tx, mut floor_sensor_rx) = mpsc::unbounded_channel::<u8>();{
            let elevator = elev_io.clone();
            tokio::spawn(async move {
                elevio::poll::floor_sensor(elevator, floor_sensor_tx, poll_period).await;
            });}

        let (stop_button_tx, mut stop_button_rx) = mpsc::unbounded_channel::<bool>();{
            let elevator = elev_io.clone();
            tokio::spawn(async move {
                elevio::poll::stop_button(elevator, stop_button_tx, poll_period).await;
            });}

        let (obstruction_tx, mut obstruction_rx) = mpsc::unbounded_channel::<bool>();{
            let elevator = elev_io.clone();
            tokio::spawn(async move {
                elevio::poll::obstruction(elevator, obstruction_tx, poll_period).await;
            });}


        // Initialize Stop and Obstruction States
        let mut stop_init = false;
        match stop_button_rx.try_recv() {
            Ok(_) => stop_init = true,
            Err(_) => stop_init = false,
        }

        let mut obs_init = false;
        match obstruction_rx.try_recv() {
            Ok(_) => obs_init = true,
            Err(_) => obs_init = false,
        }

        let last_floor = Arc::new(Mutex::new(None));
        match floor_sensor_rx.try_recv() {
            Ok(floor) => {
                *last_floor.lock().unwrap() = Some(floor + 1);
            }
            Err(_) => {
                *last_floor.lock().unwrap() = None;
            }
        }

        let elevator = Self {
            io: elev_io,
            elev_state: ElevState::Stationary,
            door_state: false,
            last_floor,
            floor_rx: Mutex::new(floor_sensor_rx),
        };

        elevator.goto_start_floor().await;

        match *elevator.last_floor.lock().unwrap() {
            Some(floor) => println!("Heisen starter i etasje {}", floor),
            None => println!("FEIL: Heisen starter uten etasjemaling"),
        }
        Ok(elevator)

        
    }
}


