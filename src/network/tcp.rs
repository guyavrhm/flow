use crate::config::{ScreenAttachments, SettingsData, get_attachments};
use crate::network::protocol::{ClipboardPayload, ScreenMetrics, UdpSessionConfig, true_recv_async, true_send_async};
use crate::network::tls::{PendingTrustRequest, TofuClientVerifier, TofuServerVerifier, load_certs_and_key};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use rand::Rng;
use rustls::{ServerConfig, ClientConfig};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::TlsConnector;

pub struct TcpServer {
    pub(crate) running: Arc<Mutex<bool>>,
    pub clients: Arc<Mutex<Vec<Arc<Mutex<TcpClientInfo>>>>>,
    shutdown_tx: Arc<Mutex<Option<tokio::sync::broadcast::Sender<()>>>>,
}

pub struct TcpClientInfo {
    pub ip: String,
    pub metrics: ScreenMetrics,
    pub stream: Arc<tokio::sync::Mutex<tokio_rustls::server::TlsStream<TcpStream>>>,
    pub attachments: ScreenAttachments,
}

impl TcpServer {
    pub fn new() -> Self {
        Self {
            running: Arc::new(Mutex::new(false)),
            clients: Arc::new(Mutex::new(Vec::new())),
            shutdown_tx: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start<FConn, FDisconn, FClip>(
        &self,
        _settings: SettingsData,
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

        let (tx, rx) = tokio::sync::broadcast::channel(1);
        {
            let mut s_lock = self.shutdown_tx.lock().unwrap();
            *s_lock = Some(tx);
        }

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
        let running_clone = running.clone();

        crate::network::TOKIO_RUNTIME.spawn(async move {
                let listener = match TcpListener::bind(format!("0.0.0.0:{}", port)).await {
                    Ok(l) => l,
                    Err(e) => {
                        log::error!("TCP Server: Failed to bind port {}: {:?}", port, e);
                        return;
                    }
                };
                log::info!("TCP Server listening on port {}", port);

                let mut shutdown_rx = rx;

                loop {
                    tokio::select! {
                        accept_res = listener.accept() => {
                            let (stream, peer_addr) = match accept_res {
                                Ok(res) => res,
                                Err(e) => {
                                    log::debug!("TCP Server accept error: {:?}", e);
                                    tokio::time::sleep(Duration::from_millis(100)).await;
                                    continue;
                                }
                            };

                            let ip = peer_addr.ip().to_string();
                            log::info!("TCP: Connection from {}", ip);

                            let clients_inner = clients_clone.clone();
                            let on_conn = on_connect.clone();
                            let on_disc = on_disconnect.clone();
                            let on_clip = on_clipboard_recv.clone();
                            let pending_trusts_inner = pending_trusts.clone();
                            let running_inner = running_clone.clone();
                            let mut client_shutdown = shutdown_rx.resubscribe();

                            // Spawn connection handler task
                            tokio::spawn(async move {
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

                                let acceptor = TlsAcceptor::from(Arc::new(server_config));
                                let mut tls_stream = match acceptor.accept(stream).await {
                                    Ok(s) => s,
                                    Err(e) => {
                                        log::error!("TCP Server: TLS handshake failed with {}: {:?}", ip, e);
                                        return;
                                    }
                                };

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
                                if let Err(e) = true_send_async(&mut tls_stream, &config_bytes).await {
                                    log::warn!("TCP Server: Failed to send UDP session config to {}: {:?}", ip, e);
                                    return;
                                }

                                // Receive client's ScreenMetrics
                                let metrics_bytes = match true_recv_async(&mut tls_stream).await {
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

                                log::info!("TCP Server: Client {} authenticated & verified (conn_id: {})", ip, connection_id);

                                // Load attachments
                                let attachments = get_attachments(&ip).unwrap_or_else(|_| ScreenAttachments {
                                    address: ip.clone(),
                                    top: None,
                                    right: None,
                                    bottom: None,
                                    left: None,
                                });

                                let tls_stream_arc = Arc::new(tokio::sync::Mutex::new(tls_stream));
                                let client_info = Arc::new(Mutex::new(TcpClientInfo {
                                    ip: ip.clone(),
                                    metrics: metrics.clone(),
                                    stream: tls_stream_arc.clone(),
                                    attachments,
                                }));

                                {
                                    let mut list = clients_inner.lock().unwrap();
                                    list.push(client_info.clone());
                                }

                                on_conn(ip.clone(), metrics, cryptor, connection_id);

                                loop {
                                    if !*running_inner.lock().unwrap() {
                                        break;
                                    }

                                    let mut s_lock = tls_stream_arc.lock().await;
                                    tokio::select! {
                                        recv_res = true_recv_async(&mut *s_lock) => {
                                            match recv_res {
                                                Ok(payload_bytes) => {
                                                    if payload_bytes.is_empty() {
                                                        log::debug!("TCP Server: Received empty payload (EOF) from {}", ip);
                                                        break;
                                                    }
                                                    log::debug!("TCP Server: Received clipboard payload from {}", ip);
                                                    if let Ok(payload) = serde_json::from_slice::<ClipboardPayload>(&payload_bytes) {
                                                        drop(s_lock); // Drop lock before invoking callback
                                                        on_clip(payload, ip.clone());
                                                    }
                                                }
                                                Err(e) => {
                                                    log::debug!("TCP Server: Read error from {}: {:?}", ip, e);
                                                    break;
                                                }
                                            }
                                        }
                                        _ = client_shutdown.recv() => {
                                            break;
                                        }
                                    }
                                }

                                // Cleanup client
                                log::info!("TCP: Client {} disconnected (conn_id: {})", ip, connection_id);
                                {
                                    let mut list = clients_inner.lock().unwrap();
                                    list.retain(|c| !Arc::ptr_eq(c, &client_info));
                                }

                                on_disc(ip, connection_id);
                            });
                        }
                        _ = shutdown_rx.recv() => {
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

        {
            let mut s_lock = self.shutdown_tx.lock().unwrap();
            if let Some(tx) = s_lock.take() {
                let _ = tx.send(());
            }
        }

        let mut clients = self.clients.lock().unwrap();
        clients.clear();
    }

    pub fn broadcast_clipboard(
        &self,
        payload: &ClipboardPayload,
        exclude_ip: Option<&str>,
    ) {
        log::debug!("TCP: Broadcasting clipboard (excluding client: {:?})", exclude_ip);
        let payload = payload.clone();
        let exclude_ip = exclude_ip.map(|s| s.to_string());
        let clients = self.clients.clone();

        crate::network::TOKIO_RUNTIME.spawn(async move {
            let payload_bytes = serde_json::to_vec(&payload).unwrap();
            let clients_list = {
                let list = clients.lock().unwrap();
                list.clone()
            };
            for client in clients_list {
                let (ip, stream) = {
                    let c = client.lock().unwrap();
                    (c.ip.clone(), c.stream.clone())
                };
                if let Some(ref exclude) = exclude_ip {
                    if &ip == exclude {
                        continue;
                    }
                }
                let mut s = stream.lock().await;
                let _ = true_send_async(&mut *s, &payload_bytes).await;
            }
        });
    }
}

pub struct TcpClient {
    running: Arc<Mutex<bool>>,
    stream: Arc<Mutex<Option<Arc<tokio::sync::Mutex<tokio_rustls::client::TlsStream<TcpStream>>>>>>,
    shutdown_tx: Arc<Mutex<Option<tokio::sync::broadcast::Sender<()>>>>,
}

impl TcpClient {
    pub fn new() -> Self {
        Self {
            running: Arc::new(Mutex::new(false)),
            stream: Arc::new(Mutex::new(None)),
            shutdown_tx: Arc::new(Mutex::new(None)),
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

        let (tx, rx) = tokio::sync::broadcast::channel(1);
        {
            let mut s_lock = self.shutdown_tx.lock().unwrap();
            *s_lock = Some(tx);
        }

        let running = self.running.clone();
        let stream_opt = self.stream.clone();
        let server_ip_str = server_ip.to_string();
        let on_connect = Arc::new(on_connect);
        let on_disconnect = Arc::new(on_disconnect);
        let on_clipboard_recv = Arc::new(on_clipboard_recv);

        let tls_stream = crate::network::TOKIO_RUNTIME.block_on(async move {
            let stream = TcpStream::connect(server_addr).await?;

            // Load client's self-signed cert and key
            let (cert_pem, key_pem) = match crate::crypto::load_or_generate_cert(crate::paths::get_app_dir()) {
                Ok(pair) => pair,
                Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
            };
            let (certs, private_key) = load_certs_and_key(&cert_pem, &key_pem);

            // Build client configuration with custom TOFU server verifier and client certificate for mTLS
            let server_verifier = Arc::new(TofuServerVerifier::new(server_ip_str.clone(), pending_trusts));
            let client_config = match ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(server_verifier)
                .with_client_auth_cert(certs, private_key)
            {
                Ok(cfg) => cfg,
                Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
            };

            let server_name = match rustls_pki_types::ServerName::try_from(server_ip_str) {
                Ok(name) => name.to_owned(),
                Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string())),
            };

            let connector = TlsConnector::from(Arc::new(client_config));
            let mut tls_stream = connector.connect(server_name, stream).await?;

            // Receive UDP session key config from server over TLS
            let session_bytes = true_recv_async(&mut tls_stream).await?;
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
            true_send_async(&mut tls_stream, &metrics_bytes).await?;

            Ok::<(tokio_rustls::client::TlsStream<TcpStream>, UdpSessionConfig), std::io::Error>((tls_stream, session_config))
        })?;

        let (tls_stream, session_config) = tls_stream;
        log::info!("TCP Client connected & authenticated via TLS");

        let tls_stream_arc = Arc::new(tokio::sync::Mutex::new(tls_stream));
        {
            let mut s_lock = stream_opt.lock().unwrap();
            *s_lock = Some(tls_stream_arc.clone());
        }

        {
            let mut r = running.lock().unwrap();
            *r = true;
        }

        on_connect(session_config.key, session_config.salt);

        let running_clone = running.clone();
        let stream_clone = stream_opt.clone();

        let mut client_shutdown = rx;

        crate::network::TOKIO_RUNTIME.spawn(async move {
                loop {
                    if !*running_clone.lock().unwrap() {
                        break;
                    }

                    let stream_opt = {
                        let s_lock = stream_clone.lock().unwrap();
                        s_lock.clone()
                    };

                    if let Some(s_arc) = stream_opt {
                        let mut s = s_arc.lock().await;
                        tokio::select! {
                            recv_res = true_recv_async(&mut *s) => {
                                match recv_res {
                                    Ok(payload_bytes) => {
                                        if payload_bytes.is_empty() {
                                            log::debug!("TCP Client: Received empty payload (EOF) from server");
                                            break;
                                        }
                                        log::debug!("TCP Client: Received clipboard payload bytes from server");
                                        // Drop lock before calling callback to avoid deadlocks
                                        drop(s);
                                        if let Ok(payload) = serde_json::from_slice::<ClipboardPayload>(&payload_bytes) {
                                            on_clipboard_recv(payload);
                                        }
                                    }
                                    Err(e) => {
                                        log::debug!("TCP Client: Read error: {:?}", e);
                                        break;
                                    }
                                }
                            }
                            _ = client_shutdown.recv() => {
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
            let payload = payload.clone();
            crate::network::TOKIO_RUNTIME.spawn(async move {
                let payload_bytes = serde_json::to_vec(&payload).unwrap();
                let mut s = s_arc.lock().await;
                let _ = true_send_async(&mut *s, &payload_bytes).await;
            });
        }
        Ok(())
    }

    pub fn stop(&self) {
        log::info!("Stopping TCP Client");
        let mut r = self.running.lock().unwrap();
        *r = false;

        {
            let mut s_lock = self.shutdown_tx.lock().unwrap();
            if let Some(tx) = s_lock.take() {
                let _ = tx.send(());
            }
        }

        let mut s_lock = self.stream.lock().unwrap();
        *s_lock = None;
    }
}
