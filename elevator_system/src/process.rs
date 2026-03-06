use std::env;
use std::process::Command;
use std::thread;
use std::time::Duration;

/// run the binary with `--backup <id>` to start in backup mode
pub fn is_backup() -> bool {
    env::args().any(|a| a == "--backup")
}

pub fn run_as_backup() {
    let exe = env::current_exe().expect("Failed to get current executable path");
    let args: Vec<String> = env::args().skip(1).filter(|a| a != "--backup").collect();

    loop {
        println!("[backup] Starting elevator process...");
        let status = Command::new(&exe).args(&args).status();

        match status {
            Ok(s) if s.success() => {
                println!("[backup] Elevator exited normally, shutting down.");
                break;
            }
            Ok(s) => {
                eprintln!("[backup] Elevator crashed with {}, restarting...", s);
                thread::sleep(Duration::from_secs(1));
            }
            Err(e) => {
                eprintln!("[backup] Failed to start elevator: {}, retrying...", e);
                thread::sleep(Duration::from_secs(2));
            }
        }
    }
}
