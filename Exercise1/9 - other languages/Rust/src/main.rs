// Build and run with `cargo run`
// Try to build the program before add synchrinzation.

use std::thread;
use std::sync::{Arc, Mutex};

fn main() {
    // TODO: Find out what Arc is and why it is needed?
    // TODO: You need to add a Mutex, should it be Arc<Mutex<i32>> or Mutex<Arc<i32>>?
    let i: Arc<Mutex<i32>> = Arc::new(Mutex::new(0));

    let i_incrementing = i.clone();
    let i_decrementing = i.clone();

    let join_incrementing = thread::spawn(move || {
        for _ in 0..1_000_000 {
            // TODO: aquire the lock before using i
            *i_incrementing.lock().unwrap() += 1;
            // Do you have to release the mutex here?
        }
        i_incrementing
    });

    let join_decrementing = thread::spawn(move || {
        for _ in 0..1_000_000 {
            // TODO: aquire the lock before using i
            *i_decrementing.lock().unwrap() -= 1;
            // Do you have to release the mutex here?
        }

    });

    let v_back = join_incrementing.join().unwrap();
    join_decrementing.join().unwrap();

    // TODO: aquire the lock before using i
    println!("The number is: {}", *v_back.lock().unwrap());
}

