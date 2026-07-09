use crate::config::{ScreenAttachments, SettingsData, get_attachments};
use crate::crypto::CryptoKey;
use crate::network::protocol::{ClipboardPayload, ScreenMetrics, true_recv, true_send};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub struct TcpServer {
    running: Arc<Mutex<bool>>,
    clients: Arc<Mutex<Vec<Arc<Mutex<TcpClientInfo>>>>>,
}

pub struct TcpClientInfo {
    pub ip: String,
    pub metrics: ScreenMetrics,
    pub stream: TcpStream,
    pub attachments: ScreenAttachments,
}

impl TcpServer {
    pub fn new() -> Self {
        Self {
            running: Arc::new(Mutex::new(false)),
            clients: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn start<FConn, FDisconn, FClip>(
        &self,
        settings: SettingsData,
        on_connect: FConn,
        on_disconnect: FDisconn,
        on_clipboard_recv: FClip,
    ) -> std::io::Result<()>
    where
        FConn: Fn(String, ScreenMetrics) + Send + Sync + 'static,
        FDisconn: Fn(String) + Send + Sync + 'static,
        FClip: Fn(ClipboardPayload, String) + Send + Sync + 'static,
    {
        let port = 8118;
        let listener = TcpListener::bind(format!("0.0.0.0:{}", port))?;
        listener.set_nonblocking(true)?; // Set non-blocking to allow clean thread exit on stop()
        println!("TCP Server listening on port {}", port);

        let running = self.running.clone();
        {
            let mut r = running.lock().unwrap();
            *r = true;
        }

        let clients_clone = self.clients.clone();
        let key = Arc::new(CryptoKey::new(&settings.password));

        let on_connect = Arc::new(on_connect);
        let on_disconnect = Arc::new(on_disconnect);
        let on_clipboard_recv = Arc::new(on_clipboard_recv);

        thread::spawn(move || {
            loop {
                {
                    let r = running.lock().unwrap();
                    if !*r {
                        break;
                    }
                }

                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let clients_inner = clients_clone.clone();
                        let key_inner = key.clone();
                        let on_conn = on_connect.clone();
                        let on_disc = on_disconnect.clone();
                        let on_clip = on_clipboard_recv.clone();

                        thread::spawn(move || {
                            let ip = match stream.peer_addr() {
                                Ok(addr) => addr.ip().to_string(),
                                Err(_) => return,
                            };

                            println!("TCP: Connection from {}", ip);

                            // Explicitly set stream back to blocking since it was accepted from non-blocking listener
                            stream.set_nonblocking(false).unwrap();

                            stream
                                .set_read_timeout(Some(Duration::from_secs(5)))
                                .unwrap();
                            stream
                                .set_write_timeout(Some(Duration::from_secs(5)))
                                .unwrap();

                            match true_recv(&mut stream, &key_inner) {
                                Ok(bytes) => {
                                    if bytes != b"." {
                                        println!(
                                            "TCP Handshake failed for client {}: mismatch payload",
                                            ip
                                        );
                                        return;
                                    }
                                }
                                Err(e) => {
                                    println!(
                                        "TCP Handshake failed for client {}: decryption error {:?}",
                                        ip, e
                                    );
                                    return;
                                }
                            }

                            if true_send(&mut stream, b".", &key_inner).is_err() {
                                println!("TCP Handshake failed: could not send response to {}", ip);
                                return;
                            }

                            // Handshake succeeded, clear timeouts
                            stream.set_read_timeout(None).unwrap();
                            stream.set_write_timeout(None).unwrap();

                            // Receive screen metrics
                            let metrics_bytes = match true_recv(&mut stream, &key_inner) {
                                Ok(b) => b,
                                Err(_) => return,
                            };

                            let metrics: ScreenMetrics =
                                match serde_json::from_slice(&metrics_bytes) {
                                    Ok(m) => m,
                                    Err(_) => return,
                                };

                            println!("TCP: Client {} metrics: {:?}", ip, metrics);

                            // Load attachments
                            let attachments =
                                get_attachments(&ip).unwrap_or_else(|_| ScreenAttachments {
                                    address: ip.clone(),
                                    top: None,
                                    right: None,
                                    bottom: None,
                                    left: None,
                                });

                            let client_info = Arc::new(Mutex::new(TcpClientInfo {
                                ip: ip.clone(),
                                metrics: metrics.clone(),
                                stream: stream.try_clone().unwrap(),
                                attachments,
                            }));

                            {
                                let mut list = clients_inner.lock().unwrap();
                                list.push(client_info.clone());
                            }

                            on_conn(ip.clone(), metrics);

                            loop {
                                match true_recv(&mut stream, &key_inner) {
                                    Ok(payload_bytes) => {
                                        if payload_bytes.is_empty() {
                                            break;
                                        }
                                        if let Ok(payload) =
                                            serde_json::from_slice::<ClipboardPayload>(
                                                &payload_bytes,
                                            )
                                        {
                                            on_clip(payload, ip.clone());
                                        }
                                    }
                                    Err(_) => {
                                        break;
                                    }
                                }
                            }

                            // Cleanup client
                            println!("TCP: Client {} disconnected", ip);
                            {
                                let mut list = clients_inner.lock().unwrap();
                                list.retain(|c| {
                                    let lock = c.lock().unwrap();
                                    lock.ip != ip
                                });
                            }

                            on_disc(ip);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(100));
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
        let mut r = self.running.lock().unwrap();
        *r = false;
        // Close all clients
        let mut clients = self.clients.lock().unwrap();
        for client in clients.iter() {
            let lock = client.lock().unwrap();
            let _ = lock.stream.shutdown(std::net::Shutdown::Both);
        }
        clients.clear();
    }

    pub fn broadcast_clipboard(
        &self,
        payload: &ClipboardPayload,
        exclude_ip: Option<&str>,
        settings: &SettingsData,
    ) {
        let payload_bytes = serde_json::to_vec(payload).unwrap();
        let key = CryptoKey::new(&settings.password);

        let clients = self.clients.lock().unwrap();
        for client in clients.iter() {
            let mut c = client.lock().unwrap();
            if let Some(exclude) = exclude_ip {
                if c.ip == exclude {
                    continue;
                }
            }
            let _ = true_send(&mut c.stream, &payload_bytes, &key);
        }
    }
}

pub struct TcpClient {
    running: Arc<Mutex<bool>>,
    stream: Arc<Mutex<Option<TcpStream>>>,
}

impl TcpClient {
    pub fn new() -> Self {
        Self {
            running: Arc::new(Mutex::new(false)),
            stream: Arc::new(Mutex::new(None)),
        }
    }

    pub fn connect<FConn, FDisconn, FClip>(
        &self,
        server_ip: &str,
        settings: SettingsData,
        on_connect: FConn,
        on_disconnect: FDisconn,
        on_clipboard_recv: FClip,
    ) -> std::io::Result<()>
    where
        FConn: Fn() + Send + Sync + 'static,
        FDisconn: Fn() + Send + Sync + 'static,
        FClip: Fn(ClipboardPayload) + Send + Sync + 'static,
    {
        let server_addr = format!("{}:8118", server_ip);
        println!("TCP Client connecting to {}...", server_addr);

        let mut stream =
            TcpStream::connect_timeout(&server_addr.parse().unwrap(), Duration::from_secs(5))?;

        let key = CryptoKey::new(&settings.password);

        // Handshake: send '.' and receive '.'
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();

        true_send(&mut stream, b".", &key)?;
        let response = true_recv(&mut stream, &key)?;
        if response != b"." {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "Handshake mismatch",
            ));
        }

        // Handshake succeeded
        stream.set_read_timeout(None).unwrap();
        stream.set_write_timeout(None).unwrap();
        println!("TCP Client connected & authenticated");

        // Send screen metrics
        let screen = crate::hardware::get_screeninfo();
        let metrics = ScreenMetrics {
            width: screen.0,
            height: screen.1,
        };
        let metrics_bytes = serde_json::to_vec(&metrics).unwrap();
        true_send(&mut stream, &metrics_bytes, &key)?;

        {
            let mut s_lock = self.stream.lock().unwrap();
            *s_lock = Some(stream.try_clone().unwrap());
        }

        let running = self.running.clone();
        {
            let mut r = running.lock().unwrap();
            *r = true;
        }

        let on_connect = Arc::new(on_connect);
        let on_disconnect = Arc::new(on_disconnect);
        let on_clipboard_recv = Arc::new(on_clipboard_recv);
        let stream_for_read = stream.try_clone().unwrap();

        on_connect();

        let running_clone = running.clone();
        let key_arc = Arc::new(key);
        let stream_clone = self.stream.clone();

        thread::spawn(move || {
            let mut s = stream_for_read;
            loop {
                {
                    let r = running_clone.lock().unwrap();
                    if !*r {
                        break;
                    }
                }

                match true_recv(&mut s, &key_arc) {
                    Ok(payload_bytes) => {
                        if payload_bytes.is_empty() {
                            break;
                        }
                        if let Ok(payload) =
                            serde_json::from_slice::<ClipboardPayload>(&payload_bytes)
                        {
                            on_clipboard_recv(payload);
                        }
                    }
                    Err(_) => {
                        break;
                    }
                }
            }

            {
                let mut r = running_clone.lock().unwrap();
                *r = false;
            }
            {
                let mut s_lock = stream_clone.lock().unwrap();
                *s_lock = None;
            }
            on_disconnect();
        });

        Ok(())
    }

    pub fn send_clipboard(
        &self,
        payload: &ClipboardPayload,
        settings: &SettingsData,
    ) -> std::io::Result<()> {
        let mut s_lock = self.stream.lock().unwrap();
        if let Some(ref mut s) = *s_lock {
            let payload_bytes = serde_json::to_vec(payload).unwrap();
            let key = CryptoKey::new(&settings.password);
            true_send(s, &payload_bytes, &key)?;
        }
        Ok(())
    }

    pub fn stop(&self) {
        let mut r = self.running.lock().unwrap();
        *r = false;
        let mut s_lock = self.stream.lock().unwrap();
        if let Some(ref s) = *s_lock {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
        *s_lock = None;
    }
}
