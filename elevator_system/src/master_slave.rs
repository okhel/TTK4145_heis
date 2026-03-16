use tokio::{sync::{
    mpsc::UnboundedReceiver as URx,
    broadcast::{Sender as BcTx, Receiver as BcRx},
}};
use tokio::time::{self, Duration, Instant};
use std::{collections::{BTreeMap, HashMap}, env, process::Command};
use dotenv::dotenv;
use crate::USER;

pub async fn store_online_elevators(
    local_id: u8,
    elevs_alive_tx: BcTx<Vec<u8>>,
    mut ping_received_rx: URx<u8>,
) {
    let mut online_elevators: BTreeMap<u8, time::Instant> = BTreeMap::new();
    let timeout_duration = Duration::from_millis(1000);
    let join_debounce: Duration = Duration::from_millis(50);
    let leave_debounce: Duration = Duration::from_millis(200);

    let mut debounce_deadline: Option<time::Instant> = None;
    let mut cleanup_interval = time::interval(Duration::from_millis(50));

    loop {
        tokio::select! {
            Some(received_id) = ping_received_rx.recv() => {
                let now = time::Instant::now();
                
                if online_elevators.insert(received_id, now).is_none() {
                    let new_deadline = now + join_debounce;
                    debounce_deadline = Some(match debounce_deadline {
                        Some(existing) => existing.min(new_deadline),
                        None => new_deadline,
                    });
                }
            }

            _ = cleanup_interval.tick() => {
                let now = time::Instant::now();
                let before_len = online_elevators.len();

                online_elevators.insert(local_id, now);

                online_elevators.retain(|_, last_seen| {
                    now.duration_since(*last_seen) < timeout_duration
                });

                if online_elevators.len() != before_len {
                    let new_deadline = now + leave_debounce;
                    debounce_deadline = Some(match debounce_deadline {
                        Some(existing) => existing.min(new_deadline),
                        None => new_deadline,
                    });
                }
            }

            // Debounce trigger
            _ = async {
                if let Some(deadline) = debounce_deadline {
                    time::sleep_until(deadline).await;
                }
            }, if debounce_deadline.is_some() => {
                elevs_alive_tx.send(online_elevators.keys().cloned().collect()).unwrap();
                println!("Online elevators: {:?}", online_elevators.keys());
                debounce_deadline = None;
            }
        }
    }
}

pub async fn restart_elevators(
    local_id: u8,
    ids: Vec<u8>,
    mut elevs_alive_rx: BcRx<Vec<u8>>,
) {
    let mut timers: HashMap<u8, Instant> = HashMap::new();
    let mut last_alive: Vec<u8> = Vec::new();

    loop {
        tokio::select! {
            Ok(alive) = elevs_alive_rx.recv() => {
                last_alive = alive.clone();
                let now = Instant::now();

                for &id in &ids {
                    if alive.contains(&id) {
                        timers.remove(&id);
                    } else {
                        timers.entry(id).or_insert(now + Duration::from_secs(5));
                    }
                }
            }

            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                let now = Instant::now();

                let expired: Vec<u8> = timers.iter()
                    .filter(|(_, deadline)| deadline <= &&now)
                    .map(|(&id, _)| id)
                    .collect();

                for id in expired {
                    timers.remove(&id);

                    let master_id = last_alive.iter().all(|&other_id| local_id <= other_id);

                    if master_id && id != local_id {
                        kill_instance(id);
                        start_instance(id);
                    }
                }
            }
        }
    }
}

pub fn kill_instance(kill_id: u8) {
    if USER == "MAC" {
        let cmd =format!("kill $(lsof -t -i :250{})", kill_id);
        let _ = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .status();
    } 
    else if USER == "LAB" {
        let user = "student";
        let host = format!("10.100.23.{}", kill_id);
        let password = &load_password(); // SSH-passord
        let remote_command = format!("kill -9 $(lsof -t -i :250{})",kill_id);

        let status = Command::new("sshpass")
            .args([
                "-p", password,
                "ssh", &format!("{}@{}", user, host),
                &format!("bash -lc '{}'", remote_command),
            ])
            .status()
            .expect("Failed to execute SSH command");
            
        if status.success() {
            println!("Successfully killed process on {}", host);
        } else {
            eprintln!("Failed to kill process on {}", host);
        }
    }
}

pub fn start_instance(start_id: u8) {
    if USER == "MAC" {
        let current_dir = env::current_dir().unwrap();
        let sim_cmd =format!("cd /Users/hanschristianlarsen/Documents/NTNU/semester6/Sanntidsprogrammering/simulator/Simulator-v2 && ./simElevatorServer --port 250{}", start_id);
        let cargo_cmd =format!("cd {} && cargo run {}", current_dir.display(), start_id);
        let apple_script = format!(
            r#"tell application "iTerm"
                create window with default profile
                tell current session of current window
                    write text "{}"
                end tell

                create window with default profile
                tell current session of current window
                    write text "{}"
                end tell

            end tell"#,
        sim_cmd, cargo_cmd
        );

        let _ = Command::new("osascript")
        .arg("-e")
        .arg(apple_script)
        .status(); 
    } 
    else if USER == "LAB" {
        let user = "student";
        let host = format!("10.100.23.{}", start_id);
        let password = &load_password();

        let kill_cmd = format!("pkill elevatorserver; pkill -f 'cargo run'");
        let _ = Command::new("sshpass")
            .args([
                "-p", password,
                "ssh", &format!("{}@{}", user, host),
                &kill_cmd,
            ])
            .status();

        let remote_cmd = format!(
            "nohup elevatorserver --port 250{} > /tmp/elevserv.log 2>&1 & \
             nohup ~/.cargo/bin/cargo run --manifest-path ~/sanntid10/Cargo.toml -- {} > /tmp/elevator.log 2>&1 &",
            start_id, start_id
        );

        let status = Command::new("sshpass")
            .args([
                "-p", password,
                "ssh", &format!("{}@{}", user, host),
                &format!("bash -lc '{}'", remote_cmd),
            ])
            .status()
            .expect("Failed to execute SSH command");

        if status.success() {
            println!("Successfully started process on {}", host);
        } else {
            eprintln!("Failed to start process on {}", host);
        }
    }
}

fn load_password() -> String {
    dotenv().ok();  // Laster miljøvariabler fra .env-filen
    env::var("PASSWORD").expect("PASSWORD must be set")
}