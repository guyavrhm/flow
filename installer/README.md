# flow Installer & Packaging System (Rust version)

This directory contains configuration files, installers, and deployment scripts used to bundle, install, and distribute **flow** KVM packages across Linux and Windows operating systems.

---

## Deployment & Installation

### 1. Linux Installer (`setup_linux.sh`)
* **Features**:
  * Automatically installs binary and resources to `~/.local/share/flow` (or `/opt/flow` if system-wide).
  * Creates environment symlinks (`~/.local/bin/flow`) and desktop application launcher (`flow.desktop`).
  * **Zero-Touch Input Permissions**: Installs `/etc/udev/rules.d/99-flow-uinput.rules` with `TAG+="uaccess"` for dynamic desktop user access. Uses `pkexec` (Graphical PolicyKit dialog) or `sudo` to prompt for system authorization seamlessly.
* **Usage**:
  * Run locally: `./installer/setup_linux.sh`
  * Or package for distribution: `make package-linux` (generates `flow-linux-x86_64.tar.gz`).

### 2. Windows Installer (`flow.iss`)
* **Installer**: Built using [Inno Setup 6](https://jrsoftware.org/isdl.php) with [flow.iss](file:///home/guyavrhm/flow/installer/flow.iss).
* **Output**: Generates a standard Windows installation setup wizard `flow.setup.exe`.

---

## File Overview

* [setup_linux.sh](file:///home/guyavrhm/flow/installer/setup_linux.sh) - Linux installation bash script to copy binary files, resources, launcher shortcut, and configure `udev` input permissions via PolicyKit.
* [flow.desktop](file:///home/guyavrhm/flow/installer/flow.desktop) - Linux desktop launcher shortcut specification.
* [flow.iss](file:///home/guyavrhm/flow/installer/flow.iss) - Inno Setup configuration script for compiling Windows installer packages.
