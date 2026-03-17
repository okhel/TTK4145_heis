mod elevator_control;

pub mod elevio;
use elevio::elev::Elevio;
use elevio::poll::CallButton as CallButton;
use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx, unbounded_channel as uc};

use std::{io::*, time::*, sync::{Arc, Mutex}};


pub const NUM_FLOORS: u8 = 4;
pub const NUM_ELEVATORS: u8 = 3;


pub struct Elevator {
    io: Elevio,
    obstruction_state: Mutex<bool>,
    pub last_floor: Mutex<Option<u8>>,
}

impl Elevator {
    async fn init(id: u8) -> Result<Elevator> {

        let elevator = Self {
            io: Elevio::init(&format!("127.0.0.1:250{}",id), NUM_FLOORS)?,
            obstruction_state: Mutex::new(false),
            last_floor: Mutex::new(None),
        };

        Ok(elevator)
    }
}

pub async fn elevator_runner(port: u8, call_request_tx: UTx<CallButton>, call_assign_rx: URx<CallButton>, update_floor_tx: UTx<u8>, call_complete_tx: UTx<CallButton>, call_light_assign_rx: URx<(CallButton, bool)>, call_light_assign_tx: UTx<(CallButton, bool)>) -> Result<()> {

    // Initialize elevator
    let my_elev = Arc::new(Elevator::init(port).await?);

    let motor_control_elevio = my_elev.io.clone();
    let call_sensing_elevio = my_elev.io.clone();
    let obstruction_elevio = my_elev.io.clone();
    let poll_period = Duration::from_millis(25);

    // Create channels to elevator IO for motor control task
    let (floor_sensor_tx, floor_sensor_rx) = uc::<Option<u8>>();{
        let elevator = motor_control_elevio.clone();
        tokio::spawn(async move {
            elevio::poll::floor_sensor(elevator, floor_sensor_tx, poll_period).await;
        });}

    // Create channels to elevator IO for io sensing task
    let (call_button_tx, call_button_rx) = uc::<elevio::poll::CallButton>();{
        let elevator = call_sensing_elevio.clone();
        tokio::spawn(async move {
            elevio::poll::call_buttons(elevator, call_button_tx, poll_period).await;
        });}
    
    let (obstruction_tx, obstruction_rx) = uc::<bool>();{
        let elevator = obstruction_elevio.clone();
        tokio::spawn(async move {
            elevio::poll::obstruction(elevator, obstruction_tx, poll_period).await;
        });
    }

    
    // Start tasks
    let motor_control_task = tokio::spawn({
        let elev = Arc::clone(&my_elev);
        async move {
            elev.motor_control(floor_sensor_rx, call_assign_rx, update_floor_tx, call_complete_tx, call_light_assign_tx).await;
        }
    });

    let io_sensing_task = tokio::spawn({
        let elev = Arc::clone(&my_elev);
        async move {
            elev.io_sensing(call_button_rx, obstruction_rx, call_request_tx).await;
        }
    });

    let io_light_task = tokio::spawn({
        let elev = Arc::clone(&my_elev);
        async move {
            elev.set_lights(call_light_assign_rx).await;
        }
    });
    
    let _ = tokio::join!(motor_control_task, io_sensing_task, io_light_task);
    Ok(())

}