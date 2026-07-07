#!/bin/bash
# setup_linux.sh - Installer for flow KVM client/server on Linux systems.

set -e

# Determine installation paths based on privileges
if [ "$EUID" -eq 0 ]; then
    # System-wide installation
    BIN_DIR="/usr/local/bin"
    APP_DIR="/usr/share/applications"
    ICON_DIR="/usr/share/icons/hicolor/256x256/apps"
    echo "Installing flow system-wide..."
else
    # User-space installation
    BIN_DIR="$HOME/.local/bin"
    APP_DIR="$HOME/.local/share/applications"
    ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"
    echo "Installing flow for current user ($USER)..."
fi

# Create target directories if they don't exist
mkdir -p "$BIN_DIR"
mkdir -p "$APP_DIR"
mkdir -p "$ICON_DIR"

# Check if pre-compiled files exist in the unpacked directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DIST_DIR="$SCRIPT_DIR/flow"

if [ ! -d "$DIST_DIR" ]; then
    echo "Error: Could not find compiled flow directory at '$DIST_DIR'."
    echo "Please run build.py to generate the build folder first, or verify the tar.gz contents."
    exit 1
fi

# Copy application files
echo "Copying application folder..."
INSTALL_OPT_DIR="/opt/flow"
if [ "$EUID" -eq 0 ]; then
    mkdir -p "$INSTALL_OPT_DIR"
    cp -r "$DIST_DIR"/* "$INSTALL_OPT_DIR/"
    # Create symlink in bin directory
    ln -sf "$INSTALL_OPT_DIR/flow" "$BIN_DIR/flow"
else
    # For local user, we can store it in ~/.local/share/flow
    INSTALL_OPT_DIR="$HOME/.local/share/flow"
    mkdir -p "$INSTALL_OPT_DIR"
    cp -r "$DIST_DIR"/* "$INSTALL_OPT_DIR/"
    # Create symlink in bin directory
    ln -sf "$INSTALL_OPT_DIR/flow" "$BIN_DIR/flow"
fi

# Copy icon
if [ -f "$DIST_DIR/src/resources/flow.png" ]; then
    cp "$DIST_DIR/src/resources/flow.png" "$ICON_DIR/flow.png"
fi

# Copy desktop entry
if [ -f "$SCRIPT_DIR/flow.desktop" ]; then
    cp "$SCRIPT_DIR/flow.desktop" "$APP_DIR/"
    # Update Exec and Icon lines in desktop entry if installed locally
    if [ "$EUID" -ne 0 ]; then
        sed -i "s|Exec=flow|Exec=$BIN_DIR/flow|g" "$APP_DIR/flow.desktop"
        sed -i "s|Icon=flow|Icon=$ICON_DIR/flow.png|g" "$APP_DIR/flow.desktop"
    else
        sed -i "s|Exec=flow|Exec=/usr/local/bin/flow|g" "$APP_DIR/flow.desktop"
        sed -i "s|Icon=flow|Icon=$ICON_DIR/flow.png|g" "$APP_DIR/flow.desktop"
    fi
    chmod +x "$APP_DIR/flow.desktop"
fi

# Configure udev rules for Wayland / evdev input sharing
echo "Configuring input device permissions for Wayland support..."
UDEV_RULE_FILE="/etc/udev/rules.d/99-flow-uinput.rules"

if [ "$EUID" -eq 0 ]; then
    # Write udev rule
    echo 'KERNEL=="uinput", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput"' > "$UDEV_RULE_FILE"
    
    # Reload udev
    udevadm control --reload-rules && udevadm trigger
    
    # Check if there is a non-root user running the script via sudo
    if [ -n "$SUDO_USER" ]; then
        usermod -aG input "$SUDO_USER"
        echo "User '$SUDO_USER' has been added to the 'input' group."
    fi
    echo "udev rules and input group permissions configured successfully."
else
    echo ""
    echo "========================================================================="
    echo "Wayland Support Permissions Notice:"
    echo "To run flow under Wayland, the app needs access to /dev/uinput and /dev/input/*."
    echo "Please run this installer as root (sudo) to configure permissions automatically, or manually:"
    echo "  1. Add rule: echo 'KERNEL==\"uinput\", GROUP=\"input\", MODE=\"0660\", OPTIONS+=\"static_node=uinput\"' | sudo tee $UDEV_RULE_FILE"
    echo "  2. Add user to input group: sudo usermod -aG input \$USER"
    echo "  3. Reload rules: sudo udevadm control --reload-rules && sudo udevadm trigger"
    echo "  4. Log out and log back in to apply group changes."
    echo "========================================================================="
    echo ""
fi

echo "Installation complete!"
echo "You can now run flow from your application menu or by typing 'flow' in the terminal."

