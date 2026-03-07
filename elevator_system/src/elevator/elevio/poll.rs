use tokio::sync::mpsc;
use tokio::time;

use super::elev;

#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum CallType {
    HallUp,
    HallDown,
    Cab,
}

impl CallType {
    pub fn from_u8(v: u8) -> CallType {
        match v {
            0 => CallType::HallUp,
            1 => CallType::HallDown,
            2 => CallType::Cab,
            _ => panic!("Invalid call type: {}", v),
        }
    }

    pub fn as_u8(self) -> u8 {
        match self {
            CallType::HallUp => 0,
            CallType::HallDown => 1,
            CallType::Cab => 2,
        }
    }

    pub fn is_hall(self) -> bool {
        matches!(self, CallType::HallUp | CallType::HallDown)
    }
}

#[derive(Debug, Eq, PartialEq, Hash, Clone, serde::Serialize, serde::Deserialize)]
pub struct CallButton {
    pub floor: u8,
    pub call: CallType,
}

pub async fn call_buttons(
    elev: elev::Elevio,
    ch: mpsc::UnboundedSender<CallButton>,
    period: time::Duration,
) {
    let mut prev = vec![[false; 3]; elev.num_floors.into()];
    loop {
        for f in 0..elev.num_floors {
            for c in 0..3u8 {
                let v = elev.call_button(f, c);
                if v && prev[f as usize][c as usize] != v {
                    if ch.send(CallButton { floor: f, call: CallType::from_u8(c) }).is_err() {
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
    ch: mpsc::UnboundedSender<Option<u8>>,
    period: time::Duration,
) {
    let mut prev: Option<u8> = None;
    loop {
        let current = elev.floor_sensor();
        if current != prev {
            if ch.send(current).is_err() {
                return;
            }
            prev = current;
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
