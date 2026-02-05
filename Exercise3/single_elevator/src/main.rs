use std::{io, env};


pub mod elevator;
pub mod networking;
// use elevator::{elevio::elev::Elevio as e, NUM_FLOORS};

#[tokio::main]

async fn main() -> io::Result<()> {

    // let id: u8 = std::env::args().last().unwrap().parse().unwrap();
    // let mut ids = vec![19, 20, 21];

    // ids.retain(|x| *x !=id);
    // println!("Connecting to {}", ids[1]);

    // networking::udptest(ids[1]).await;
    
    elevator::elevator_runner().await?;
    
    Ok(())
}
