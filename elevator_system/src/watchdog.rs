use tokio::time::{Duration, Instant};
use tokio::sync::mpsc::{UnboundedReceiver as URx, UnboundedSender as UTx, unbounded_channel as uc};
use std::collections::HashMap;

pub struct WatchdogHandle {
    reset_tx: UTx<usize>,
    remove_tx: UTx<usize>,
}

impl WatchdogHandle {
    pub fn spawn(timeout: Duration, expired_tx: UTx<usize>) -> Self {
        let (reset_tx, reset_rx) = uc::<usize>();
        let (remove_tx, remove_rx) = uc::<usize>();
        tokio::spawn(watchdog_timer(timeout, reset_rx, remove_rx, expired_tx));
        Self { reset_tx, remove_tx }
    }

    pub fn reset(&self, id: usize) {
        let _ = self.reset_tx.send(id);
    }

    pub fn remove(&self, id: usize) {
        let _ = self.remove_tx.send(id);
    }
}

async fn watchdog_timer(
    timeout: Duration,
    mut reset_rx: URx<usize>,
    mut remove_rx: URx<usize>,
    expired_tx: UTx<usize>,
) {
    let mut timers: HashMap<usize, Instant> = HashMap::new();

    loop {
        tokio::select! {
            Some(elev_idx) = reset_rx.recv() => {
                timers.insert(elev_idx, Instant::now() + timeout);
            }

            Some(elev_idx) = remove_rx.recv() => {
                timers.remove(&elev_idx);
            }

            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                let now = Instant::now();

                let expired: Vec<usize> = timers.iter()
                    .filter(|(_, deadline)| *deadline <= &now)
                    .map(|(&id, _)| id)
                    .collect();

                for id in expired {
                    timers.remove(&id);
                    let _ = expired_tx.send(id);
                }
            }
        }
    }
}
