use crate::elevator::{Elevator, elevio};
use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx};
use tokio::time::{sleep, Duration};
use crate::elevator::elevio::poll::CallButton as CallButton;

impl Elevator {

    // Go to a floor, cannot be called if not at a floor
    pub async fn motor_control(&self, mut floor_cmd_rx: URx<CallButton>, floor_msg_tx: UTx<CallButton>, mut floor_sensor_rx: URx<Option<u8>>) {

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
        }
        
        let mut direction: Option<u8> = Some(elevio::elev::DIRN_STOP);
        let mut target_call: CallButton = CallButton { floor: 0, call: 0 };
        let mut between_floors: bool = false;

        loop {
            tokio::select! {
                biased;
                
                // Recieved new target floor
                Some(call) = floor_cmd_rx.recv() => {
                    target_call = call;
                    let last_floor = self.last_floor.lock().unwrap().unwrap();

                    // Update direction of travel, if necessary
                    match find_direction(last_floor, between_floors, target_call.floor, direction) {
                        Some(dir) => {
                            direction = Some(dir);
                            self.io.motor_direction(dir);
                            let new_state = match dir {
                                elevio::elev::DIRN_STOP => crate::elevator::ElevState::Stationary,
                                _ => crate::elevator::ElevState::Moving,
                            };
                            *self.elev_state.lock().unwrap() = new_state;
                        },

                        // If there is no change in direction, and direction is stop, send order complete message
                        None => {
                            if direction == Some(elevio::elev::DIRN_STOP) {
                                println!("Recieved order to current floor, when stopped");
                                // TODO: Wait 3 seconds, open doors stuff, THEN send order complete message
                                sleep(Duration::from_secs(3)).await;
                                let _ = floor_msg_tx.send(target_call.clone());
                            }
                        },
                    }
                }

                // Recieved new floor sensor measurement
                Some(floor_opt) = floor_sensor_rx.recv() => {
                    if let Some(floor) = floor_opt {
                        between_floors = false;
                        *self.last_floor.lock().unwrap() = Some(floor);

                        if floor == target_call.floor {
                            direction = Some(elevio::elev::DIRN_STOP);
                            self.io.motor_direction(elevio::elev::DIRN_STOP);
                            *self.elev_state.lock().unwrap() = crate::elevator::ElevState::Stationary;

                            // TODO: Wait 3 seconds, open doors stuff, THEN send order complete message
                            sleep(Duration::from_secs(3)).await;
                            let _ = floor_msg_tx.send(target_call.clone());
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

    pub async fn io_sensing(
        &self, mut call_rx: URx<elevio::poll::CallButton>,
        floor_order_tx: UTx<CallButton>,
        mut elev_req_rx: URx<bool>,
        elev_resp_tx: UTx<u8>) {
        loop {
            tokio::select! {
                
                Some(call) = call_rx.recv() => {
                    let _ = floor_order_tx.send(call);
                }

                Some(_) = elev_req_rx.recv() => {
                    let _ = elev_resp_tx.send(self.last_floor.lock().unwrap().unwrap());
                }
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