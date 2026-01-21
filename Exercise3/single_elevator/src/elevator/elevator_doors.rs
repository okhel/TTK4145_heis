use crate::elevator::{Elevator, ElevState, elevio};

use std::time::Duration;
use std::thread::sleep;

impl Elevator {
    pub fn open_doors(&mut self) {
        self.door_state = true;
        self.io.door_light(true);
    }

    pub fn close_doors(&mut self) {
        self.door_state = false;
        self.io.door_light(false);
    }

    pub fn complete_order(&mut self) {
        self.open_doors();
        let duration = Duration::from_secs(3);
        sleep(duration);
        self.close_doors();
    }

    pub fn obstruction(&mut self) {

        // TODO: Add "at floor" criteria

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
            if self.door_state == true {
                sleep(Duration::from_secs(3));
                self.close_doors();
            }
            self.obs_state = false;
        }
    }

    pub fn stop_button(&mut self, stop_signal: bool) {

        // TODO: Add "at floor" criteria

        // If the stop button is pressed
        if stop_signal == true {

            // Change stop state to true and light up the stop button only once
            if self.stop_state == false {
                self.stop_state = true;
                self.io.stop_button_light(true);
            }
            
            // If the elevator is moving
            if self.elev_state != ElevState::Stationary {
                self.io.motor_direction(elevio::elev::DIRN_STOP);
                self.elev_state = ElevState::Stationary;
            }

            // If the elevator is at a floor
            self.open_doors();
        }

        // If the stop button is released
        if stop_signal == false {
            let duration = Duration::from_secs(3);
            sleep(duration);
            self.close_doors();
            
            self.io.stop_button_light(false);
            self.stop_state = false;
        }
    }
}