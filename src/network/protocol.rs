use crate::crypto::CryptoKey;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpStream;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ScreenMetrics {
    pub width: i32,
    pub height: i32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClipboardFile {
    pub is_dir: bool,
    pub name: String, // Relative path from root
    pub data: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum ClipboardPayload {
    Text { text: String },
    Files { files: Vec<ClipboardFile> },
}

// UDP input event payload
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum InputEvent {
    Move { x: i32, y: i32 },
    MouseClick { button: String, pressed: bool },
    MouseScroll { dx: i32, dy: i32 },
    KeyPress { key: String, pressed: bool },
    Stop,
}

pub fn recv_exactly(stream: &mut TcpStream, n: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0u8; n];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}

pub fn true_send(stream: &mut TcpStream, payload: &[u8], key: &CryptoKey) -> std::io::Result<()> {
    let encrypted = key.encrypt(payload);
    let len_str = format!("{:010}", encrypted.len());
    stream.write_all(len_str.as_bytes())?;
    stream.write_all(&encrypted)?;
    stream.flush()?;
    Ok(())
}

pub fn true_recv(stream: &mut TcpStream, key: &CryptoKey) -> std::io::Result<Vec<u8>> {
    let len_bytes = recv_exactly(stream, 10)?;
    let len_str = std::str::from_utf8(&len_bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let len: usize = len_str
        .parse()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let enc_data = recv_exactly(stream, len)?;
    let decrypted = key.decrypt(&enc_data);
    Ok(decrypted)
}
