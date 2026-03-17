use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::UdpSocket;

use crate::USER;

pub fn local_addr(id: u8, port: u16) -> String {
    if USER == "MAC" {
        format!("127.0.0.1:{port}")
    } else {
        format!("10.100.23.{id}:{port}")
    }
}

pub fn random_port_addr(id: u8) -> String {
    if USER == "MAC" {
        "127.0.0.1:0".to_string()
    } else {
        format!("10.100.23.{id}:0")
    }
}

pub fn peer_msg_addr(id: u8) -> SocketAddr {
    if USER == "MAC" {
        format!("127.0.0.1:{}", 21000 + id as u16)
    } else {
        format!("10.100.23.{id}:21000")
    }
    .parse()
    .unwrap()
}

pub fn peer_ping_addr(id: u8) -> SocketAddr {
    if USER == "MAC" {
        format!("127.0.0.1:{}", 30000 + id as u16)
    } else {
        format!("10.100.23.{id}:30000")
    }
    .parse()
    .unwrap()
}

pub async fn bind(addr: &str) -> Arc<UdpSocket> {
    Arc::new(UdpSocket::bind(addr).await.unwrap())
}