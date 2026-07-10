# Contributing to flow

Thank you for your interest in contributing to **flow**! This guide outlines how to set up your local development environment, compile from source, run the test suites, and submit contributions.

For detailed technical explanations of the codebase architecture, thread communication, and platform-specific FFI implementations, please review the **[ARCHITECTURE.md](file:///Users/guyavraham/flow/ARCHITECTURE.md)** guide.

---

## 1. Getting Started & Setup

### Prerequisites
To build and test the Rust port of `flow`, you need the Rust compiler toolchain installed.

* **Install Rust:** 
  Run the following command in your terminal, or visit [rustup.rs](https://rustup.rs):
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```

* **Linux Dependencies:**
  If compiling on Linux (X11), you must install the GTK3 development headers:
  ```bash
  # Ubuntu / Debian
  sudo apt-get update
  sudo apt-get install libgtk-3-dev build-essential
  ```

* **macOS Permissions:**
  Since KVM input redirection uses system event taps (`CGEventTap`), your terminal or IDE (e.g. VS Code, Cargo) must be granted **Accessibility Permissions** in macOS *System Settings -> Privacy & Security -> Accessibility* in order to run and debug the server local input hooks.

---

## 2. Common Development Commands

Use standard Cargo utilities to manage the build lifecycle:

```bash
# Clone the repository
git clone https://github.com/guyavrhm/flow.git
cd flow

# Build the project (Debug profile)
cargo build

# Build the project (Release profile with optimizations)
cargo build --release

# Run the application locally
cargo run

# Run all unit and integration tests
cargo test
```

---

## 3. Contribution Workflow

1. **Fork the Repository:** Create a personal fork on GitHub.
2. **Create a Feature Branch:** Branch off from `rust-port` (or `dev` if writing Python code). Use a descriptive name:
   ```bash
   git checkout -b feat/my-new-feature
   ```
3. **Commit Your Changes:** Keep commits clean and write meaningful commit messages.
4. **Run Tests:** Ensure that `cargo test` passes successfully before submitting.
5. **Open a Pull Request:** Open a PR against the upstream branch. Explain the goal of the changes, any design trade-offs, and test results.

---

## 4. Coding Standards

* **FFI Boundaries:** Any direct calls to native operating system APIs must be wrapped in safe Rust boundaries. Keep `unsafe {}` blocks as small as possible and add comments explaining the safety invariants.
* **Thread Safety:** Ensure all data shared across execution threads is wrapped securely (typically using `Arc<Mutex<T>>`). Do not introduce unprotected data access patterns.
* **Compatibility:** If making updates to the network protocol structures (`InputEvent`, `ClipboardPayload`), run compatibility test suites to ensure they work correctly with existing clients.
