use crate::elevator::{Elevator, ElevState};

use std::time::Duration;
use std::thread::sleep;

impl Elevator {
    pub fn open_doors(&mut self) {
        self.io.door_light(true);
        self.door_state = true;
    }

    pub fn close_doors(&mut self) {
        self.io.door_light(false);
        self.door_state = false;
    }

    pub fn complete_order(&mut self) {
        self.open_doors();
        let duration = Duration::from_secs(3);
        sleep(duration);
        self.close_doors();
    }

    pub fn obstruction(&mut self) {

        // If the doors become obstructed, the elevator will
        if self.obs_state == false {
            self.obs_state = true;
            if self.elev_state == ElevState::Stationary {

                // If the doors are closed, the elevator will open the doors
                if self.door_state == false {
                    self.open_doors();
                }
            }
        }

        // If the doors become unobstructed, the elevator will close the doors
        else {
            self.obs_state = false;
            if self.door_state == true {
                sleep(Duration::from_secs(3));
                self.close_doors();
            }
        }
    }
}