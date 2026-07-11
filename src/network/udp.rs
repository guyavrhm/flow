use crate::config::SettingsData;
use crate::crypto::CryptoKey;
use crate::hardware::{KeyboardController, MouseController};
use crate::network::protocol::InputEvent;
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Clone)]
pub struct UdpServer {
    socket: Arc<UdpSocket>,
}

impl UdpServer {
    pub fn new() -> std::io::Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:8118")?;
        Ok(Self {
            socket: Arc::new(socket),
        })
    }

    pub fn listen_handshake(
        &self,
        client_ip: &str,
        settings: &SettingsData,
    ) -> std::io::Result<std::net::SocketAddr> {
        log::debug!("UDP Server: Listening for handshake from {}", client_ip);
        self.socket.set_read_timeout(Some(Duration::from_secs(3)))?;
        let key = CryptoKey::new(&settings.password);

        let mut buf = [0u8; 1024];
        let start_time = std::time::Instant::now();

        while start_time.elapsed() < Duration::from_secs(3) {
            match self.socket.recv_from(&mut buf) {
                Ok((len, addr)) => {
                    if addr.ip().to_string() == client_ip {
                        let decrypted = key.decrypt(&buf[..len]);
                        if decrypted == b"." {
                            self.socket.set_read_timeout(None)?;
                            log::info!("UDP Handshake: Succeeded for client endpoint: {}", addr);
                            return Ok(addr);
                        }
                    }
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    break;
                }
                Err(_) => {}
            }
        }

        self.socket.set_read_timeout(None)?;
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "UDP handshake timed out",
        ))
    }

    pub fn send_event(
        &self,
        event: &InputEvent,
        dest: std::net::SocketAddr,
        settings: &SettingsData,
    ) -> std::io::Result<()> {
        log::trace!("UDP Server: Sending event {:?} to {}", event, dest);
        let payload = format_event(event);
        let key = CryptoKey::new(&settings.password);
        let encrypted = key.encrypt(payload.as_bytes());
        self.socket.send_to(&encrypted, dest)?;
        Ok(())
    }
}

pub struct UdpClient {
    running: Arc<Mutex<bool>>,
    socket: Arc<Mutex<Option<UdpSocket>>>,
}

impl UdpClient {
    pub fn new() -> Self {
        Self {
            running: Arc::new(Mutex::new(false)),
            socket: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start(&self, server_ip: &str, settings: SettingsData) -> std::io::Result<()> {
        log::info!("UDP Client connecting to {}...", server_ip);
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.set_read_timeout(Some(Duration::from_millis(500)))?;

        let server_dest = format!("{}:8118", server_ip);

        let key = CryptoKey::new(&settings.password);
        let encrypted = key.encrypt(b".");

        // Send handshake packet
        socket.send_to(&encrypted, &server_dest)?;
        log::info!("UDP Client sent handshake to {}", server_dest);

        {
            let mut s = self.socket.lock().unwrap();
            *s = Some(socket.try_clone().unwrap());
        }

        let running = self.running.clone();
        {
            let mut r = running.lock().unwrap();
            *r = true;
        }

        let socket_clone = socket.try_clone().unwrap();
        let running_clone = running.clone();

        thread::spawn(move || {
            let mouse = MouseController::new();
            let keyboard = KeyboardController::new();
            let key = CryptoKey::new(&settings.password);
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
                        let decrypted = key.decrypt(&buf[..len]);
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
                                        log::info!("UDP Client: Received stop command from server");
                                    }
                                }
                            } else {
                                log::warn!("UDP Client: Received unparseable payload: {}", dec_str);
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
    }
}

pub(crate) fn format_event(event: &InputEvent) -> String {
    match event {
        InputEvent::Move { x, y } => format!("mov {} {}", x, y),
        InputEvent::MouseScroll { dx, dy } => format!("scrl {} {}", dx, dy),
        InputEvent::MouseClick { button, pressed } => format!("prsm {} {}", pressed, button),
        InputEvent::KeyPress { key, pressed } => format!("prsk {} {}", pressed, key),
        InputEvent::Stop => "stp".to_string(),
    }
}

pub(crate) fn parse_event(s: &str) -> Option<InputEvent> {
    let parts: Vec<&str> = s.split_whitespace().collect();
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
