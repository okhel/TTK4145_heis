use crate::elevator::{Elevator, elevio};



impl Elevator {

    // Go up until a floor sensor measurement is received
    pub async fn goto_start_floor(&self) {
        if self.last_floor.lock().unwrap().is_none() {
            self.io.motor_direction(elevio::elev::DIRN_UP);
            let floor = self.floor_rx.lock().unwrap().recv().await.unwrap();
            *self.last_floor.lock().unwrap() = Some(floor + 1);
            self.io.motor_direction(elevio::elev::DIRN_STOP);
        }
    }

    // Go to a floor, cannot be called if not at a floor
    pub async fn goto_floor(&self, floor: u8) {

        if self.last_floor.lock().unwrap().is_none() {
            panic!("Heisen har ingen tidligere etasjemaling");
        }

        else if self.last_floor.lock().unwrap().unwrap() == floor {
            println!("Heisen er allerede i etasje {}", floor);
            return;
        }

        else if self.last_floor.lock().unwrap().unwrap() > floor {
            self.io.motor_direction(elevio::elev::DIRN_DOWN);
            loop {
                let current_floor = self.floor_rx.lock().unwrap().recv().await.unwrap() + 1;
                *self.last_floor.lock().unwrap() = Some(current_floor); // Update last floor
                println!("Heisen er i etasje {}, går ned til etasje {}", current_floor, floor);
                if current_floor == floor {
                    self.io.motor_direction(elevio::elev::DIRN_STOP);
                    println!("Heisen er framme i etasje {}", floor);
                    break;
                }
            }
        }

        else if self.last_floor.lock().unwrap().unwrap() < floor {
            self.io.motor_direction(elevio::elev::DIRN_UP);
            loop {
                let current_floor = self.floor_rx.lock().unwrap().recv().await.unwrap() + 1;
                *self.last_floor.lock().unwrap() = Some(current_floor); // Update last floor
                println!("Heisen er i etasje {}, går opp til etasje {}", current_floor, floor);
                if current_floor == floor {
                    self.io.motor_direction(elevio::elev::DIRN_STOP);
                    println!("Heisen er framme i etasje {}", floor);
                    break;
                }
            }
        }
    }
    
}