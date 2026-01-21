use crate::elevator::{Elevator, elevio};



impl Elevator {
    pub fn goto_floor(&self, floor: u8) {
        self.io.motor_direction(elevio::elev::DIRN_UP);
    }
}