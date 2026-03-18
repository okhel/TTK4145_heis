use crate::elevator::Elevator;
use crate::elevator::elevio;
use crate::elevator::elevio::poll::CallButton;
use crate::types::Position;
use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx};

impl Elevator {
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
