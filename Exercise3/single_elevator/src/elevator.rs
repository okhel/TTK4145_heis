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
    _DirUp,
    _DirDown,
    Stationary
}


pub struct Elevator {
    io: Elevio,
    _elev_state: ElevState,
    door_state: bool,
    pub last_floor: Arc<Mutex<Option<u8>>>,
}

impl Elevator {
    async fn init() -> Result<Elevator> {

        // Initialize Elevator IO
        let elev_io: Elevio = Elevio::init("localhost:15657", NUM_FLOORS)?;

        // // Define channels, receiving messages from elevator IO concurrently
        // let poll_period = Duration::from_millis(25);

        // let (stop_button_tx, mut stop_button_rx) = mpsc::unbounded_channel::<bool>();{
        //     let elevator = elev_io.clone();
        //     tokio::spawn(async move {
        //         elevio::poll::stop_button(elevator, stop_button_tx, poll_period).await;
        //     });}

        // let (obstruction_tx, mut obstruction_rx) = mpsc::unbounded_channel::<bool>();{
        //     let elevator = elev_io.clone();
        //     tokio::spawn(async move {
        //         elevio::poll::obstruction(elevator, obstruction_tx, poll_period).await;
        //     });}


        // // Initialize Stop and Obstruction States
        // let mut _stop_init = false;
        // match stop_button_rx.try_recv() {
        //     Ok(_) => _stop_init = true,
        //     Err(_) => _stop_init = false,
        // }

        // let mut _obs_init = false;
        // match obstruction_rx.try_recv() {
        //     Ok(_) => _obs_init = true,
        //     Err(_) => _obs_init = false,
        // }

        let mut elevator = Self {
            io: elev_io,
            _elev_state: ElevState::Stationary,
            door_state: false,
            last_floor: Arc::new(Mutex::new(None)),
        };

        Ok(elevator)

        
    }
}



pub async fn elevator_runner() -> Result<()> {

    // Initialize elevator
    let mut my_elev = Elevator::init().await?;

    // Goto_floor
    let mut my_elevio = my_elev.io.clone();
    let poll_period = Duration::from_millis(25);

    let (call_button_tx, call_button_rx) = mpsc::unbounded_channel::<elevio::poll::CallButton>();{
        let elevator = my_elevio.clone();
        tokio::spawn(async move {
            elevio::poll::call_buttons(elevator, call_button_tx, poll_period).await;
        });}

    let (floor_sensor_tx, mut floor_sensor_rx) = mpsc::unbounded_channel::<Option<u8>>();{
        let elevator = my_elevio.clone();
        tokio::spawn(async move {
            elevio::poll::floor_sensor(elevator, floor_sensor_tx, poll_period).await;
        });}
    
    let goto_floor_task = tokio::spawn(async move {
        my_elev.goto_floor(call_button_rx, floor_sensor_rx).await;
    });

    tokio::join!(goto_floor_task);
    Ok(())

}


