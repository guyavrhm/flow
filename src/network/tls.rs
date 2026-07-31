use std::io::BufReader;
use std::sync::{Arc, Mutex};
use rustls::client::danger::{ServerCertVerifier, ServerCertVerified, HandshakeSignatureValid};
use rustls::server::danger::{ClientCertVerifier, ClientCertVerified};
use rustls::{Error, SignatureScheme};
use rustls_pki_types::{CertificateDer, ServerName, PrivateKeyDer, UnixTime};

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
        
        // 1. Check if trusted in database
        match crate::config::get_trusted_fingerprint(&self.server_ip) {
            Ok(Some(trusted_fp)) => {
                if trusted_fp == fingerprint {
                    return Ok(ServerCertVerified::assertion());
                } else {
                    log::warn!("Fingerprint mismatch for server {}: expected {}, got {}", self.server_ip, trusted_fp, fingerprint);
                }
            }
            Ok(None) => {}
            Err(e) => {
                log::error!("Database error checking fingerprint: {:?}", e);
            }
        }

        // 2. Prompt user for Trust On First Use
        let (tx, rx) = std::sync::mpsc::channel();
        {
            let mut list = self.pending_trusts.lock().unwrap();
            list.push(PendingTrustRequest {
                ip: self.server_ip.clone(),
                fingerprint: fingerprint.clone(),
                tx,
            });
        }

        // Block connection thread until user accepts or rejects
        match rx.recv() {
            Ok(true) => {
                // Trust and save to DB
                if let Err(e) = crate::config::trust_fingerprint(&self.server_ip, &fingerprint) {
                    log::error!("Failed to save trusted fingerprint: {:?}", e);
                }
                Ok(ServerCertVerified::assertion())
            }
            _ => Err(Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer)),
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
        
        // 1. Check if trusted in database
        match crate::config::get_trusted_fingerprint(&self.client_ip) {
            Ok(Some(trusted_fp)) => {
                if trusted_fp == fingerprint {
                    return Ok(ClientCertVerified::assertion());
                } else {
                    log::warn!("Fingerprint mismatch for client {}: expected {}, got {}", self.client_ip, trusted_fp, fingerprint);
                }
            }
            Ok(None) => {}
            Err(e) => {
                log::error!("Database error checking client fingerprint: {:?}", e);
            }
        }

        // 2. Prompt user for Trust On First Use
        let (tx, rx) = std::sync::mpsc::channel();
        {
            let mut list = self.pending_trusts.lock().unwrap();
            list.push(PendingTrustRequest {
                ip: self.client_ip.clone(),
                fingerprint: fingerprint.clone(),
                tx,
            });
        }

        // Block connection thread until user accepts or rejects
        match rx.recv() {
            Ok(true) => {
                // Trust and save to DB
                if let Err(e) = crate::config::trust_fingerprint(&self.client_ip, &fingerprint) {
                    log::error!("Failed to save trusted client fingerprint: {:?}", e);
                }
                Ok(ClientCertVerified::assertion())
            }
            _ => Err(Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer)),
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
