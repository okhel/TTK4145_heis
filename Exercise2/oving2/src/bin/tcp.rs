use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::sleep;
use std::time::Duration;
use std::io;

#[tokio::main]
async fn main() ->  io::Result<()> {
    let remote_addr_initial = "10.100.23.11:33546";

    let mut initial_stream = TcpStream::connect(remote_addr_initial).await?;
    initial_stream.write_all(b"Connect to: 10.100.23.21:20011\0").await?;

    let listener = TcpListener::bind("10.100.23.21:20011").await?;
    let (stream, _addr) = listener.accept().await?;

    let (mut read_half, mut write_half) = stream.into_split();

    let send_task = tokio::spawn( async move {
        loop {
            let _ = write_half.write(b"TCP TEST\0").await;
            sleep(Duration::from_millis(1000)).await;
        }
    });

    let read_task = tokio::spawn( async move {
        let mut buf= [0; 1024];
        loop {
            let n = read_half.read(&mut buf).await.unwrap();

            println!("Got message: {:?}", String::from_utf8_lossy(&buf[..n]));
        }
    });

    let _ = tokio::try_join!(send_task, read_task);

    Ok(())

}