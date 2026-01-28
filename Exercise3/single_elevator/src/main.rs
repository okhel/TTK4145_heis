use std::{io, env};


pub mod elevator;
pub mod networking;
// use elevator::{elevio::elev::Elevio as e, NUM_FLOORS};

#[tokio::main]

async fn main() -> io::Result<()> {

    let id: u8 = std::env::args().last().unwrap().parse().unwrap();
    let mut ids = vec![19, 20, 21];

    ids.retain(|x| *x !=id);
    println!("Connecting to {}", ids[1]);

    networking::udptest(ids[1]).await;

    let my_elev = elevator::Elevator::init().await?; // give ownership of channels to object    
    
    for _ in 0..100  {
        my_elev.goto_floor(1).await;
        my_elev.goto_floor(4).await;
        my_elev.goto_floor(2).await;
        my_elev.goto_floor(3).await;
    }
    Ok(())
}
