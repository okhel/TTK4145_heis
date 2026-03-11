use crate::networking::types::Msg;
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

const MAX_MSG_BYTES: usize = 65507;
const ACK_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_RETRIES: u32 = 5;

#[derive(Serialize, Deserialize, Debug)]
pub enum Frame {
    Data { seq: u32, msg: Msg },
    Ack { seq: u32, msg: Msg },
}

// single socket reader
pub async fn socket_reader(
    socket: Arc<UdpSocket>,
    data_tx: UnboundedSender<(Msg, u32, SocketAddr)>,
    ack_tx: UnboundedSender<(u32, Msg)>,
) {
    let mut buf = vec![0u8; MAX_MSG_BYTES];
    loop {
        let (len, addr) = match socket.recv_from(&mut buf).await {
            Ok(result) => result,
            Err(e) => {
                eprintln!("Socket read error: {e}");
                continue;
            }
        };

        let frame: Frame = match bincode::deserialize(&buf[..len]) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Deserialize error: {e}");
                continue;
            }
        };

        match frame {
            Frame::Data { seq, msg } => {
                // Send ACK back immediately
                let ack_frame = Frame::Ack { seq, msg: msg.clone() };
                let payload = bincode::serialize(&ack_frame).expect("serialize ack failed");
                let _ = socket.send_to(&payload, addr).await;

                // Forward to data channel
                let _ = data_tx.send((msg, seq, addr));
            }
            Frame::Ack { seq, msg } => {
                // Forward to ack channel
                let _ = ack_tx.send((seq, msg));
            }
        }
    }
}

pub fn create_channels() -> (
    UnboundedReceiver<(Msg, u32, SocketAddr)>,
    UnboundedReceiver<(u32, Msg)>,
    UnboundedSender<(Msg, u32, SocketAddr)>,
    UnboundedSender<(u32, Msg)>,
) {
    let (data_tx, data_rx) = mpsc::unbounded_channel();
    let (ack_tx, ack_rx) = mpsc::unbounded_channel();
    (data_rx, ack_rx, data_tx, ack_tx)
}

pub async fn send_reliable(
    socket: Arc<UdpSocket>,
    msg: Msg,
    addr: SocketAddr,
    seq: u32,
    ack_rx: &mut UnboundedReceiver<(u32, Msg)>,
) -> std::io::Result<()> {
    let frame = Frame::Data { seq, msg: msg.clone() };
    let payload = bincode::serialize(&frame).expect("serialize failed");

    for attempt in 0..MAX_RETRIES {
        socket.send_to(&payload, addr).await?;

        match tokio::time::timeout(ACK_TIMEOUT, wait_for_ack(ack_rx, seq, &msg)).await {
            Ok(true) => return Ok(()),
            Ok(false) => continue,
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

async fn wait_for_ack(
    ack_rx: &mut UnboundedReceiver<(u32, Msg)>,
    seq: u32,
    original_msg: &Msg,
) -> bool {
    while let Some((ack_seq, ack_msg)) = ack_rx.recv().await {
        if ack_seq == seq && ack_msg == *original_msg {
            return true;
        }
    }
    false
}

pub async fn recv_reliable(
    data_rx: &mut UnboundedReceiver<(Msg, u32, SocketAddr)>,
) -> Option<(Msg, u32, SocketAddr)> {
    data_rx.recv().await
}