use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use once_cell::sync::Lazy;

use crate::config::MonitorLayout;
use crate::crypto::UdpCryptor;
use crate::network::protocol::ScreenMetrics;

/// Global atomic flag checked directly by low-level OS input hooks (macOS EventTap, Linux evdev, Windows LL hook)
pub static IS_REDIRECTING: AtomicBool = AtomicBool::new(false);

/// Global atomic flag signaling an emergency release request from keyboard hook
pub static FORCE_EMERGENCY_RELEASE: AtomicBool = AtomicBool::new(false);

/// Global singleton instance of the StateManager for FFI callbacks and emergency escape hooks
pub static STATE_MANAGER: Lazy<Arc<StateManager>> = Lazy::new(|| Arc::new(StateManager::new()));

#[derive(Clone, Debug)]
pub struct PendingTrust {
    pub ip: String,
    pub fingerprint: String,
    pub is_mismatch: bool,
    pub tx: Arc<Mutex<Option<std::sync::mpsc::Sender<bool>>>>,
}

#[derive(Clone)]
pub enum PeerState {
    Connecting,
    PendingTrustApproval(PendingTrust),
    TlsEstablished,
    UdpHandshaking,
    FullyConnected {
        metrics: ScreenMetrics,
        udp_addr: SocketAddr,
        cryptor: UdpCryptor,
        connection_id: u64,
    },
    Disconnected,
}

impl std::fmt::Debug for PeerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PeerState::Connecting => write!(f, "Connecting"),
            PeerState::PendingTrustApproval(p) => write!(f, "PendingTrustApproval(ip={}, mismatch={})", p.ip, p.is_mismatch),
            PeerState::TlsEstablished => write!(f, "TlsEstablished"),
            PeerState::UdpHandshaking => write!(f, "UdpHandshaking"),
            PeerState::FullyConnected { udp_addr, connection_id, .. } => {
                write!(f, "FullyConnected(udp={}, conn_id={})", udp_addr, connection_id)
            }
            PeerState::Disconnected => write!(f, "Disconnected"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum InputControlState {
    Local,
    Redirected {
        target_ip: String,
        return_pos: (i32, i32),
    },
}

pub struct StateManager {
    peers: Arc<Mutex<HashMap<String, PeerState>>>,
    input_state: Arc<Mutex<InputControlState>>,
    egui_ctx: Arc<Mutex<Option<eframe::egui::Context>>>,
    active_monitors: Arc<Mutex<Vec<MonitorLayout>>>,
    topology_version: Arc<std::sync::atomic::AtomicU64>,
}

impl Default for StateManager {
    fn default() -> Self {
        Self::new()
    }
}

impl StateManager {
    pub fn new() -> Self {
        Self {
            peers: Arc::new(Mutex::new(HashMap::new())),
            input_state: Arc::new(Mutex::new(InputControlState::Local)),
            egui_ctx: Arc::new(Mutex::new(None)),
            active_monitors: Arc::new(Mutex::new(Vec::new())),
            topology_version: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    pub fn topology_version(&self) -> u64 {
        self.topology_version.load(Ordering::Relaxed)
    }

    pub fn notify_topology_changed(&self) {
        self.topology_version.fetch_add(1, Ordering::SeqCst);
        self.request_repaint();
    }

    pub fn set_egui_ctx(&self, ctx: eframe::egui::Context) {
        let mut lock = self.egui_ctx.lock().unwrap();
        *lock = Some(ctx);
    }

    pub fn request_repaint(&self) {
        if let Some(ctx) = self.egui_ctx.lock().unwrap().as_ref() {
            ctx.request_repaint();
        }
    }

    pub fn set_peer_state(&self, ip: &str, new_state: PeerState) {
        let prev_repr = {
            let mut peers = self.peers.lock().unwrap();
            let prev = peers.insert(ip.to_string(), new_state.clone());
            format!("{:?}", prev)
        };

        log::info!("[STATE] Peer [{}] transition: {} -> {:?}", ip, prev_repr, new_state);
        self.topology_version.fetch_add(1, Ordering::SeqCst);

        // Invariant guard: If a peer disconnects while we are redirecting to it, trigger immediate emergency release
        if matches!(new_state, PeerState::Disconnected) {
            let should_release = {
                let input = self.input_state.lock().unwrap();
                match &*input {
                    InputControlState::Redirected { target_ip, .. } => target_ip == ip,
                    _ => false,
                }
            };
            if should_release {
                log::warn!("[STATE] Active controlled peer [{}] disconnected! Forcing emergency release to host.", ip);
                self.emergency_release();
            }
        }

        self.request_repaint();
    }

    pub fn remove_peer(&self, ip: &str) {
        let prev = {
            let mut peers = self.peers.lock().unwrap();
            peers.remove(ip)
        };
        log::info!("[STATE] Peer [{}] removed (was: {:?})", ip, prev);
        self.topology_version.fetch_add(1, Ordering::SeqCst);

        let should_release = {
            let input = self.input_state.lock().unwrap();
            match &*input {
                InputControlState::Redirected { target_ip, .. } => target_ip == ip,
                _ => false,
            }
        };
        if should_release {
            self.emergency_release();
        }

        self.request_repaint();
    }

    pub fn get_peer_state(&self, ip: &str) -> Option<PeerState> {
        let peers = self.peers.lock().unwrap();
        peers.get(ip).cloned()
    }

    pub fn is_peer_fully_connected(&self, ip: &str) -> bool {
        let peers = self.peers.lock().unwrap();
        matches!(peers.get(ip), Some(PeerState::FullyConnected { .. }))
    }

    pub fn get_fully_connected_peers(&self) -> Vec<String> {
        let peers = self.peers.lock().unwrap();
        peers
            .iter()
            .filter_map(|(ip, state)| {
                if matches!(state, PeerState::FullyConnected { .. }) {
                    Some(ip.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn has_any_connected_peer(&self) -> bool {
        let peers = self.peers.lock().unwrap();
        peers.values().any(|s| matches!(s, PeerState::FullyConnected { .. }))
    }

    pub fn get_pending_trusts(&self) -> Vec<PendingTrust> {
        let peers = self.peers.lock().unwrap();
        peers
            .values()
            .filter_map(|state| {
                if let PeerState::PendingTrustApproval(req) = state {
                    Some(req.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn approve_trust(&self, ip: &str) {
        log::info!("[STATE] User approved trust for peer [{}]", ip);
        let peers = self.peers.lock().unwrap();
        if let Some(PeerState::PendingTrustApproval(req)) = peers.get(ip) {
            if let Some(tx) = req.tx.lock().unwrap().take() {
                let _ = tx.send(true);
            }
        }
        self.request_repaint();
    }

    pub fn reject_trust(&self, ip: &str) {
        log::info!("[STATE] User rejected trust for peer [{}]", ip);
        let mut peers = self.peers.lock().unwrap();
        if let Some(PeerState::PendingTrustApproval(req)) = peers.get(ip) {
            if let Some(tx) = req.tx.lock().unwrap().take() {
                let _ = tx.send(false);
            }
        }
        peers.insert(ip.to_string(), PeerState::Disconnected);
        self.request_repaint();
    }

    pub fn get_input_state(&self) -> InputControlState {
        let lock = self.input_state.lock().unwrap();
        lock.clone()
    }

    /// Requests to transition input redirection to `target_ip`.
    /// Enforces the invariant that `target_ip` MUST be `PeerState::FullyConnected`.
    pub fn request_transition(&self, target_ip: &str, enter_pos: (i32, i32)) -> bool {
        if !self.is_peer_fully_connected(target_ip) {
            log::warn!(
                "[KVM] Transition to [{}] REJECTED: Peer is not in FullyConnected state!",
                target_ip
            );
            return false;
        }

        {
            let mut input = self.input_state.lock().unwrap();
            *input = InputControlState::Redirected {
                target_ip: target_ip.to_string(),
                return_pos: enter_pos,
            };
        }

        IS_REDIRECTING.store(true, Ordering::SeqCst);
        crate::hardware::hide_cursor();
        log::info!("[KVM] Input redirection ACTIVATED -> [{}]", target_ip);
        self.request_repaint();
        true
    }

    pub fn transition_to_local(&self) {
        {
            let mut input = self.input_state.lock().unwrap();
            *input = InputControlState::Local;
        }

        IS_REDIRECTING.store(false, Ordering::SeqCst);

        // Ensure host cursor is displayed natively via hardware abstraction
        crate::hardware::show_cursor();

        log::info!("[KVM] Input redirection DEACTIVATED: Control returned to host");
        self.request_repaint();
    }

    pub fn emergency_release(&self) -> Option<String> {
        log::warn!("[KVM] !!! EMERGENCY RELEASE TRIGGERED: Restoring all host controls !!!");
        let prev_target = {
            let mut input = self.input_state.lock().unwrap();
            let target = match &*input {
                InputControlState::Redirected { target_ip, .. } => Some(target_ip.clone()),
                _ => None,
            };
            *input = InputControlState::Local;
            target
        };

        IS_REDIRECTING.store(false, Ordering::SeqCst);
        FORCE_EMERGENCY_RELEASE.store(true, Ordering::SeqCst);

        // Restore cursor natively via hardware abstraction
        crate::hardware::show_cursor();

        self.request_repaint();
        prev_target
    }

    pub fn update_active_monitors(&self, layouts: Vec<MonitorLayout>) {
        let mut lock = self.active_monitors.lock().unwrap();
        *lock = layouts;
        self.topology_version.fetch_add(1, Ordering::SeqCst);
        self.request_repaint();
    }

    pub fn get_active_monitors(&self) -> Vec<MonitorLayout> {
        let lock = self.active_monitors.lock().unwrap();
        lock.clone()
    }
}
