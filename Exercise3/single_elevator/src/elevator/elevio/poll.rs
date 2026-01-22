use tokio::sync::mpsc;
use tokio::time;

use super::elev;

#[derive(Debug)]
pub struct CallButton {
    pub floor: u8,
    pub call: u8,
}

pub async fn call_buttons(
    elev: elev::Elevio,
    ch: mpsc::UnboundedSender<CallButton>,
    period: time::Duration,
) {
    let mut prev = vec![[false; 3]; elev.num_floors.into()];
    loop {
        for f in 0..elev.num_floors {
            for c in 0..3 {
                let v = elev.call_button(f, c);
                if v && prev[f as usize][c as usize] != v {
                    if ch.send(CallButton { floor: f, call: c }).is_err() {
                        return;
                    }
                }
                prev[f as usize][c as usize] = v;
            }
        }
        time::sleep(period).await;
    }
}

pub async fn floor_sensor(
    elev: elev::Elevio,
    ch: mpsc::UnboundedSender<u8>,
    period: time::Duration,
) {
    let mut prev = u8::MAX;
    loop {
        if let Some(f) = elev.floor_sensor() {
            if f != prev {
                if ch.send(f).is_err() {
                    return;
                }
                prev = f;
            }
        }
        time::sleep(period).await;
    }
}

pub async fn stop_button(
    elev: elev::Elevio,
    ch: mpsc::UnboundedSender<bool>,
    period: time::Duration,
) {
    let mut prev = false;
    loop {
        let v = elev.stop_button();
        if prev != v {
            if ch.send(v).is_err() {
                return;
            }
            prev = v;
        }
        time::sleep(period).await;
    }
}

pub async fn obstruction(
    elev: elev::Elevio,
    ch: mpsc::UnboundedSender<bool>,
    period: time::Duration,
) {
    let mut prev = false;
    loop {
        let v = elev.obstruction();
        if prev != v {
            if ch.send(v).is_err() {
                return;
            }
            prev = v;
        }
        time::sleep(period).await;
    }
}
