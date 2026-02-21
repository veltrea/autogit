#!/bin/bash

# AutoGit Setup Script for Mac

BINARY="./target/release/autogit"
INSTALL_DIR="/usr/local/bin"

if [ -f "$BINARY" ]; then
    echo "Changing permissions for $BINARY..."
    chmod +x "$BINARY"
    
    echo "Do you want to install autogit globally to $INSTALL_DIR? (y/n)"
    read -r response
    if [ "$response" = "y" ]; then
        echo "Installing to $INSTALL_DIR (requires sudo)..."
        sudo cp "$BINARY" "$INSTALL_DIR/"
        echo "[V] Global installation complete! You can now use 'autogit' from any directory."
    else
        echo "[V] Permissions updated locally. Run using: $BINARY"
    fi
else
    echo "[X] Error: Binary not found at $BINARY"
    echo "Please run 'cargo build --release' first."
    exit 1
fi
