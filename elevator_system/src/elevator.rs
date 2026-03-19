mod motor;
mod sensing;

pub mod elevio;
use elevio::elev::Elevio;
use elevio::poll::CallButton;
use tokio::sync::mpsc::{unbounded_channel as uc};

use std::{io::*, time::*, sync::{Arc, Mutex}};

use crate::types::{ElevatorCommand, ElevatorInternal};
pub struct Elevator {
    io: Elevio,
    obstruction_state: Mutex<bool>,
    pub last_floor: Mutex<Option<u8>>,
}

impl Elevator {
    async fn init(id: u8) -> Result<Elevator> {
        let elevator = Self {
            io: Elevio::init(&format!("127.0.0.1:250{}", id), crate::types::NUM_FLOORS)?,
            obstruction_state: Mutex::new(false),
            last_floor: Mutex::new(None),
        };
        Ok(elevator)
    }
}

pub async fn elevator_runner(port: u8, internal: ElevatorInternal) -> Result<()> {

    // Initialize elevator
    let my_elev = Arc::new(Elevator::init(port).await?);
    let poll_period = Duration::from_millis(25);

    // Polling tasks
    let (floor_sensor_tx, floor_sensor_rx) = uc::<Option<u8>>();{
        let elevator = my_elev.io.clone();
        tokio::spawn(async move {
            elevio::poll::floor_sensor(elevator, floor_sensor_tx, poll_period).await;
        });}

    let (call_button_tx, call_button_rx) = uc::<elevio::poll::CallButton>();{
        let elevator = my_elev.io.clone();
        tokio::spawn(async move {
            elevio::poll::call_buttons(elevator, call_button_tx, poll_period).await;
        });}

    let (obstruction_tx, obstruction_rx) = uc::<bool>();{
        let elevator = my_elev.io.clone();
        tokio::spawn(async move {
            elevio::poll::obstruction(elevator, obstruction_tx, poll_period).await;
        });
    }

    // Internal channels for command dispatching
    let (assign_tx, assign_rx) = uc::<CallButton>();
    let (light_tx, light_rx) = uc::<(CallButton, bool)>();
    let init_light_tx = light_tx.clone();

    // routes ElevatorCommand to the right internal task
    let mut cmd_rx = internal.cmd_rx;
    tokio::spawn(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                ElevatorCommand::AssignOrder(cb) => { let _ = assign_tx.send(cb); }
                ElevatorCommand::SetLight(cb, on) => { let _ = light_tx.send((cb, on)); }
            }
        }
    });

    // Start tasks
    let motor_event_tx = internal.event_tx.clone();
    let motor_control_task = tokio::spawn({
        let elev = Arc::clone(&my_elev);
        async move {
            elev.motor_control(floor_sensor_rx, assign_rx, motor_event_tx, init_light_tx).await;
        }
    });

    let sensing_event_tx = internal.event_tx.clone();
    let io_sensing_task = tokio::spawn({
        let elev = Arc::clone(&my_elev);
        async move {
            elev.io_sensing(call_button_rx, obstruction_rx, sensing_event_tx).await;
        }
    });

    let io_light_task = tokio::spawn({
        let elev = Arc::clone(&my_elev);
        async move {
            elev.set_lights(light_rx).await;
        }
    });

    let _ = tokio::join!(motor_control_task, io_sensing_task, io_light_task);
    Ok(())
}
