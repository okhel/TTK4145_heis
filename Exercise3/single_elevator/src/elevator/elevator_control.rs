use crate::elevator::{Elevator, elevio};
use tokio::sync::mpsc::UnboundedReceiver as URx;

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

impl Elevator {

    // Go to a floor, cannot be called if not at a floor
    pub async fn goto_floor(&mut self, mut call_rx: URx<elevio::poll::CallButton>, mut floor_rx: URx<Option<u8>>) {

        // If not at a floor, go to start floor
        match URx::try_recv(&mut floor_rx) {
            Ok(Some(floor)) => {
                *self.last_floor.lock().unwrap() = Some(floor);
            }
            _ => {
                self.io.motor_direction(elevio::elev::DIRN_UP);
                loop {
                    if let Some(floor) = floor_rx.recv().await.unwrap() {
                        *self.last_floor.lock().unwrap() = Some(floor + 1);
                        self.io.motor_direction(elevio::elev::DIRN_STOP);
                        break;
                    }
                }
            }
        }
        
        let mut direction: Option<u8> = None;
        let mut target_floor: u8 = 0;
        let mut between_floors: bool = false;

        loop {
            tokio::select! {
                biased;
                
                // Recieved new target floor
                Some(call) = call_rx.recv() => {
                    target_floor = call.floor;
                    let mut last_floor = self.last_floor.lock().unwrap().unwrap();

                    // Update direction of travel, if necessary
                    match find_direction(last_floor, between_floors, target_floor, direction) {
                        Some(dir) => {
                            direction = Some(dir);
                            self.io.motor_direction(dir);
                        },
                        None => (),
                    }
                }

                // Recieved new floor sensor measurement
                Some(floor_opt) = floor_rx.recv() => {
                    if let Some(floor) = floor_opt {
                        between_floors = false;
                        *self.last_floor.lock().unwrap() = Some(floor);
                        if floor == target_floor {
                            direction = Some(elevio::elev::DIRN_STOP);
                            self.io.motor_direction(elevio::elev::DIRN_STOP);
                            // TODO: Send message: At destination floor
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
    
}