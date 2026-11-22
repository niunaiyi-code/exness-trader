#!/bin/bash

# GL Server build & deploy script

set -e

echo "🔨 Building GL Server..."
cargo build --release -p gl-server

echo "🐳 Creating Docker image..."
docker build -t gl-trader:latest .

echo "✅ Build complete!"
echo ""
echo "Run with:"
echo "  docker run -p 8080:8080 -v ./config.toml:/app/config.toml gl-trader:latest"
echo ""
echo "Or systemd:"
echo "  sudo cp target/release/gl-server /usr/local/bin/"
echo "  sudo systemctl start gl-server"
