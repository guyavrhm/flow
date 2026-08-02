#!/bin/bash
# setup_linux.sh - Professional Installer for flow KVM on Linux systems (X11 & Wayland)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Find compiled binary (dist folder, release build, debug build, or local bundle)
if [ -f "$SCRIPT_DIR/flow" ]; then
    BINARY_PATH="$SCRIPT_DIR/flow"
elif [ -f "$PROJECT_ROOT/target/release/flow" ]; then
    BINARY_PATH="$PROJECT_ROOT/target/release/flow"
elif [ -f "$PROJECT_ROOT/target/debug/flow" ]; then
    BINARY_PATH="$PROJECT_ROOT/target/debug/flow"
else
    echo "Error: Could not find compiled flow binary."
    echo "Please build the project using 'cargo build --release' or 'make build' first."
    exit 1
fi

# Find resources folder
if [ -d "$SCRIPT_DIR/resources" ]; then
    RESOURCES_PATH="$SCRIPT_DIR/resources"
elif [ -d "$PROJECT_ROOT/resources" ]; then
    RESOURCES_PATH="$PROJECT_ROOT/resources"
else
    RESOURCES_PATH=""
fi

# Determine installation paths based on privileges
if [ "$EUID" -eq 0 ]; then
    BIN_DIR="/usr/local/bin"
    APP_DIR="/usr/share/applications"
    ICON_DIR="/usr/share/icons/hicolor/256x256/apps"
    INSTALL_OPT_DIR="/opt/flow"
    echo "==> Installing flow system-wide..."
else
    BIN_DIR="$HOME/.local/bin"
    APP_DIR="$HOME/.local/share/applications"
    ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
    INSTALL_OPT_DIR="$HOME/.local/share/flow"
    echo "==> Installing flow for user ($USER)..."
fi

# Create target directories
mkdir -p "$BIN_DIR"
mkdir -p "$APP_DIR"
mkdir -p "$ICON_DIR"
mkdir -p "$INSTALL_OPT_DIR"

# Copy binary and resources
echo "==> Copying binary & resources to $INSTALL_OPT_DIR..."
cp "$BINARY_PATH" "$INSTALL_OPT_DIR/flow"
chmod +x "$INSTALL_OPT_DIR/flow"

if [ -n "$RESOURCES_PATH" ] && [ -d "$RESOURCES_PATH" ]; then
    cp -r "$RESOURCES_PATH" "$INSTALL_OPT_DIR/"
fi

# Create symlink in bin directory
ln -sf "$INSTALL_OPT_DIR/flow" "$BIN_DIR/flow"

# Copy icon if available
if [ -f "$INSTALL_OPT_DIR/resources/flow.png" ]; then
    cp "$INSTALL_OPT_DIR/resources/flow.png" "$ICON_DIR/flow.png"
fi

# Install desktop shortcut
DESKTOP_SRC=""
if [ -f "$SCRIPT_DIR/flow.desktop" ]; then
    DESKTOP_SRC="$SCRIPT_DIR/flow.desktop"
elif [ -f "$PROJECT_ROOT/installer/flow.desktop" ]; then
    DESKTOP_SRC="$PROJECT_ROOT/installer/flow.desktop"
fi

if [ -n "$DESKTOP_SRC" ]; then
    echo "==> Installing desktop shortcut..."
    cp "$DESKTOP_SRC" "$APP_DIR/flow.desktop"
    sed -i "s|Exec=flow|Exec=$BIN_DIR/flow|g" "$APP_DIR/flow.desktop"
    if [ -f "$ICON_DIR/flow.png" ]; then
        sed -i "s|Icon=flow|Icon=$ICON_DIR/flow.png|g" "$APP_DIR/flow.desktop"
    fi
    chmod +x "$APP_DIR/flow.desktop"
fi

# Configure udev rules for /dev/uinput (TAG+="uaccess" & group input for desktop access)
UDEV_RULE_FILE="/etc/udev/rules.d/99-flow-uinput.rules"
UDEV_CONTENT='KERNEL=="uinput", SUBSYSTEM=="misc", GROUP="input", MODE="0660", TAG+="uaccess", OPTIONS+="static_node=uinput"'

echo "==> Configuring udev input permissions for Wayland & X11 evdev support..."

install_udev_rule() {
    echo "$UDEV_CONTENT" > "$UDEV_RULE_FILE"
    udevadm control --reload-rules && udevadm trigger
}

if [ "$EUID" -eq 0 ]; then
    install_udev_rule
    echo "==> udev input rules configured successfully."
else
    # Non-root user: attempt pkexec or sudo to write udev rule
    if command -v pkexec >/dev/null 2>&1; then
        echo "==> Requesting system authorization to install udev input rules..."
        pkexec bash -c "echo '$UDEV_CONTENT' > '$UDEV_RULE_FILE' && udevadm control --reload-rules && udevadm trigger" || {
            echo "Warning: Administrator permission was denied. You may need to run setup with sudo to grant /dev/uinput permissions."
        }
    elif command -v sudo >/dev/null 2>&1; then
        echo "==> Requesting sudo access to install udev input rules..."
        sudo bash -c "echo '$UDEV_CONTENT' > '$UDEV_RULE_FILE' && udevadm control --reload-rules && udevadm trigger" || {
            echo "Warning: Sudo authentication failed. You may need to run setup with sudo to grant /dev/uinput permissions."
        }
    else
        echo "Warning: Could not elevate privileges automatically. Please run 'sudo ./installer/setup_linux.sh' to finish input setup."
    fi
fi

echo ""
echo "========================================================================="
echo "  flow installation complete!"
echo "  Executable: $BIN_DIR/flow"
echo "  Application Launcher: $APP_DIR/flow.desktop"
echo "========================================================================="
echo ""
