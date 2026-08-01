use crate::config::{ScreenAttachments, SettingsData, get_attachments};
use crate::network::protocol::{ClipboardPayload, ScreenMetrics, UdpSessionConfig, true_recv, true_send};
use crate::network::tls::{PendingTrustRequest, TofuClientVerifier, TofuServerVerifier, load_certs_and_key};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use rand::Rng;
use rustls::{ServerConfig, ClientConfig};

pub struct TcpServer {
    pub(crate) running: Arc<Mutex<bool>>,
    pub clients: Arc<Mutex<Vec<Arc<Mutex<TcpClientInfo>>>>>,
}

pub struct TcpClientInfo {
    pub ip: String,
    pub metrics: ScreenMetrics,
    pub stream: Arc<Mutex<rustls::StreamOwned<rustls::ServerConnection, TcpStream>>>,
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
        pending_trusts: Arc<Mutex<Vec<PendingTrustRequest>>>,
        on_connect: FConn,
        on_disconnect: FDisconn,
        on_clipboard_recv: FClip,
    ) -> std::io::Result<()>
    where
        FConn: Fn(String, ScreenMetrics, crate::crypto::UdpCryptor, u64) + Send + Sync + 'static,
        FDisconn: Fn(String, u64) + Send + Sync + 'static,
        FClip: Fn(ClipboardPayload, String) + Send + Sync + 'static,
    {
        let port = 8118;
        let listener = TcpListener::bind(format!("0.0.0.0:{}", port))?;
        listener.set_nonblocking(true)?; // Set non-blocking to allow clean thread exit on stop()
        log::info!("TCP Server listening on port {}", port);

        let running = self.running.clone();
        {
            let mut r = running.lock().unwrap();
            *r = true;
        }

        // Install the ring provider as process default if not already done
        let _ = rustls::crypto::ring::default_provider().install_default();

        let clients_clone = self.clients.clone();
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
                    Ok((stream, _)) => {
                        let clients_inner = clients_clone.clone();
                        let on_conn = on_connect.clone();
                        let on_disc = on_disconnect.clone();
                        let on_clip = on_clipboard_recv.clone();
                        let pending_trusts_inner = pending_trusts.clone();
                        let running_inner = running.clone();

                        thread::spawn(move || {
                            let ip = match stream.peer_addr() {
                                Ok(addr) => addr.ip().to_string(),
                                Err(_) => return,
                            };

                            log::info!("TCP: Connection from {}", ip);

                            // Explicitly set stream back to blocking since it was accepted from non-blocking listener
                            stream.set_nonblocking(false).unwrap();

                            stream
                                .set_read_timeout(Some(Duration::from_secs(5)))
                                .unwrap();
                            stream
                                .set_write_timeout(Some(Duration::from_secs(5)))
                                .unwrap();

                            // Load server's self-signed cert and key
                            let (cert_pem, key_pem) = match crate::crypto::load_or_generate_cert(crate::paths::get_app_dir()) {
                                Ok(pair) => pair,
                                Err(e) => {
                                    log::error!("TCP Server: Failed to load/generate cert: {:?}", e);
                                    return;
                                }
                            };
                            let (certs, private_key) = load_certs_and_key(&cert_pem, &key_pem);

                            // Build server config dynamically using client's IP to support mTLS TOFU verification
                            let client_verifier = Arc::new(TofuClientVerifier::new(ip.clone(), pending_trusts_inner));
                            let server_config = match ServerConfig::builder()
                                .with_client_cert_verifier(client_verifier)
                                .with_single_cert(certs, private_key)
                            {
                                Ok(cfg) => cfg,
                                Err(e) => {
                                    log::error!("TCP Server: Failed to build ServerConfig: {:?}", e);
                                    return;
                                }
                            };

                            // Establish TLS Server session
                            let conn = match rustls::ServerConnection::new(Arc::new(server_config)) {
                                Ok(c) => c,
                                Err(e) => {
                                    log::error!("TCP Server: Failed to create ServerConnection: {:?}", e);
                                    return;
                                }
                            };

                            let mut stream_owned = rustls::StreamOwned::new(conn, stream);

                            // Generate ephemeral key & salt for UDP events encryption
                            let mut udp_key = [0u8; 32];
                            let mut udp_salt = [0u8; 4];
                            rand::thread_rng().fill(&mut udp_key);
                            rand::thread_rng().fill(&mut udp_salt);

                            let cryptor = crate::crypto::UdpCryptor::new(&udp_key, udp_salt);
                            let connection_id: u64 = rand::random();

                            // Send UDP session key config to client over TLS
                            let session_config = UdpSessionConfig {
                                key: udp_key,
                                salt: udp_salt,
                            };
                            let config_bytes = serde_json::to_vec(&session_config).unwrap();
                            if let Err(e) = true_send(&mut stream_owned, &config_bytes) {
                                log::warn!("TCP Server: Failed to send UDP session config to {}: {:?}", ip, e);
                                return;
                            }

                            // Receive client's ScreenMetrics
                            let metrics_bytes = match true_recv(&mut stream_owned) {
                                Ok(b) => b,
                                Err(e) => {
                                    log::warn!("TCP Server: Failed to receive metrics from {}: {:?}", ip, e);
                                    return;
                                }
                            };

                            let metrics: ScreenMetrics = match serde_json::from_slice(&metrics_bytes) {
                                Ok(m) => m,
                                Err(e) => {
                                    log::warn!("TCP Server: Failed to parse metrics from {}: {:?}", ip, e);
                                    return;
                                }
                            };

                            // Handshake succeeded. Set a short read timeout (100ms) on the stream
                            // to support concurrent locking for reads & writes.
                            stream_owned.get_mut().set_read_timeout(Some(Duration::from_millis(100))).unwrap();
                            stream_owned.get_mut().set_write_timeout(None).unwrap();

                            log::info!("TCP Server: Client {} authenticated & verified (conn_id: {})", ip, connection_id);

                            // Load attachments
                            let attachments =
                                get_attachments(&ip).unwrap_or_else(|_| ScreenAttachments {
                                    address: ip.clone(),
                                    top: None,
                                    right: None,
                                    bottom: None,
                                    left: None,
                                });

                            let stream_owned_arc = Arc::new(Mutex::new(stream_owned));

                            let client_info = Arc::new(Mutex::new(TcpClientInfo {
                                ip: ip.clone(),
                                metrics: metrics.clone(),
                                stream: stream_owned_arc.clone(),
                                attachments,
                            }));

                            {
                                let mut list = clients_inner.lock().unwrap();
                                list.push(client_info.clone());
                            }

                            on_conn(ip.clone(), metrics, cryptor, connection_id);

                            loop {
                                {
                                    let r = running_inner.lock().unwrap();
                                    if !*r {
                                        break;
                                    }
                                }

                                let mut s_lock = stream_owned_arc.lock().unwrap();
                                match true_recv(&mut *s_lock) {
                                    Ok(payload_bytes) => {
                                        if payload_bytes.is_empty() {
                                            log::debug!("TCP Server: Received empty payload (EOF) from {}", ip);
                                            break;
                                        }
                                        log::debug!("TCP Server: Received clipboard payload from {}", ip);
                                        if let Ok(payload) =
                                            serde_json::from_slice::<ClipboardPayload>(
                                                &payload_bytes,
                                            )
                                        {
                                            drop(s_lock); // Drop lock before invoking callback
                                            on_clip(payload, ip.clone());
                                        }
                                    }
                                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                                        drop(s_lock);
                                        thread::sleep(Duration::from_millis(10));
                                    }
                                    Err(e) => {
                                        log::debug!("TCP Server: Read error from {}: {:?}", ip, e);
                                        break;
                                    }
                                }
                            }

                            // Cleanup client
                            log::info!("TCP: Client {} disconnected (conn_id: {})", ip, connection_id);
                            {
                                let mut list = clients_inner.lock().unwrap();
                                list.retain(|c| {
                                    !Arc::ptr_eq(c, &client_info)
                                });
                            }

                            on_disc(ip, connection_id);
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
        log::info!("Stopping TCP Server");
        let mut r = self.running.lock().unwrap();
        *r = false;
        // Close all clients
        let mut clients = self.clients.lock().unwrap();
        for client in clients.iter() {
            let lock = client.lock().unwrap();
            let mut s = lock.stream.lock().unwrap();
            let _ = s.get_mut().shutdown(std::net::Shutdown::Both);
        }
        clients.clear();
    }

    pub fn broadcast_clipboard(
        &self,
        payload: &ClipboardPayload,
        exclude_ip: Option<&str>,
    ) {
        log::debug!("TCP: Broadcasting clipboard (excluding client: {:?})", exclude_ip);
        let payload_bytes = serde_json::to_vec(payload).unwrap();

        let clients = self.clients.lock().unwrap();
        for client in clients.iter() {
            let c = client.lock().unwrap();
            if let Some(exclude) = exclude_ip {
                if c.ip == exclude {
                    continue;
                }
            }
            let mut s = c.stream.lock().unwrap();
            let _ = true_send(&mut *s, &payload_bytes);
        }
    }
}

pub struct TcpClient {
    running: Arc<Mutex<bool>>,
    stream: Arc<Mutex<Option<Arc<Mutex<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>>>>>,
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
        _settings: SettingsData,
        pending_trusts: Arc<Mutex<Vec<PendingTrustRequest>>>,
        on_connect: FConn,
        on_disconnect: FDisconn,
        on_clipboard_recv: FClip,
    ) -> std::io::Result<()>
    where
        FConn: Fn([u8; 32], [u8; 4]) + Send + Sync + 'static,
        FDisconn: Fn() + Send + Sync + 'static,
        FClip: Fn(ClipboardPayload) + Send + Sync + 'static,
    {
        let server_addr = format!("{}:8118", server_ip);
        log::info!("TCP Client connecting to {}...", server_addr);

        let stream =
            TcpStream::connect_timeout(&server_addr.parse().unwrap(), Duration::from_secs(5))?;

        // Install the ring provider as process default if not already done
        let _ = rustls::crypto::ring::default_provider().install_default();

        // Load client's self-signed cert and key
        let (cert_pem, key_pem) = match crate::crypto::load_or_generate_cert(crate::paths::get_app_dir()) {
            Ok(pair) => pair,
            Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
        };
        let (certs, private_key) = load_certs_and_key(&cert_pem, &key_pem);

        // Build client configuration with custom TOFU server verifier and client certificate for mTLS
        let server_verifier = Arc::new(TofuServerVerifier::new(server_ip.to_string(), pending_trusts));
        let client_config = match ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(server_verifier)
            .with_client_auth_cert(certs, private_key)
        {
            Ok(cfg) => cfg,
            Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
        };

        let server_name = match rustls_pki_types::ServerName::try_from(server_ip.to_string()) {
            Ok(name) => name.to_owned(),
            Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string())),
        };

        let conn = match rustls::ClientConnection::new(Arc::new(client_config), server_name) {
            Ok(c) => c,
            Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
        };

        let mut stream_owned = rustls::StreamOwned::new(conn, stream);

        stream_owned.get_mut().set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        stream_owned.get_mut().set_write_timeout(Some(Duration::from_secs(5))).unwrap();

        // Receive UDP session key config from server over TLS
        let session_bytes = true_recv(&mut stream_owned)?;
        let session_config: UdpSessionConfig = serde_json::from_slice(&session_bytes)?;

        // Send screen metrics to server
        let screen = crate::hardware::get_screeninfo();
        let monitors = crate::hardware::get_monitors();
        let uses_physical_pixels = cfg!(not(target_os = "macos"));
        let metrics = ScreenMetrics {
            width: screen.0,
            height: screen.1,
            monitors,
            uses_physical_pixels,
        };
        let metrics_bytes = serde_json::to_vec(&metrics).unwrap();
        true_send(&mut stream_owned, &metrics_bytes)?;

        // Handshake succeeded. Set a short read timeout (100ms) on client socket
        // to support cooperative concurrent locking for reads & writes.
        stream_owned.get_mut().set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        stream_owned.get_mut().set_write_timeout(None).unwrap();
        log::info!("TCP Client connected & authenticated via TLS");

        let stream_owned_arc = Arc::new(Mutex::new(stream_owned));

        {
            let mut s_lock = self.stream.lock().unwrap();
            *s_lock = Some(stream_owned_arc.clone());
        }

        let running = self.running.clone();
        {
            let mut r = running.lock().unwrap();
            *r = true;
        }

        let on_connect = Arc::new(on_connect);
        let on_disconnect = Arc::new(on_disconnect);
        let on_clipboard_recv = Arc::new(on_clipboard_recv);

        on_connect(session_config.key, session_config.salt);

        let running_clone = running.clone();
        let stream_clone = self.stream.clone();

        thread::spawn(move || {
            loop {
                {
                    let r = running_clone.lock().unwrap();
                    if !*r {
                        break;
                    }
                }

                let stream_opt = {
                    let s_lock = stream_clone.lock().unwrap();
                    s_lock.clone()
                };

                if let Some(s_arc) = stream_opt {
                    let mut s = s_arc.lock().unwrap();
                    match true_recv(&mut *s) {
                        Ok(payload_bytes) => {
                            if payload_bytes.is_empty() {
                                log::debug!("TCP Client: Received empty payload (EOF) from server");
                                break;
                            }
                            log::debug!("TCP Client: Received clipboard payload bytes from server");
                            // Drop lock before calling callback to avoid deadlocks
                            drop(s);
                            if let Ok(payload) =
                                serde_json::from_slice::<ClipboardPayload>(&payload_bytes)
                            {
                                on_clipboard_recv(payload);
                            }
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {
                            drop(s);
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(e) => {
                            log::debug!("TCP Client: Read error: {:?}", e);
                            break;
                        }
                    }
                } else {
                    break;
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
    ) -> std::io::Result<()> {
        log::debug!("TCP Client: Sending clipboard payload to server");
        let s_opt = {
            let s_lock = self.stream.lock().unwrap();
            s_lock.clone()
        };
        if let Some(s_arc) = s_opt {
            let mut s = s_arc.lock().unwrap();
            let payload_bytes = serde_json::to_vec(payload).unwrap();
            true_send(&mut *s, &payload_bytes)?;
        }
        Ok(())
    }

    pub fn stop(&self) {
        log::info!("Stopping TCP Client");
        let mut r = self.running.lock().unwrap();
        *r = false;
        let s_opt = {
            let mut s_lock = self.stream.lock().unwrap();
            s_lock.take()
        };
        if let Some(s_arc) = s_opt {
            let mut s = s_arc.lock().unwrap();
            let _ = s.get_mut().shutdown(std::net::Shutdown::Both);
        }
    }
}
