use tokio::join;

pub mod networking;
pub mod process;

#[tokio::main]
async fn main(){

    let mut args= std::env::args();
    let local = args.nth(1).unwrap();
    let remote = args.nth(0).unwrap();


    let mysock = networking::init_socket(&local).await;

    let recv_sock = mysock.clone();
    let highest = process::master_check(recv_sock).await; // blocks untill there is no master online -> become master

    // restart code

    let _ = std::process::Command::new("gnome-terminal")
    .arg("--")
    .arg("bash")
    .arg("-c")
    .arg(format!("cargo run {remote} {local}"))
    .spawn();



    let send_sock = mysock.clone();
    let send_thread = tokio::spawn(async move{
        networking::ping_alive(send_sock, highest, remote).await;
    });
    
    let _ = join!(send_thread);
}
