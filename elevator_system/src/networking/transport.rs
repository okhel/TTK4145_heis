use crate::networking::types::Msg;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;

const MAX_MSG_BYTES: usize = 65507;
const ACK_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_RETRIES: u32 = 5;

#[derive(Serialize, Deserialize, Debug)]
pub enum Frame {
    Data { seq: u32, msg: Msg },
    Ack { seq: u32, msg: Msg },
}

pub async fn send_reliable(
    socket: &UdpSocket,
    msg: &Msg,
    addr: SocketAddr,
    seq: u32,
) -> std::io::Result<()> {
    let frame = Frame::Data {
        seq,
        msg: msg.clone(),
    };
    let payload = bincode::serialize(&frame).expect("serialize failed");
    for attempt in 0..MAX_RETRIES {
        socket.send_to(&payload, addr).await?;
        match tokio::time::timeout(ACK_TIMEOUT, recv_ack(socket, seq)).await {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                eprintln!(
                    "ACK timeout for seq={seq}, attempt {}/{MAX_RETRIES}",
                    attempt + 1
                );
            }
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!("no ACK after {MAX_RETRIES} attempts"),
    ))
}

pub async fn recv_reliable(socket: &UdpSocket) -> std::io::Result<(Msg, u32, SocketAddr)> {
    let mut buf = vec![0u8; MAX_MSG_BYTES];
    loop {
        let (len, addr) = socket.recv_from(&mut buf).await?;
        let frame: Frame = bincode::deserialize(&buf[..len])
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        match frame {
            Frame::Data { seq, msg } => {
                send_ack(socket, seq, &msg, addr).await?;
                return Ok((msg, seq, addr));
            }
            Frame::Ack { .. } => {
                continue;
            }
        }
    }
}

async fn send_ack(
    socket: &UdpSocket,
    seq: u32,
    msg: &Msg,
    addr: SocketAddr,
) -> std::io::Result<()> {
    let frame = Frame::Ack {
        seq,
        msg: msg.clone(),
    };
    let payload = bincode::serialize(&frame).expect("serialize ack failed");
    socket.send_to(&payload, addr).await?;
    Ok(())
}

async fn recv_ack(socket: &UdpSocket, expected_seq: u32) -> std::io::Result<()> {
    let mut buf = vec![0u8; MAX_MSG_BYTES];
    loop {
        let (len, _) = socket.recv_from(&mut buf).await?;
        if let Ok(Frame::Ack { seq, .. }) = bincode::deserialize(&buf[..len]) {
            if seq == expected_seq {
                return Ok(());
            }
        }
    }
}
