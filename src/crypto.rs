use rcgen::{CertificateParams, KeyPair, date_time_ymd, SanType, Ia5String};
use sha2::{Sha256, Digest};
use chacha20poly1305::{aead::{Aead, KeyInit}, ChaCha20Poly1305, Nonce};

// Generate self-signed certificate and private key in PEM format
pub fn generate_self_signed_cert() -> Result<(String, String), rcgen::Error> {
    let key_pair = KeyPair::generate()?;
    let mut params = CertificateParams::default();
    params.not_before = date_time_ymd(2026, 1, 1);
    params.not_after = date_time_ymd(2036, 1, 1);
    params.subject_alt_names = vec![SanType::DnsName(Ia5String::try_from("flow-kvm").unwrap())];
    
    let cert = params.self_signed(&key_pair)?;
    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();
    Ok((cert_pem, key_pem))
}

// Compute a human-readable SHA-256 fingerprint from a DER-encoded certificate
pub fn compute_fingerprint(cert_der: &[u8]) -> String {
    let hash = Sha256::digest(cert_der);
    hash.iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<String>>()
        .join(":")
}

// Sliding window replay protector (64-packet window)
pub struct UdpReplayProtector {
    max_received_seq: u64,
    window: u64,
}

impl UdpReplayProtector {
    pub fn new() -> Self {
        Self {
            max_received_seq: 0,
            window: 0,
        }
    }

    pub fn is_valid(&mut self, seq: u64) -> bool {
        if seq > self.max_received_seq {
            let shift = seq - self.max_received_seq;
            if shift >= 64 {
                self.window = 1;
            } else {
                self.window = (self.window << shift) | 1;
            }
            self.max_received_seq = seq;
            true
        } else if seq + 64 > self.max_received_seq {
            // Packet is older, but within our 64-packet window.
            let bit_pos = self.max_received_seq - seq;
            let mask = 1 << bit_pos;
            if (self.window & mask) == 0 {
                // We haven't seen this packet yet!
                self.window |= mask;
                true
            } else {
                false // We already saw this specific packet (Replay)
            }
        } else {
            false // Packet is too old (outside the window)
        }
    }
}

// Secure UDP encryption/decryption using ChaCha20-Poly1305 AEAD
#[derive(Clone)]
pub struct UdpCryptor {
    cipher: ChaCha20Poly1305,
    salt: [u8; 4],
}

impl UdpCryptor {
    pub fn new(key: &[u8; 32], salt: [u8; 4]) -> Self {
        Self {
            cipher: ChaCha20Poly1305::new(key.into()),
            salt,
        }
    }

    pub fn encrypt(&self, seq: u64, plaintext: &[u8]) -> Result<Vec<u8>, chacha20poly1305::Error> {
        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[0..4].copy_from_slice(&self.salt);
        nonce_bytes[4..12].copy_from_slice(&seq.to_be_bytes());
        let nonce = Nonce::from_slice(&nonce_bytes);
        self.cipher.encrypt(nonce, plaintext)
    }

    pub fn decrypt(&self, seq: u64, ciphertext: &[u8]) -> Result<Vec<u8>, chacha20poly1305::Error> {
        let mut nonce_bytes = [0u8; 12];
        nonce_bytes[0..4].copy_from_slice(&self.salt);
        nonce_bytes[4..12].copy_from_slice(&seq.to_be_bytes());
        let nonce = Nonce::from_slice(&nonce_bytes);
        self.cipher.decrypt(nonce, ciphertext)
    }
}

pub fn load_or_generate_cert(app_dir: std::path::PathBuf) -> Result<(String, String), Box<dyn std::error::Error>> {
    let cert_path = app_dir.join("cert.pem");
    let key_path = app_dir.join("key.pem");
    if cert_path.exists() && key_path.exists() {
        let cert_pem = std::fs::read_to_string(cert_path)?;
        let key_pem = std::fs::read_to_string(key_path)?;
        Ok((cert_pem, key_pem))
    } else {
        let _ = std::fs::create_dir_all(&app_dir);
        let (cert_pem, key_pem) = generate_self_signed_cert()?;
        std::fs::write(cert_path, &cert_pem)?;
        std::fs::write(key_path, &key_pem)?;
        Ok((cert_pem, key_pem))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cert_generation_and_fingerprint() {
        let cert_res = generate_self_signed_cert();
        assert!(cert_res.is_ok());
        let (cert_pem, key_pem) = cert_res.unwrap();
        assert!(!cert_pem.is_empty());
        assert!(!key_pem.is_empty());
        
        let fingerprint = compute_fingerprint(b"test cert content");
        assert!(!fingerprint.is_empty());
    }

    #[test]
    fn test_udp_replay_protector() {
        let mut protector = UdpReplayProtector::new();
        
        // Initial sequence must be accepted
        assert!(protector.is_valid(10));
        
        // Higher sequence accepted
        assert!(protector.is_valid(15));
        
        // Duplicate sequence rejected
        assert!(!protector.is_valid(15));
        assert!(!protector.is_valid(10));
        
        // Slightly older but unseen sequence accepted
        assert!(protector.is_valid(14));
        
        // Older sequence now marked as seen, must be rejected
        assert!(!protector.is_valid(14));
        
        // High jump resets window to 100
        assert!(protector.is_valid(100));
        // 15 is now outside the window (100 - 15 = 85 >= 64), must be rejected
        assert!(!protector.is_valid(15));
        // 35 is also outside the window (100 - 35 = 65 >= 64), must be rejected
        assert!(!protector.is_valid(35));
    }

    #[test]
    fn test_udp_cryptor() {
        let key = [7u8; 32];
        let salt = [1, 2, 3, 4];
        let cryptor = UdpCryptor::new(&key, salt);
        
        let plaintext = b"Hello, secure KVM world!";
        let seq = 42u64;
        
        let encrypted = cryptor.encrypt(seq, plaintext).unwrap();
        let decrypted = cryptor.decrypt(seq, &encrypted).unwrap();
        
        assert_eq!(plaintext.as_slice(), decrypted.as_slice());
        
        // Mismatched sequence decryption should fail
        assert!(cryptor.decrypt(seq + 1, &encrypted).is_err());
    }
}
