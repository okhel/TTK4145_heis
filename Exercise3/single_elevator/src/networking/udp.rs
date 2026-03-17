use serde::{Serialize, de::DeserializeOwned};
use std::net::{UdpSocket, SocketAddr};

pub const MAGIC: [u8; 4] = *b"EVL1";       // tag to make sure packet is sent from us, kinda redundant might delete later 
pub const VERSION: u8 = 1;               // to counter out of order messages (TODO: upgrade to actual system we made)


// returns bytes sent 
pub fn send_msg<T: Serialize>(
    sock: &UdpSocket,
    addr: impl std::net::ToSocketAddrs,
    msg: &T,
) -> usize {
    let payload = bincode::serialize(msg).expect("bincode serialize failed");

    let mut pkt = Vec::with_capacity(5 + payload.len());
    pkt.extend_from_slice(&MAGIC);
    pkt.push(VERSION);
    pkt.extend_from_slice(&payload);

    sock.send_to(&pkt, addr).expect("udp send_to failed")
}


pub fn recv_msg<T: DeserializeOwned>(
    sock: &UdpSocket,
    buf: &mut [u8],
) -> (T, SocketAddr) {
    let (n, from) = sock.recv_from(buf).expect("udp recv_from failed");
    let data = &buf[..n];

    assert!(data.len() >= 5, "packet too short");
    assert!(data[..4] == MAGIC, "bad magic");
    assert!(data[4] == VERSION, "bad version");

    let msg: T = bincode::deserialize(&data[5..]).expect("bincode deserialize failed");
    (msg, from)
}
