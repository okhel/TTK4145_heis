use crate::elevator::{Elevator, ElevState, elevio};

use tokio::time::{sleep, Duration};

impl Elevator {
    pub fn open_doors(&mut self) {
        self.door_state = true;
        self.io.door_light(true);
    }

    pub fn close_doors(&mut self) {
        self.door_state = false;
        self.io.door_light(false);
    }

    pub async fn complete_order(&mut self) {
        self.open_doors();
        let duration = Duration::from_secs(3);
        sleep(duration).await;
        self.close_doors();
    }

    // pub async fn obstruction(&mut self) {

    //     // TODO: Add "at floor" criteria

    //     // If the doors become obstructed, the elevator will
    //     if self.obs_state == false {
    //         self.obs_state = true;
    //         if self.elev_state == ElevState::Stationary {

    //             // If the doors are closed, the elevator will open the doors
    //             if self.door_state == false {
    //                 self.open_doors();
    //             }
    //         }

    //     }

    //     // If the doors become unobstructed, the elevator will close the doors
    //     else {
    //         if self.door_state == true {
    //             sleep(Duration::from_secs(3)).await;
    //             self.close_doors();
    //         }
    //         self.obs_state = false;
    //     }
    // }

    // pub async fn stop_button(&mut self, on: bool) {

    //     if on == true {

    //         // Change stop state to true, light up the stop button, and stop motor only once
    //         if self.stop_state == false {
    //             self.stop_state = true;
    //             self.io.stop_button_light(true);

    //             // If the elevator is moving
    //             if self.elev_state != ElevState::Stationary {
    //                 self.io.motor_direction(elevio::elev::DIRN_STOP);
    //                 self.elev_state = ElevState::Stationary;
    //             }  
    //         }
    //     }
    //     else {
    //         self.io.stop_button_light(false);
    //         self.stop_state = false;
    //     }
    // }
}