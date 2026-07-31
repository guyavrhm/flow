# flow Architecture & Developer Guide

Welcome to the **flow** developer guide! This document explains the system architecture, network flow, threading model, and implementation details to help you understand the inner workings of flow's codebase.

---

## 1. System Overview

`flow` is an open-source, cross-platform virtual KVM (Keyboard, Video, and Mouse) software. It runs in one of two modes:
1. **Server Mode (Host):** The computer with the physical mouse and keyboard. It tracks the mouse position, and if the cursor crosses a virtual boundary, it intercepts the inputs (swallowing them locally) and redirects them over the network.
2. **Client Mode (Guest):** The computer that receives input packets over the network and injects them directly into its native OS input event stream.

---

## 2. Codebase Architecture

The project is structured logically into separate systems:

```mermaid
graph TD
    Main[src/main.rs] --> UI[src/ui/mod.rs]
    Main --> Engine[src/engine.rs]
    UI --> Engine
    Engine --> Network[src/network]
    Engine --> Hardware[src/hardware]
    Network --> Protocol[src/network/protocol.rs]
    Network --> TCP[src/network/tcp.rs]
    Network --> UDP[src/network/udp.rs]
    Engine --> Crypto[src/crypto.rs]
    Engine --> Config[src/config.rs]
```

* **[src/main.rs](src/main.rs):** Entry point. Initializes the SQLite DB cache, GTK (on Linux), and starts the `egui` native loop.
* **[src/engine.rs](src/engine.rs):** The central state machine (`AppEngine`). Coordinates transitions between server and client modes, manages active connection records, and orchestrates tracking threads.
* **[src/network/](src/network/):** Networking module stack:
  * **[mod.rs](src/network/mod.rs):** Exposes network submodules and provides general utility functions (e.g. retrieving the local IP address).
  * **[tcp.rs](src/network/tcp.rs):** Manages connection handshakes, screen metrics exchange, and clipboard synchronization.
  * **[udp.rs](src/network/udp.rs):** Runs the low-latency network pipeline for high-frequency input events.
  * **[protocol.rs](src/network/protocol.rs):** Defines network serialization structs (`InputEvent`, `ClipboardPayload`, `ScreenMetrics`).
  * **[tls.rs](src/network/tls.rs):** Custom Trust-On-First-Use (TOFU) mutual TLS verifiers (`TofuServerVerifier` and `TofuClientVerifier`) and PEM loading helpers.
* **[src/hardware/](src/hardware/):** Hardware input capture and simulation stack:
  * **[mod.rs](src/hardware/mod.rs):** Manages conditional compilation (`#[cfg(target_os)]`) to dynamically select and export the correct OS-specific FFI backend at build time.
  * **Platform Backends:** Platform-specific FFI modules (`mac.rs`, `win.rs`, `linux.rs`) implementing OS-specific hooks and input injection.
* **[src/crypto.rs](src/crypto.rs):** Handles self-signed X.509 certificate generation/loading, SHA-256 fingerprint computation, sliding window replay protection for UDP packets, and symmetric ChaCha20-Poly1305 encryption/decryption.
* **[src/config.rs](src/config.rs):** SQLite database layer (`rusqlite`) for saving settings and screen-arrangement layout mapping.
* **[src/ui/](src/ui/):** Immediate-mode UI layout modules:
  * **[src/ui/mod.rs](src/ui/mod.rs):** Coordinates the desktop configurations view, polls background status variables, manages tray menu interactions, and executes async engine reloads when settings are saved.
  * **[src/ui/canvas.rs](src/ui/canvas.rs):** Renders the interactive layout map panel where users drag, drop, and snap virtual monitor edges.
  * **[src/ui/tray.rs](src/ui/tray.rs):** Hooks native platform system tray icons, managing connection status visuals (Checkmark vs. Error indicators).

---

## 3. Network & Control Timeline

The following sequence diagram outlines a typical timeline where **Client A** starts active, **Client B** connects in the background, control returns to the server, and a clipboard event is broadcasted:

```mermaid
sequenceDiagram
    participant Server as Server (Host)
    participant ClientA as Client A (Active Guest)
    participant ClientB as Client B (Inactive Guest)

    Note over ClientA, Server: TLS 1.3 Handshake (Client A)
    ClientA->>Server: TCP Connection (Port 8118) + ClientHello
    Server-->>ClientA: ServerHello + Self-Signed X.509 Certificate
    Note over ClientA: Calculates SHA-256 Fingerprint
    Note over ClientA: Checks SQLite DB (TOFU). Prompts user if new.
    ClientA-->>Server: Client Certificate (mTLS)
    Note over Server: Calculates Client Certificate Fingerprint
    Note over Server: Checks SQLite DB (TOFU). Prompts user if new.
    
    Note over Server: Server generates 32-byte Key & 4-byte Salt
    Server->>ClientA: UdpSessionConfig (JSON over TLS TCP)
    ClientA->>Server: Send ScreenMetrics (JSON over TLS TCP)
    
    Note over ClientA, Server: UDP Handshake
    ClientA->>Server: UDP packet with "." encrypted with Key/Salt (Seq 0)
    Note over Server: Decrypts with Client Cryptor, registers client UDP Port
    
    Note over ClientB, Server: TLS 1.3 Handshake & Handshake (Client B)

    Note over ClientA, Server: Input Redirection Flow (Mouse reaches edge)
    Note over Server: Install Input Hooks (Swallow local inputs)
    loop Every Move/Click/Scroll/Keypress
        Note over Server: Increment sequence number (nonce counter)
        Server->>ClientA: Send InputEvent (UDP: [8-byte seq] [ChaCha20-Poly1305 payload])
        Note over ClientA: Verify seq (sliding window) & decrypt. Inject event.
    end

    Note over Server: Virtual cursor reaches client edge to return
    Server->>ClientA: Send InputEvent::Stop (UDP)
    Note over Server: Uninstall Input Hooks (Resume local inputs)

    Note over ClientB, Server: Clipboard Sharing (All sides poll local clipboard every 1s)
    ClientB->>Server: Send ClipboardPayload (TLS TCP)
    Note over Server: Writes Text/Files to local Clipboard
    Server->>ClientA: Broadcast ClipboardPayload (TLS TCP)
    Note over ClientA: Writes Text/Files to local Clipboard
```

## 4. Communication Protocol

`flow` encrypts all control traffic using **TLS 1.3** and real-time input events using **ChaCha20-Poly1305 AEAD**. Trust is validated using a **Trust on First Use (TOFU)** model checking self-signed certificate fingerprints against a SQLite database store (see [src/crypto.rs](file:///Users/guyavraham/flow/src/crypto.rs)).

### 4.1 TCP Control Channel (Port 8118)

TCP is used for connection establishment, capability exchange, and clipboard synchronization, entirely wrapped in TLS 1.3.

#### TCP Frame Format (Message Framing)
To demarcate JSON payloads sent over the stream, each message is transmitted with a length prefix:
* **Length Header**: 10-byte ASCII, zero-padded integer representing the length of the plaintext payload (e.g., `0000000084`).
* **Plaintext Payload**: The raw JSON payload (e.g. `UdpSessionConfig`, `ScreenMetrics`, or `ClipboardPayload`).

#### Connection & Handshake Flow
1. **TLS Handshake & mTLS TOFU**:
   * The client connects to port `8118` and starts the TLS 1.3 handshake.
   * Both sides exchange self-signed certificates and calculate their SHA-256 fingerprints.
   * If a fingerprint is not found in the local SQLite table `known_hosts`, the handshake thread blocks, and a modal displays in the `egui` GUI requesting user confirmation. If accepted, the fingerprint is saved to the DB.
2. **UDP Session Key Configuration**:
   * Immediately after TLS is established, the Server generates a random 32-byte key and 4-byte salt, serializing them into a `UdpSessionConfig` message sent to the client.
3. **Screen Metrics Exchange**:
   - The client responds by sending its screen dimensions serialized as JSON in `ScreenMetrics` format.
4. **Control & Clipboard Channel**:
   - When a clipboard change is detected on either client or server, a `ClipboardPayload` is serialized to JSON and transmitted over the TLS stream.
5. **Connection Persistence & Reconnection Loop**:
   - The client runs a background connection loop (using cooperative 200ms checks) that automatically retries the TCP/TLS connection every 2 seconds if disconnected.
   - To prevent clean-up race conditions when a client disconnects and quickly reconnects, the server generates a unique ephemeral random `connection_id` (`u64`) for each session. During cleanup, the server checks this ID to ensure it only terminates the exact defunct connection instance, and uses pointer comparison (`Arc::ptr_eq`) to preserve active sessions.

### 4.2 UDP Input Channel (Port 8118)

UDP is used for low-latency transmission of high-frequency input events.

#### UDP Handshake
To bind client/server UDP sockets:
* The client sends a UDP packet containing a handshake signature: the character `.` encrypted with `UdpCryptor` at sequence number `0`.
* The server decrypts and verifies the packet using the client's cryptor to register the client's public UDP socket endpoint (`SocketAddr`).

#### Input Event Payload Format
All input events are sent as secure UDP packets:
* **Sequence Header**: 8-byte big-endian unsigned 64-bit integer (`u64`). Used directly to construct the 12-byte cryptographic nonce: `[4-byte salt] [8-byte sequence]`.
* **Ciphertext**: ChaCha20-Poly1305 encrypted space-delimited text event payload.
* **Anti-Replay**: The receiver validates incoming sequence numbers using a **64-packet sliding window bitmask** to discard duplicate or delayed packets.

Once decrypted, the payload follows a space-delimited text protocol:

| Format / Event | Description | Example |
|---|---|---|
| `mov <x> <y>` | Warps mouse to coordinates `x`, `y` | `mov 1280 720` |
| `scrl <dx> <dy>` | Simulates mouse scroll wheel offsets | `scrl 0 -120` |
| `prsm <pressed> <button>` | Simulates mouse click event (`pressed` is `true`/`false`, `button` is e.g. `Left`, `Right`) | `prsm true Left` |
| `prsk <pressed> <key>` | Simulates key press/release (`pressed` is `true`/`false`, `key` is key string identifier) | `prsk false Shift` |
| `stp` | Directs the client to stop capturing inputs and returns focus to the server | `stp` |

---

## 5. Threading & Concurrency Model

`flow` leverages native OS multi-threading for zero-latency execution. Below is a detailed breakdown of all active threads running in both Server and Client modes:

### Server Mode Threads (Host)

| Thread | Spawned by | Purpose / Role | Lifespan |
|---|---|---|---|
| **Main Thread (UI)** | System | Runs the `egui` / `winit` UI rendering loop (`eframe::run_native`). Handles user interactions, updates visual elements, and renders layout canvases. | Application lifetime |
| **TCP Accept Thread** | tcp.rs | Binds to port 8118 and runs a blocking loop waiting to accept incoming client connections. | Active while Server is running |
| **Client TCP Connections (1 per client)** | tcp.rs | Runs a blocking loop listening for incoming TCP payloads (handshake validation, screen metrics, and clipboard payloads) from a specific client. | Lifetime of client connection |
| **UDP Handshake Thread (1 per client)** | engine.rs | Created temporarily to wait for the client's UDP handshake packet to extract and store their remote UDP port, then terminates. | Transient (less than 3 seconds) |
| **Edge Tracking Thread** | engine.rs | Runs a 10ms loop checking local mouse boundaries. When control shifts, it activates blocking system-level input hooks (listening for mouse/keyboard inputs) and packages them to UDP. | Active while Server is running |
| **Clipboard Monitor Thread** | engine.rs | Runs a 1-second loop polling the local OS clipboard and broadcasts changes to clients via TCP. | Active while Server is running |
| **Engine Reload Thread** | mod.rs | Spawned briefly when the user hits "Save" to stop the engine and re-initialize socket bindings without freezing the UI thread. | Transient |

### Client Mode Threads (Guest)

| Thread | Spawned by | Purpose / Role | Lifespan |
|---|---|---|---|
| **Main Thread (UI)** | System | Runs the `egui` interface and tray indicators. | Application lifetime |
| **TCP Client Thread** | engine.rs | Runs a periodic reconnection loop that attempts to connect to the server's TCP socket every 2 seconds when disconnected, and runs a persistent blocking loop to receive incoming server clipboard packets once connected. | Active while Client is running |
| **UDP Client Thread** | udp.rs | Listens on a UDP socket for real-time input events (`Move`, `KeyPress`, `Stop`), decrypts them, and immediately simulates them on the local OS. | Active while Client is connected |
| **Clipboard Monitor Thread** | engine.rs | Runs a 1-second loop polling the local OS clipboard and sends changes to the server via TCP. | Active while Client is running |

### Thread Communication & Shared Data

State variables are synchronized across thread boundaries using lock-protected reference counters (`Arc<Mutex<T>>`):

| Shared Variable | Type | Written By | Read By | Purpose |
|---|---|---|---|---|
| **`settings`** | `Arc<Mutex<SettingsData>>` | Main UI Thread | Edge Tracker, TCP/UDP threads | Stores mode (Server/Client), Server IP, and encryption password. |
| **`is_running`** | `Arc<Mutex<bool>>` | Main UI Thread | All background threads | Controls engine startup, teardown, and reloading loops. |
| **`active_clients`** | `Arc<Mutex<HashMap<...>>>` | TCP connection threads | Edge Tracker | Tracks connected client metrics, screen attachments, and UDP endpoints. |
| **`current_controlled`** | `Arc<Mutex<String>>` | Edge Tracker | OS Input Hooks | Tracks which screen holds input focus (`"main"` or client IP). |
| **`is_connected`** | `Arc<Mutex<bool>>` | TCP threads | Main UI Thread | Drives visual tray connection status indicators ($V$ / $X$). |
| **`udp_server`** | `Arc<Mutex<Option<UdpServer>>>` | Main UI Thread | Edge Tracker | Stores the server's UDP socket reference to send input event packets. |
| **`clipboard_history`** | `Arc<Mutex<Vec<String>>>` | Clipboard & TCP threads | Clipboard & TCP threads | Stores recent clipboard content hashes to prevent network loopbacks. |

---

## 6. OS-Specific FFI & Hardware Integration

`flow` uses the src/hardware module as an abstraction layer to standardize cross-platform hardware events and OS windowing hooks (e.g., Win32, Cocoa/Quartz, X11).

#### 1. Screen & Keyboard Initialization Functions
* **`pub fn get_screeninfo() -> (i32, i32)`**
  * Returns the primary screen width and height in pixels.
* **`pub fn init_keyboard_layout()`**
  * Invoked on the main thread during app startup to cache keyboard layouts if required by the target OS.

#### 2. Clipboard Struct
Manages reading from and writing to the OS clipboard.
```rust
pub struct Clipboard;

impl Clipboard {
    // Polls current clipboard contents and returns it as a string
    pub fn data() -> String;

    // Sets the clipboard text content
    pub fn set_text(text: &str);

    // Sets the clipboard copied files content using absolute file paths
    pub fn set_files(paths: Vec<String>);
}
```

#### 3. Keyboard Controller (Client simulation)
Simulates keyboard events.
```rust
pub struct KeyboardController;

impl KeyboardController {
    pub fn new() -> Self;

    // Simulates key press down
    pub fn press(&self, key: &str);

    // Simulates key release up
    pub fn release(&self, key: &str);
}
```

#### 4. Keyboard Listener (Server interception)
Hooks keyboard events and swallows/suppresses local input when KVM redirection is active.
```rust
pub struct KeyboardListener;

impl KeyboardListener {
    // Constructor. callback parameters should accept key strings and press status.
    // if suppress is true, captured keys must be swallowed at the OS level.
    pub fn new<FPress, FRelease>(on_press: FPress, on_release: FRelease, suppress: bool) -> Self
    where
        FPress: Fn(String) + Send + Sync + 'static,
        FRelease: Fn(String) + Send + Sync + 'static;

    // Starts the event listening loop
    pub fn start(&self);
}
```

#### 5. Mouse Controller (Client simulation)
Simulates mouse coordinates, clicks, and scroll wheel actions.
```rust
pub struct MouseController;

impl MouseController {
    pub fn new() -> Self;

    // Returns current local cursor (x, y) coordinates
    pub fn position(&self) -> (i32, i32);

    // Warps mouse cursor directly to coordinates (x, y)
    pub fn set_position(&self, pos: (i32, i32));

    // Simulates mouse button click (press down)
    pub fn press(&self, button: &str);

    // Simulates mouse button release (up)
    pub fn release(&self, button: &str);

    // Simulates scroll wheel movement
    pub fn scroll(&self, dx: i32, dy: i32);
}
```

#### 6. Mouse Listener (Server interception)
Hooks mouse inputs and swallows local movements when redirection is active.
```rust
pub struct MouseListener;

impl MouseListener {
    // Constructor. If suppress is true, captured events must be swallowed.
    pub fn new<FMove, FClick, FScroll>(on_move: FMove, on_click: FClick, on_scroll: FScroll, suppress: bool) -> Self
    where
        FMove: Fn(i32, i32) + Send + Sync + 'static,
        FClick: Fn(i32, i32, String, bool) + Send + Sync + 'static,
        FScroll: Fn(i32, i32, i32, i32) + Send + Sync + 'static;

    // Starts the mouse tracking loop
    pub fn start(&self);
}
```

### Adding a New OS Backend

To support a new operating system or windowing system, implement a new backend module using these steps:

1. Create the source file under the hardware directory: `src/hardware/<your_os>.rs`.
2. Implement all structures and functions detailed above inside your new `src/hardware/<your_os>.rs` file.
3. Expose the new module in `src/hardware/mod.rs` using conditional compilation attributes:

```rust
#[cfg(target_os = "<your_os>")]
pub mod <your_os>;

#[cfg(target_os = "<your_os>")]
pub use <your_os>::{
    Clipboard, KeyboardController, KeyboardListener, MouseController, MouseListener, get_screeninfo,
    init_keyboard_layout,
};