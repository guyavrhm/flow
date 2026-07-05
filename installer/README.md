# flow Installer & Packaging System

This directory contains the cross-platform packaging system for **flow**, used to compile the application and generate distributable installer packages.

---

## Prerequisites

Before running the installer builder, make sure you have the following prerequisites installed on your system:

### 1. General Python Dependencies
The build script automatically installs PyInstaller if it is missing, but make sure the main dependencies for `flow` are installed:
```bash
pip install -r requirements.txt
```

### 2. C Compiler (for compiling AES libraries)
- **macOS:** Xcode Command Line Tools (install by running `xcode-select --install` in terminal)
- **Linux:** GCC (install with `sudo apt install build-essential` or your distro equivalent)
- **Windows:** MinGW GCC or Microsoft Visual C++ Build Tools (make sure `gcc` is in your system PATH)

### 3. Installer Creation Utilities (Optional)
- **macOS:** Uses the built-in `hdiutil` utility to bundle the application into a `.dmg` file.
- **Linux:** Uses python's built-in `tarfile` module to generate `.tar.gz` and packages it with [setup_linux.sh](file:///Users/guyavraham/flow/installer/setup_linux.sh).
- **Windows:** To generate the `.setup.exe` installer, download and install [Inno Setup 6](https://jrsoftware.org/isdl.php). If Inno Setup is not found, the script will fall back to creating a `.zip` archive.

---

## How to Build

Run the [build.py](file:///Users/guyavraham/flow/installer/build.py) script using python:

```bash
python installer/build.py
```

### What this script does:
1. Validates build dependencies.
2. Compiles the C extension libraries (`aes.so` or `aes.dll`).
3. Packages the python application into a standalone executable tree using [flow.spec](file:///Users/guyavraham/flow/installer/flow.spec).
4. Packages the compiled binaries into the platform-specific target installer:
   - **macOS:** Produces a `.dmg` installer (`dist/flow.dmg`).
   - **Linux:** Produces a `.tar.gz` containing binary files, the launcher shortcut, and an installation script (`dist/flow.tar.gz`).
   - **Windows:** Produces a setup installer (`dist/flow.setup.exe` or `dist/flow-windows.zip`).

---

## File Overview

* [build.py](file:///Users/guyavraham/flow/installer/build.py) - Main build orchestrator.
* [flow.spec](file:///Users/guyavraham/flow/installer/flow.spec) - PyInstaller configuration file detailing bundled dependencies, C libraries, and static resources.
* [setup_linux.sh](file:///Users/guyavraham/flow/installer/setup_linux.sh) - Linux post-install script (bundled in `.tar.gz`) to install desktop shortcuts and bin files.
* [flow.desktop](file:///Users/guyavraham/flow/installer/flow.desktop) - Linux launcher menu shortcut.
* [flow.iss](file:///Users/guyavraham/flow/installer/flow.iss) - Inno Setup configuration script for compiling Windows installer packages.
