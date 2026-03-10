use tokio::sync::{
    mpsc::UnboundedReceiver as URx,
    broadcast::{Sender as BcTx},
};
use tokio::time::{self, Duration};
use std::{collections::BTreeMap, env, process::Command};


pub async fn store_online_elevators(
    local_id: u8,
    elevs_alive_tx: BcTx<Vec<u8>>,
    mut ping_received_rx: URx<u8>,
) {
    let mut online_elevators: BTreeMap<u8, time::Instant> = BTreeMap::new();
    let timeout_duration = Duration::from_millis(5000);
    let debounce_duration = Duration::from_secs(1);

    let mut debounce_deadline: Option<time::Instant> = None;

    loop {
        tokio::select! {
            Some(received_id) = ping_received_rx.recv() => {
                let now = time::Instant::now();
                
                if online_elevators.insert(received_id, now).is_none() {
                    debounce_deadline = Some(now + debounce_duration);
                }
            }

            _ = time::sleep(Duration::from_millis(500)) => {
                let now = time::Instant::now();
                let before_len = online_elevators.len();

                online_elevators.insert(local_id, now);

                online_elevators.retain(|_, last_seen| {
                    now.duration_since(*last_seen) < timeout_duration
                });

                if online_elevators.len() != before_len {
                    debounce_deadline = Some(now + debounce_duration);
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



pub const USER: &str = "MAC"; // "MAC" or "LAB"

pub async fn kill_instance(local_id: u8, kill_id: u16) {
    time::sleep(time::Duration::from_secs(5)).await;
    if local_id == 19 {
        if USER == "MAC" {
            let cmd =format!(r#"kill $(lsof -i :250{kill_id} | awk 'NR>1 && $1!="SimElevat" {{print $2}}')"#);
            let _ = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .status();
        } 
        else if USER == "LAB" {
            let user = "student";
            let host = format!("10.100.23.{}", kill_id);
            let password = &env::var("PASSWORD").expect("PASSWORD must be set"); // SSH-passord
            let remote_command = "kill -9 $(lsof -t -i :30000)";

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
}

pub async fn start_instance(local_id: u8, start_id: u16) {
    if local_id == 19 {
        if USER == "MAC" {
            let current_dir = env::current_dir().unwrap();
            let shell_cmd =format!("cd {} && cargo run {}", current_dir.display(), start_id);
            let apple_script = format!(
                r#"tell application "iTerm"
                    create window with default profile
                    tell current session of current window
                        write text "{}"
                    end tell
                end tell"#,
            shell_cmd
            );

            let _ = Command::new("osascript")
            .arg("-e")
            .arg(apple_script)
            .status(); 
        } 
        else if USER == "LAB" {
            let user = "student";
            let host = format!("10.100.23.{}", start_id);
            let password = &env::var("PASSWORD").expect("PASSWORD must be set"); // SSH-passord
            let remote_cmd = format!("elevatorserver --port 30000 & cd ~/sanntid10; ~/.cargo/bin/cargo run {}", start_id);

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
}