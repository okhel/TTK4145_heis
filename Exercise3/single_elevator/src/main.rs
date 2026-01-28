use std::io;

pub mod elevator;
// use elevator::{elevio::elev::Elevio as e, NUM_FLOORS};

#[tokio::main]
async fn main() -> io::Result<()> {


    let my_elev = elevator::Elevator::init().await?; // give ownership of channels to object    
    
    for _ in 0..100  {
        my_elev.goto_floor(1).await;
        my_elev.goto_floor(4).await;
        my_elev.goto_floor(2).await;
        my_elev.goto_floor(3).await;
    }
    Ok(())
}
