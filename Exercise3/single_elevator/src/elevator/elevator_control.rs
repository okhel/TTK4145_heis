use crate::elevator::{Elevator, elevio};



impl Elevator {
    pub fn goto_floor(&self, floor: u8) {
        if self.current_floor.lock().unwrap().unwrap() != 0 {
            if self.current_floor.lock().unwrap().unwrap() < floor {
                while self.current_floor.lock().unwrap().unwrap() < floor {
                    self.io.motor_direction(elevio::elev::DIRN_UP);
                }
                self.io.motor_direction(elevio::elev::DIRN_STOP);
            }
            if self.current_floor.lock().unwrap().unwrap() > floor {
                while self.current_floor.lock().unwrap().unwrap() > floor {
                    self.io.motor_direction(elevio::elev::DIRN_DOWN);
                }
                self.io.motor_direction(elevio::elev::DIRN_STOP);
            }
        }
    }
    pub fn goto_start_floor(&self) {
        while self.current_floor.lock().unwrap().unwrap() == 0 {
            self.io.motor_direction(elevio::elev::DIRN_UP);
        }
        self.io.motor_direction(elevio::elev::DIRN_STOP);
    }
}