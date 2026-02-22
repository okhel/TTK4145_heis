use tokio::{
    net::TcpStream,
    io::{AsyncReadExt, AsyncWriteExt},
};
use crate::networking::types::Msg;

pub const MAX_MSG_BYTES: usize = 64 * 1024;

// length-prefixed framing, can switch to \n instead if we want 

pub async fn send_msg(stream: &mut TcpStream, msg: &Msg) -> std::io::Result<()> {
    let payload = bincode::serialize(msg).expect("serialize failed");
    let len = payload.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(&payload).await?;
    Ok(())
}

pub async fn recv_msg(stream: &mut TcpStream) -> std::io::Result<Msg> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_MSG_BYTES {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "msg too large"));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    let msg = bincode::deserialize(&buf).expect("deserialize failed");
    Ok(msg)
}

// same as recv_msg but reads from a BufReader over a split TCP read half
pub async fn recv_from_buf(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>,
) -> std::io::Result<Msg> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_MSG_BYTES {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "msg too large"));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    let msg = bincode::deserialize(&buf).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
    })?;
    Ok(msg)
}