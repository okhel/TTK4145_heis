use driver_rust::elevio::elev as e;
use std::io;
mod elevator;

const NUM_FLOORS: u8 = 4;

fn main() -> io::Result<()> {
    let elevator_io = e::Elevator::init("localhost:15657", NUM_FLOORS)?;
    println!("Elevator started");
    elevator_io.motor_direction(e::DIRN_UP);
    loop  {
    }
    Ok(())
}
