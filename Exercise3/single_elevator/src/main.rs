use std::io;
pub mod elevator;


fn main() -> io::Result<()> {
    let my_elev = elevator::Elevator::init()?;
    
    my_elev.goto_floor(2);

    loop  {
    }
    Ok(())
}
