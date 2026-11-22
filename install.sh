#!/bin/bash

# Installation script for systemd deployment

set -e

BINARY_PATH="./target/release/gl-server"
INSTALL_DIR="/usr/local/bin"
CONFIG_DIR="/etc/gl-trader"
SERVICE_FILE="gl-trader.service"

echo "📦 Installing GL Server..."

if [ ! -f "$BINARY_PATH" ]; then
    echo "❌ Binary not found. Run './build.sh' first."
    exit 1
fi

# Create user
sudo useradd -r -s /bin/false gl-trader || true

# Create config directory
sudo mkdir -p "$CONFIG_DIR"
sudo cp config.example.toml "$CONFIG_DIR/config.toml"
sudo chown -R gl-trader:gl-trader "$CONFIG_DIR"

# Install binary
sudo cp "$BINARY_PATH" "$INSTALL_DIR/"
sudo chmod +x "$INSTALL_DIR/gl-server"

# Install systemd service
sudo cp "$SERVICE_FILE" /etc/systemd/system/
sudo systemctl daemon-reload

echo ""
echo "✅ Installation complete!"
echo ""
echo "Commands:"
echo "  sudo systemctl start gl-trader      # Start service"
echo "  sudo systemctl status gl-trader     # Check status"
echo "  sudo systemctl enable gl-trader     # Auto-start on boot"
echo "  sudo journalctl -u gl-trader -f     # Watch logs"
echo ""
echo "⚙️  Edit config: sudo nano $CONFIG_DIR/config.toml"
