mod elevator_control;
mod elevator_doors;
mod elevator_lights;

pub mod elevio;
use elevio::elev::Elevio;
use elevio::poll::CallButton as CallButton;
use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx, unbounded_channel as uc};
use crate::order_management::Order;

use std::{io::*, time::*, sync::{Arc, Mutex}};


pub const NUM_FLOORS: u8 = 4;

#[derive(PartialEq)]
enum ElevState {
    Moving,
    Stationary,
    DoorsOpen
}


pub struct Elevator {
    io: Elevio,
    elev_state: Mutex<ElevState>,
    door_state: bool,
    pub last_floor: Mutex<Option<u8>>,
    id: usize,
}

impl Elevator {
    async fn init(id: u8) -> Result<Elevator> {

        let elevator = Self {
            io: Elevio::init(&format!("192.168.0.155:{}",id), NUM_FLOORS)?,
            elev_state: Mutex::new(ElevState::Stationary),
            door_state: false,
            last_floor: Mutex::new(None),
            id: id as usize,
        };

        Ok(elevator)

    }
}



pub async fn elevator_runner(id: u8, floor_order_tx: UTx<CallButton>, floor_msg_tx: UTx<CallButton>, floor_cmd_rx: URx<CallButton>, elev_req_rx: URx<bool>, elev_resp_tx: UTx<u8>, floor_msg_light_rx: URx<(Order, bool)>, at_floor_tx: UTx<u8>) -> Result<()> {

    // Initialize elevator
    let my_elev = Arc::new(Elevator::init(id).await?);

    let motor_control_elevio = my_elev.io.clone();
    let io_sensing_elevio = my_elev.io.clone();
    let io_light_elevio = my_elev.io.clone();
    let poll_period = Duration::from_millis(25);

    // Create channels to elevator IO for motor control task
    let (floor_sensor_tx, floor_sensor_rx) = uc::<Option<u8>>();{
        let elevator = motor_control_elevio.clone();
        tokio::spawn(async move {
            elevio::poll::floor_sensor(elevator, floor_sensor_tx, poll_period).await;
        });}

    // Create channels to elevator IO for io sensing task
    let (call_button_tx, call_button_rx) = uc::<elevio::poll::CallButton>();{
        let elevator = io_sensing_elevio.clone();
        tokio::spawn(async move {
            elevio::poll::call_buttons(elevator, call_button_tx, poll_period).await;
        });}

    
    // Start tasks
    let motor_control_task = tokio::spawn({
        let elev = Arc::clone(&my_elev);
        async move {
            elev.motor_control(floor_cmd_rx, floor_msg_tx, floor_sensor_rx, at_floor_tx).await;
        }
    });

    let io_sensing_task = tokio::spawn({
        let elev = Arc::clone(&my_elev);
        async move {
            elev.io_sensing(call_button_rx, floor_order_tx, elev_req_rx, elev_resp_tx).await;
        }
    });

    let io_light_task = tokio::spawn({
        let elev = Arc::clone(&my_elev);
        async move {
            elev.set_lights(floor_msg_light_rx).await;
        }
    });

    let _ = tokio::join!(motor_control_task, io_sensing_task, io_light_task);
    Ok(())

}


