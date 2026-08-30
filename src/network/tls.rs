use std::io::BufReader;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use rustls::client::danger::{ServerCertVerifier, ServerCertVerified, HandshakeSignatureValid};
use rustls::server::danger::{ClientCertVerifier, ClientCertVerified};
use rustls::{Error, SignatureScheme};
use rustls_pki_types::{CertificateDer, ServerName, PrivateKeyDer, UnixTime};

use crate::state::{STATE_MANAGER, PeerState, PendingTrust};

#[derive(Debug)]
pub struct PendingTrustRequest {
    pub ip: String,
    pub fingerprint: String,
    pub tx: std::sync::mpsc::Sender<bool>,
}

pub fn load_certs_and_key(cert_pem: &str, key_pem: &str) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
    let mut cert_reader = BufReader::new(cert_pem.as_bytes());
    let certs = rustls_pemfile::certs(&mut cert_reader)
        .map(|r| r.unwrap())
        .collect();

    let mut key_reader = BufReader::new(key_pem.as_bytes());
    let key = rustls_pemfile::private_key(&mut key_reader)
        .map(|r| r.unwrap())
        .unwrap();

    (certs, key)
}

#[derive(Debug)]
pub struct TofuServerVerifier {
    pub server_ip: String,
    pub pending_trusts: Arc<Mutex<Vec<PendingTrustRequest>>>,
}

impl TofuServerVerifier {
    pub fn new(server_ip: String, pending_trusts: Arc<Mutex<Vec<PendingTrustRequest>>>) -> Self {
        Self {
            server_ip,
            pending_trusts,
        }
    }
}

impl ServerCertVerifier for TofuServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let cert_der = end_entity.as_ref();
        let fingerprint = crate::crypto::compute_fingerprint(cert_der);
        log::info!("[TLS] Verifying Server certificate for [{}] (Fingerprint: {})", self.server_ip, fingerprint);
        
        let mut is_mismatch = false;

        // 1. Check if trusted in database
        match crate::config::get_trusted_fingerprint(&self.server_ip) {
            Ok(Some(trusted_fp)) => {
                if trusted_fp == fingerprint {
                    log::info!("[TLS] Server [{}] fingerprint MATCHES trusted database entry. Trust verified!", self.server_ip);
                    STATE_MANAGER.set_peer_state(&self.server_ip, PeerState::TlsEstablished);
                    return Ok(ServerCertVerified::assertion());
                } else {
                    is_mismatch = true;
                    log::warn!("[TLS] SECURITY WARNING: Fingerprint mismatch for server [{}]! Expected: {}, Got: {}", self.server_ip, trusted_fp, fingerprint);
                }
            }
            Ok(None) => {
                log::info!("[TLS] Server [{}] is not yet in known_hosts database. Prompting user for Trust-On-First-Use.", self.server_ip);
            }
            Err(e) => {
                log::error!("[TLS] Database error checking server fingerprint: {:?}", e);
            }
        }

        // 2. Prompt user for Trust On First Use
        let (tx, rx) = std::sync::mpsc::channel();
        {
            let mut list = self.pending_trusts.lock().unwrap();
            list.push(PendingTrustRequest {
                ip: self.server_ip.clone(),
                fingerprint: fingerprint.clone(),
                tx: tx.clone(),
            });
        }

        // Register with StateManager to wake up UI immediately
        STATE_MANAGER.set_peer_state(
            &self.server_ip,
            PeerState::PendingTrustApproval(PendingTrust {
                ip: self.server_ip.clone(),
                fingerprint: fingerprint.clone(),
                is_mismatch,
                tx: Arc::new(Mutex::new(Some(tx))),
            }),
        );
        STATE_MANAGER.request_repaint();

        // Block connection thread until user accepts, rejects, or times out after 60 seconds
        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(true) => {
                log::info!("[TLS] User ACCEPTED trust for server [{}]. Saving to database.", self.server_ip);
                if let Err(e) = crate::config::trust_fingerprint(&self.server_ip, &fingerprint) {
                    log::error!("[TLS] Failed to save trusted fingerprint to DB: {:?}", e);
                }
                STATE_MANAGER.set_peer_state(&self.server_ip, PeerState::TlsEstablished);
                Ok(ServerCertVerified::assertion())
            }
            Ok(false) => {
                log::warn!("[TLS] User REJECTED connection from server [{}]", self.server_ip);
                STATE_MANAGER.set_peer_state(&self.server_ip, PeerState::Disconnected);
                Err(Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer))
            }
            Err(_) => {
                log::warn!("[TLS] Trust verification for server [{}] timed out after 60s", self.server_ip);
                STATE_MANAGER.set_peer_state(&self.server_ip, PeerState::Disconnected);
                Err(Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer))
            }
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::ED25519,
        ]
    }
}

#[derive(Debug)]
pub struct TofuClientVerifier {
    pub client_ip: String,
    pub pending_trusts: Arc<Mutex<Vec<PendingTrustRequest>>>,
}

impl TofuClientVerifier {
    pub fn new(client_ip: String, pending_trusts: Arc<Mutex<Vec<PendingTrustRequest>>>) -> Self {
        Self {
            client_ip,
            pending_trusts,
        }
    }
}

impl ClientCertVerifier for TofuClientVerifier {
    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        let cert_der = end_entity.as_ref();
        let fingerprint = crate::crypto::compute_fingerprint(cert_der);
        log::info!("[TLS] Verifying Client certificate for [{}] (Fingerprint: {})", self.client_ip, fingerprint);
        
        let mut is_mismatch = false;

        // 1. Check if trusted in database
        match crate::config::get_trusted_fingerprint(&self.client_ip) {
            Ok(Some(trusted_fp)) => {
                if trusted_fp == fingerprint {
                    log::info!("[TLS] Client [{}] fingerprint MATCHES trusted database entry. Trust verified!", self.client_ip);
                    STATE_MANAGER.set_peer_state(&self.client_ip, PeerState::TlsEstablished);
                    return Ok(ClientCertVerified::assertion());
                } else {
                    is_mismatch = true;
                    log::warn!("[TLS] SECURITY WARNING: Fingerprint mismatch for client [{}]! Expected: {}, Got: {}", self.client_ip, trusted_fp, fingerprint);
                }
            }
            Ok(None) => {
                log::info!("[TLS] Client [{}] is not yet in known_hosts database. Prompting user for Trust-On-First-Use.", self.client_ip);
            }
            Err(e) => {
                log::error!("[TLS] Database error checking client fingerprint: {:?}", e);
            }
        }

        // 2. Prompt user for Trust On First Use
        let (tx, rx) = std::sync::mpsc::channel();
        {
            let mut list = self.pending_trusts.lock().unwrap();
            list.push(PendingTrustRequest {
                ip: self.client_ip.clone(),
                fingerprint: fingerprint.clone(),
                tx: tx.clone(),
            });
        }

        // Register with StateManager to wake up UI immediately
        STATE_MANAGER.set_peer_state(
            &self.client_ip,
            PeerState::PendingTrustApproval(PendingTrust {
                ip: self.client_ip.clone(),
                fingerprint: fingerprint.clone(),
                is_mismatch,
                tx: Arc::new(Mutex::new(Some(tx))),
            }),
        );
        STATE_MANAGER.request_repaint();

        // Block connection thread until user accepts, rejects, or times out after 60 seconds
        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(true) => {
                log::info!("[TLS] User ACCEPTED trust for client [{}]. Saving to database.", self.client_ip);
                if let Err(e) = crate::config::trust_fingerprint(&self.client_ip, &fingerprint) {
                    log::error!("[TLS] Failed to save trusted client fingerprint to DB: {:?}", e);
                }
                STATE_MANAGER.set_peer_state(&self.client_ip, PeerState::TlsEstablished);
                Ok(ClientCertVerified::assertion())
            }
            Ok(false) => {
                log::warn!("[TLS] User REJECTED connection from client [{}]", self.client_ip);
                STATE_MANAGER.set_peer_state(&self.client_ip, PeerState::Disconnected);
                Err(Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer))
            }
            Err(_) => {
                log::warn!("[TLS] Trust verification for client [{}] timed out after 60s", self.client_ip);
                STATE_MANAGER.set_peer_state(&self.client_ip, PeerState::Disconnected);
                Err(Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer))
            }
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::ED25519,
        ]
    }
}
