use crate::elevator::{Elevator, NUM_FLOORS, elevio};
use crate::elevator::elevio::poll::{CallButton, CallType};
use crate::networking::types::Position;
use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx};
use tokio::time::{sleep, Duration};

impl Elevator {

    // Go to a floor, cannot be called if not at a floor
    pub async fn motor_control(&self, mut floor_sensor_rx: URx<Option<u8>>, mut call_assign_rx: URx<CallButton>, update_state_tx: UTx<Position>, call_complete_tx: UTx<CallButton>, call_light_assign_tx: UTx<(CallButton, bool)>) {
        
        // Turn off all lights before initilaizing them
        for i in 0..NUM_FLOORS {
            let _ = call_light_assign_tx.send((CallButton {floor: i, call: CallType::Cab}, false));
            let _ = call_light_assign_tx.send((CallButton {floor: i, call: CallType::HallDown}, false));
            let _ = call_light_assign_tx.send((CallButton {floor: i, call: CallType::HallUp}, false));
        }
        self.io.door_light(false);

        // If not at a floor, go to start floor
        match URx::try_recv(&mut floor_sensor_rx) {
            Ok(Some(floor)) => {
                *self.last_floor.lock().unwrap() = Some(floor);
            }
            _ => {
                self.io.motor_direction(elevio::elev::DIRN_UP);
                loop {
                    if let Some(floor) = floor_sensor_rx.recv().await.unwrap() {
                        *self.last_floor.lock().unwrap() = Some(floor);
                        self.io.motor_direction(elevio::elev::DIRN_STOP);
                        break;
                    }
                }
            }
        };

        let _ = update_state_tx.send(Position { floor: self.last_floor.lock().unwrap().unwrap(), obstruction: *self.obstruction_state.lock().unwrap() });
        
        let mut direction: Option<u8> = Some(elevio::elev::DIRN_STOP);
        let mut target_call: CallButton = CallButton { floor: 0, call: CallType::HallUp };
        let mut between_floors: bool = false;

        loop {
            tokio::select! {
                biased;
                
                // Recieved new target floor
                Some(call) = call_assign_rx.recv() => {
                    target_call = call;
                    let last_floor = self.last_floor.lock().unwrap().unwrap();

                    // Update direction of travel, if necessary
                    match find_direction(last_floor, between_floors, target_call.floor, direction) {
                        Some(dir) => {
                            direction = Some(dir);
                            self.io.motor_direction(dir);
                        },

                        // If there is no change in direction, and direction is stop, send order complete message
                        None => {
                            if direction == Some(elevio::elev::DIRN_STOP) {
                                self.io.door_light(true);

                                sleep(Duration::from_secs(3)).await;
                                while *self.obstruction_state.lock().unwrap() {
                                    sleep(Duration::from_millis(100)).await;
                                }
                                self.io.door_light(false);
                                let _ = call_complete_tx.send(target_call.clone());
                            }
                        },
                    }
                }

                // Recieved new floor sensor measurement
                Some(floor_opt) = floor_sensor_rx.recv() => {
                    if let Some(floor) = floor_opt {
                        between_floors = false;
                        *self.last_floor.lock().unwrap() = Some(floor);
                        let _ = update_state_tx.send(Position { floor, obstruction: *self.obstruction_state.lock().unwrap() });

                        if floor == target_call.floor {
                            direction = Some(elevio::elev::DIRN_STOP);
                            self.io.motor_direction(elevio::elev::DIRN_STOP);
                            self.io.door_light(true);

                            sleep(Duration::from_secs(3)).await;
                            while *self.obstruction_state.lock().unwrap() {
                                sleep(Duration::from_millis(100)).await;
                            }
                            self.io.door_light(false);
                            // println!("Sending order complete message for {:?}", target_call);
                            let _ = call_complete_tx.send(target_call.clone());
                        }
                    }
                    else {
                        between_floors = true;
                    }
                }
                else => (),
            }
        }
    }

    pub async fn io_sensing(&self, mut call_rx: URx<elevio::poll::CallButton>, mut obstruction_rx: URx<bool>, call_request_tx: UTx<CallButton>, update_state_tx: UTx<Position>) {
        loop {
            tokio::select! {
                
                Some(call) = call_rx.recv() => {
                    let _ = call_request_tx.send(call);
                }

                Some(_) = obstruction_rx.recv() => {
                    let mut obs = self.obstruction_state.lock().unwrap();
                    *obs = !*obs;
                    update_state_tx.send(Position { floor: self.last_floor.lock().unwrap().unwrap(), obstruction: *obs }).unwrap();
                }

            }
        }
    }
    
    pub async fn set_lights(&self, mut call_light_assign_rx: URx<(CallButton, bool)>) {
        loop {
            if let Some((cb, on)) = call_light_assign_rx.recv().await {
                self.io.call_button_light(cb.floor, cb.call.as_u8(), on);
            }
        }
    }
}

// ---------- PURE FUNCTIONS ----------

fn find_direction(last_floor: u8, between_floors: bool, target_floor: u8, direction: Option<u8>) -> Option<u8> {

    // Set direction to appropriate direction, unless it is already set
    if last_floor < target_floor {
        match direction {
            Some(elevio::elev::DIRN_UP) => None,
            _ => Some(elevio::elev::DIRN_UP),
        }
    } else if last_floor > target_floor {
        match direction {
            Some(elevio::elev::DIRN_DOWN) => None,
            _ => Some(elevio::elev::DIRN_DOWN),
        }
    } else {
        match direction {
            Some(elevio::elev::DIRN_STOP) => None,
            _ => {
                if between_floors == false{
                    Some(elevio::elev::DIRN_STOP)
                } else {
                    if direction == Some(elevio::elev::DIRN_UP) {
                        Some(elevio::elev::DIRN_DOWN)
                    } else  {
                        Some(elevio::elev::DIRN_UP)
                    }
                }
            }
        }
    }
}