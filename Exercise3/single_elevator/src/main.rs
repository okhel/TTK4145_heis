use std::io;
use std::thread::*;
use std::time::*;
use std::sync::Arc;
use crossbeam_channel as cbc;

pub mod elevator;
use elevator::elevio::elev::Elevio as e;


fn main() -> io::Result<()> {

    
    // Initialize Elevator IO
    let elev_io: e = e::init("localhost:15657", 4)?;
    let elev_io_ptr : Arc<e> = Arc::new(elev_io.clone());


    // Define channels
    let poll_period = Duration::from_millis(25);

    let (call_button_tx, call_button_rx) = cbc::unbounded::<elevator::elevio::poll::CallButton>();
    {
        let elevator = elev_io.clone();
        spawn(move || elevator::elevio::poll::call_buttons(elevator, call_button_tx, poll_period));
    }

    let (floor_sensor_tx, floor_sensor_rx) = cbc::unbounded::<u8>();
    {
        let elevator = elev_io.clone();
        spawn(move || elevator::elevio::poll::floor_sensor(elevator, floor_sensor_tx, poll_period));
    }

    let (stop_button_tx, stop_button_rx) = cbc::unbounded::<bool>();
    {
        let elevator = elev_io.clone();
        spawn(move || elevator::elevio::poll::stop_button(elevator, stop_button_tx, poll_period));
    }

    let (obstruction_tx, obstruction_rx) = cbc::unbounded::<bool>();
    {
        let elevator = elev_io.clone();
        spawn(move || elevator::elevio::poll::obstruction(elevator, obstruction_tx, poll_period));
    }


    // Initialize Elevator Object
    let mut stop_init = false;
    match stop_button_rx.try_recv() {
        Ok(_)       => stop_init = true,
        Err(_)      => stop_init = false
    }

    let mut obs_init = false;
    match obstruction_rx.try_recv() {
        Ok(_)       => obs_init = true,
        Err(_)      => obs_init = false
    }
    
    let mut my_elev = elevator::Elevator::init(elev_io_ptr, stop_init, obs_init)?; // give ownership of channels to object
    
    // MAIN LOOP

    // my_elev.goto_floor(2);
    my_elev.open_doors();


    loop  {
        cbc::select! {
            recv(obstruction_rx) -> a => {
                my_elev.obstruction();
            },
        }
    }
    Ok(())
}
