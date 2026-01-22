use std::io;
use std::time::*;
use std::sync::Arc;
use tokio::sync::mpsc;

pub mod elevator;
use elevator::{elevio::elev::Elevio as e, NUM_FLOORS};

#[tokio::main]
async fn main() -> io::Result<()> {

    
    // Initialize Elevator IO
    let elev_io: e = e::init("localhost:15657", NUM_FLOORS)?;
    let elev_io_ptr : Arc<e> = Arc::new(elev_io.clone());


    // Define channels, receiving messages from elevator IO concurrently
    let poll_period = Duration::from_millis(25);

    let (call_button_tx, call_button_rx) = mpsc::unbounded_channel::<elevator::elevio::poll::CallButton>();
    {
        let elevator = elev_io.clone();
        tokio::spawn(async move {
            elevator::elevio::poll::call_buttons(elevator, call_button_tx, poll_period).await;
        });
    }

    let (floor_sensor_tx, mut floor_sensor_rx) = mpsc::unbounded_channel::<u8>();
    {
        let elevator = elev_io.clone();
        tokio::spawn(async move {
            elevator::elevio::poll::floor_sensor(elevator, floor_sensor_tx, poll_period).await;
        });
    }

    let (stop_button_tx, mut stop_button_rx) = mpsc::unbounded_channel::<bool>();
    {
        let elevator = elev_io.clone();
        tokio::spawn(async move {
            elevator::elevio::poll::stop_button(elevator, stop_button_tx, poll_period).await;
        });
    }

    let (obstruction_tx, mut obstruction_rx) = mpsc::unbounded_channel::<bool>();
    {
        let elevator = elev_io.clone();
        tokio::spawn(async move {
            elevator::elevio::poll::obstruction(elevator, obstruction_tx, poll_period).await;
        });
    }


    // Initialize Elevator Object
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

    let my_elev = elevator::Elevator::init(elev_io_ptr, stop_init, obs_init, floor_sensor_rx)?; // give ownership of channels to object    
    

    my_elev.goto_start_floor();
    if let Some(floor) = *my_elev.current_floor.lock().unwrap() {
        println!("Heisen starter i etasje {}", floor);
    }
    loop  {
        // my_elev.goto_floor(1);
        // my_elev.goto_floor(4);
        
        // if let Some(floor) = *my_elev.current_floor.lock().unwrap() {
        //     println!("Heisen er i etasje {}", floor);
        // }
    }
    Ok(())
}
