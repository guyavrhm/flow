use crate::hardware::{KeyboardController, MouseController};
use crate::network::protocol::InputEvent;
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Clone)]
pub struct UdpServer {
    pub(crate) socket: Arc<UdpSocket>,
    handshake_lock: Arc<Mutex<()>>,
}

impl UdpServer {
    pub fn from_socket(socket: UdpSocket) -> Self {
        Self {
            socket: Arc::new(socket),
            handshake_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn new() -> std::io::Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:8118")?;
        Ok(Self {
            socket: Arc::new(socket),
            handshake_lock: Arc::new(Mutex::new(())),
        })
    }

    pub fn listen_handshake(
        &self,
        client_ip: &str,
        cryptor: &crate::crypto::UdpCryptor,
    ) -> std::io::Result<std::net::SocketAddr> {
        let _lock = self.handshake_lock.lock().unwrap();
        log::info!("[UDP] Server waiting for handshake pulse from client [{}]...", client_ip);
        self.socket.set_read_timeout(Some(Duration::from_millis(150)))?;

        let mut buf = [0u8; 1024];
        let start_time = std::time::Instant::now();
        let mut result = Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("UDP handshake timed out waiting for client [{}]", client_ip),
        ));

        while start_time.elapsed() < Duration::from_secs(3) {
            match self.socket.recv_from(&mut buf) {
                Ok((len, addr)) => {
                    if addr.ip().to_string() == client_ip {
                        if len > 8 {
                            let mut seq_bytes = [0u8; 8];
                            seq_bytes.copy_from_slice(&buf[0..8]);
                            let seq = u64::from_be_bytes(seq_bytes);

                            if let Ok(decrypted) = cryptor.decrypt(seq, &buf[8..len]) {
                                if decrypted == b"." {
                                    // Send ACK back to client endpoint
                                    if let Ok(ack_cipher) = cryptor.encrypt(0, b"ACK") {
                                        let mut ack_packet = Vec::with_capacity(8 + ack_cipher.len());
                                        ack_packet.extend_from_slice(&0u64.to_be_bytes());
                                        ack_packet.extend_from_slice(&ack_cipher);
                                        // Send twice for packet loss tolerance
                                        let _ = self.socket.send_to(&ack_packet, addr);
                                        let _ = self.socket.send_to(&ack_packet, addr);
                                    }
                                    log::info!("[UDP] Handshake verified: Client [{}] matched endpoint {}, sent ACK", client_ip, addr);
                                    result = Ok(addr);
                                    break;
                                }
                            }
                        }
                    }
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    // Small timeout slice to allow cooperative cancellation
                    continue;
                }
                Err(_) => {}
            }
        }

        let _ = self.socket.set_read_timeout(None);
        if result.is_err() {
            log::warn!("[UDP] Handshake timed out waiting for pulse from client [{}]", client_ip);
        }
        result
    }

    pub fn send_event(
        &self,
        event: &InputEvent,
        dest: std::net::SocketAddr,
        cryptor: &crate::crypto::UdpCryptor,
        seq: u64,
    ) -> std::io::Result<()> {
        log::trace!("UDP Server: Sending event {:?} to {}", event, dest);
        let payload = format_event(event);
        let ciphertext = cryptor.encrypt(seq, payload.as_bytes())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        let mut packet = Vec::with_capacity(8 + ciphertext.len());
        packet.extend_from_slice(&seq.to_be_bytes());
        packet.extend_from_slice(&ciphertext);

        self.socket.send_to(&packet, dest)?;
        Ok(())
    }
}

pub struct UdpClient {
    running: Arc<Mutex<bool>>,
    socket: Arc<Mutex<Option<UdpSocket>>>,
    cryptor: Arc<Mutex<Option<crate::crypto::UdpCryptor>>>,
}

impl UdpClient {
    pub fn new() -> Self {
        Self {
            running: Arc::new(Mutex::new(false)),
            socket: Arc::new(Mutex::new(None)),
            cryptor: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start(&self, server_ip: &str, key: [u8; 32], salt: [u8; 4]) -> std::io::Result<()> {
        log::info!("UDP Client connecting to {}...", server_ip);
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.set_read_timeout(Some(Duration::from_millis(500)))?;

        let server_dest = format!("{}:8118", server_ip);

        let cryptor = crate::crypto::UdpCryptor::new(&key, salt);

        // Encrypt handshake packet with sequence 0
        let ciphertext = cryptor.encrypt(0, b".")
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        let mut packet = Vec::with_capacity(8 + ciphertext.len());
        packet.extend_from_slice(&0u64.to_be_bytes());
        packet.extend_from_slice(&ciphertext);

        log::info!("[UDP] Client initiating handshake with server at {}...", server_dest);
        socket.set_read_timeout(Some(Duration::from_millis(100)))?;

        let mut ack_received = false;
        let mut resp_buf = [0u8; 1024];

        for attempt in 1..=60 {
            let _ = socket.send_to(&packet, &server_dest);

            match socket.recv_from(&mut resp_buf) {
                Ok((len, _)) if len > 8 => {
                    let mut seq_bytes = [0u8; 8];
                    seq_bytes.copy_from_slice(&resp_buf[0..8]);
                    let seq = u64::from_be_bytes(seq_bytes);

                    if let Ok(decrypted) = cryptor.decrypt(seq, &resp_buf[8..len]) {
                        if decrypted == b"ACK" {
                            log::info!("[UDP] Client received ACK from server on attempt #{}. Handshake confirmed!", attempt);
                            ack_received = true;
                            break;
                        }
                    }
                }
                _ => {}
            }
        }

        if !ack_received {
            log::error!("[UDP] Client failed to receive handshake ACK from server at {}", server_dest);
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("Failed to receive UDP ACK from server [{}] after 60 attempts", server_ip),
            ));
        }

        socket.set_read_timeout(Some(Duration::from_millis(500)))?;

        {
            let mut s = self.socket.lock().unwrap();
            *s = Some(socket.try_clone().unwrap());
        }
        {
            let mut c = self.cryptor.lock().unwrap();
            *c = Some(cryptor);
        }

        let running = self.running.clone();
        {
            let mut r = running.lock().unwrap();
            *r = true;
        }

        let socket_clone = socket.try_clone().unwrap();
        let running_clone = running.clone();
        let cryptor_clone = self.cryptor.clone();

        thread::spawn(move || {
            let mouse = MouseController::new();
            let keyboard = KeyboardController::new();
            let mut replay_protector = crate::crypto::UdpReplayProtector::new();
            let mut buf = [0u8; 1024];

            loop {
                {
                    let r = running_clone.lock().unwrap();
                    if !*r {
                        break;
                    }
                }

                match socket_clone.recv_from(&mut buf) {
                    Ok((len, _)) => {
                        if len <= 8 {
                            continue;
                        }

                        let mut seq_bytes = [0u8; 8];
                        seq_bytes.copy_from_slice(&buf[0..8]);
                        let seq = u64::from_be_bytes(seq_bytes);

                        if !replay_protector.is_valid(seq) {
                            log::warn!("UDP Client: Replay protector rejected sequence {}", seq);
                            continue;
                        }

                        let crypt_opt = cryptor_clone.lock().unwrap();
                        if let Some(ref c) = *crypt_opt {
                            if let Ok(decrypted) = c.decrypt(seq, &buf[8..len]) {
                                if let Ok(dec_str) = std::str::from_utf8(&decrypted) {
                                    if let Some(event) = parse_event(dec_str) {
                                        log::trace!("UDP Client: Received event {:?}", event);
                                        match event {
                                            InputEvent::Move { x, y } => {
                                                mouse.set_position((x, y));
                                            }
                                            InputEvent::MouseScroll { dx, dy } => {
                                                mouse.scroll(dx, dy);
                                            }
                                            InputEvent::MouseClick { button, pressed } => {
                                                if pressed {
                                                    mouse.press(&button);
                                                } else {
                                                    mouse.release(&button);
                                                }
                                            }
                                            InputEvent::KeyPress { key: k, pressed } => {
                                                if pressed {
                                                    keyboard.press(&k);
                                                } else {
                                                    keyboard.release(&k);
                                                }
                                            }
                                            InputEvent::Stop => {
                                                log::info!("UDP Client: Received stop command from server - releasing inputs");
                                                mouse.release("left");
                                                mouse.release("right");
                                                mouse.release("middle");
                                                mouse.release("x1");
                                                mouse.release("x2");
                                                keyboard.release_all();
                                            }
                                        }
                                    } else {
                                        log::warn!("UDP Client: Received unparseable payload: {}", dec_str);
                                    }
                                }
                            } else {
                                log::warn!("UDP Client: Failed to decrypt UDP packet with sequence {}", seq);
                            }
                        }
                    }
                    Err(ref e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            || e.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        // Timeout: loop again to check running flag
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    pub fn stop(&self) {
        log::info!("Stopping UDP Client");
        let mut r = self.running.lock().unwrap();
        *r = false;
        let mut s = self.socket.lock().unwrap();
        *s = None;
        let mut c = self.cryptor.lock().unwrap();
        *c = None;
    }
}

pub fn format_event(event: &InputEvent) -> String {
    match event {
        InputEvent::Move { x, y } => format!("mov {} {}", x, y),
        InputEvent::MouseScroll { dx, dy } => format!("scrl {} {}", dx, dy),
        InputEvent::MouseClick { button, pressed } => format!("prsm {} {}", pressed, button),
        InputEvent::KeyPress { key, pressed } => format!("prsk {} {}", pressed, key),
        InputEvent::Stop => "stp".to_string(),
    }
}

pub fn parse_event(s: &str) -> Option<InputEvent> {
    let parts: Vec<&str> = s.splitn(3, ' ').collect();
    if parts.is_empty() {
        return None;
    }
    match parts[0] {
        "mov" if parts.len() == 3 => {
            let x = parts[1].parse().ok()?;
            let y = parts[2].parse().ok()?;
            Some(InputEvent::Move { x, y })
        }
        "scrl" if parts.len() == 3 => {
            let dx = parts[1].parse().ok()?;
            let dy = parts[2].parse().ok()?;
            Some(InputEvent::MouseScroll { dx, dy })
        }
        "prsm" if parts.len() == 3 => {
            let pressed = parts[1].parse().ok()?;
            let button = parts[2].replace("Button.", "");
            Some(InputEvent::MouseClick { button, pressed })
        }
        "prsk" if parts.len() == 3 => {
            let pressed = parts[1].parse().ok()?;
            let key = parts[2].to_string();
            Some(InputEvent::KeyPress { key, pressed })
        }
        "stp" => Some(InputEvent::Stop),
        _ => None,
    }
}
