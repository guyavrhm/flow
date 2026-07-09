# flow Installer & Packaging System (Rust version)

This directory contains configuration files and deployment scripts used to bundle, install, and distribute **flow** KVM packages across different operating systems.

---

## Deployment Architectures

For Rust, binaries compile natively to target platforms. The releases are built in CI via GitHub Actions and packaged into standard distributable archives.

### 1. macOS (Apple Silicon & Intel)
* **Packaging**: Built in CI using `cargo build --release`.
* **Output**: Redirection binary + [resources/](file:///Users/guyavraham/flow/resources) packed into a `.tar.gz` archive.

### 2. Windows
* **Packaging**: Compiled using `cargo build --release` producing `target/release/flow.exe`.
* **Installer**: Built using [Inno Setup 6](https://jrsoftware.org/isdl.php) with the [flow.iss](file:///Users/guyavraham/flow/installer/flow.iss) configuration file.
* **Output**: Produces a windows installation setup wizard `flow.setup.exe`.

### 3. Linux
* **Packaging**: Compiled natively under Ubuntu/Debian targets, generating `target/release/flow`.
* **Installer**: Deployed as a `.tar.gz` package carrying the binary, [flow.desktop](file:///Users/guyavraham/flow/installer/flow.desktop) menu entry, and the [setup_linux.sh](file:///Users/guyavraham/flow/installer/setup_linux.sh) script.
* **Installation**: Extract and run:
  ```bash
  tar -xzf flow-linux-*.tar.gz
  sudo ./setup_linux.sh
  ```

---

## File Overview

* [flow.iss](file:///Users/guyavraham/flow/installer/flow.iss) - Inno Setup configuration script for compiling Windows installer packages.
* [setup_linux.sh](file:///Users/guyavraham/flow/installer/setup_linux.sh) - Linux installation bash script to copy binary files, resources, launcher shortcut, and set up Wayland/udev permissions.
* [flow.desktop](file:///Users/guyavraham/flow/installer/flow.desktop) - Linux desktop launcher menu configuration.
