use crate::networking::types::Msg;
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::net::UdpSocket;

const MAX_MSG_BYTES: usize = 65507;
const ACK_TIMEOUT: Duration = Duration::from_millis(5);
const MAX_RETRIES: u32 = 5;

#[derive(Serialize, Deserialize, Debug)]
pub enum Frame {
    Data { seq: u32, msg: Msg },
    Ack { seq: u32, msg: Msg },
}

pub async fn send_reliable(
    socket: Arc<UdpSocket>,
    msg: Msg,
    addr: SocketAddr,
    seq: u32,
) -> std::io::Result<()> {
    let frame = Frame::Data { seq, msg: msg.clone() };
    let payload = bincode::serialize(&frame).expect("serialize failed");
    for _attempt in 0..MAX_RETRIES {
        if socket.send_to(&payload, addr).await.is_err() {
            tokio::time::sleep(ACK_TIMEOUT).await;
            continue;
        }
        match tokio::time::timeout(ACK_TIMEOUT, recv_ack(&socket, seq, &msg)).await {
            Ok(true) => return Ok(()),
            _ => continue,
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
        let (len, addr) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        let frame: Frame = match bincode::deserialize(&buf[..len]) {
            Ok(f) => f,
            Err(_) => continue,
        };
        match frame {
            Frame::Data { seq, msg } => {
                let _ = send_ack(socket, seq, &msg, addr).await;
                return Ok((msg, seq, addr));
            }
            Frame::Ack { .. } => {
                continue;
            }
        }
    }
}

const ACK_COPIES: usize = 5;

async fn send_ack(socket: &UdpSocket, seq: u32, msg: &Msg, addr: SocketAddr) {
    let frame = Frame::Ack {
        seq,
        msg: msg.clone(),
    };
    let payload = bincode::serialize(&frame).expect("serialize ack failed");
    for _ in 0..ACK_COPIES {
        let _ = socket.send_to(&payload, addr).await;
    }
}

async fn recv_ack(socket: &UdpSocket, seq: u32, original_msg: &Msg) -> bool {
    let mut buf = vec![0u8; MAX_MSG_BYTES];
    loop {
        let (len, _) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(_) => return false,
        };
        let frame: Frame = match bincode::deserialize(&buf[..len]) {
            Ok(f) => f,
            Err(_) => continue,
        };
        match frame {
            Frame::Ack { seq: s, msg } if s == seq && msg == *original_msg => return true,
            _ => continue,
        }
    }
}
