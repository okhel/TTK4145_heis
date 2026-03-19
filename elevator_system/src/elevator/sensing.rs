use crate::elevator::{Elevator};
use crate::elevator::elevio;
use crate::elevator::elevio::poll::CallButton;
use crate::types::{ElevatorEvent, Position};
use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx};

impl Elevator {
    pub async fn io_sensing(
        &self,
        mut call_rx: URx<elevio::poll::CallButton>,
        mut obstruction_rx: URx<bool>,
        event_tx: UTx<ElevatorEvent>,
    ) {
        loop {
            tokio::select! {
                Some(call) = call_rx.recv() => {
                    let _ = event_tx.send(ElevatorEvent::ButtonPress(call));
                }

                Some(_) = obstruction_rx.recv() => {
                    let mut obs = self.obstruction_state.lock().unwrap();
                    *obs = !*obs;
                    event_tx.send(ElevatorEvent::StateUpdate(Position {
                        floor: self.last_floor.lock().unwrap().unwrap(),
                        obstruction: *obs,
                    })).unwrap();
                }
            }
        }
    }

    pub async fn set_lights(&self, mut light_rx: URx<(CallButton, bool)>) {
        loop {
            if let Some((cb, on)) = light_rx.recv().await {
                self.io.call_button_light(cb.floor, cb.call.as_u8(), on);
            }
        }
    }
}
