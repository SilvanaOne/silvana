#!/bin/bash

# Silvana Installation Script
# Usage: curl -sSL https://raw.githubusercontent.com/SilvanaOne/silvana/main/install.sh | bash

set -e

# Configuration
REPO="SilvanaOne/silvana"
INSTALL_DIR="/usr/local/bin"
BINARY_NAME="silvana"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to print colored output
print_error() { echo -e "${RED}✗ $1${NC}" >&2; }
print_success() { echo -e "${GREEN}✓ $1${NC}"; }
print_info() { echo -e "${YELLOW}→ $1${NC}"; }

# Detect OS and Architecture
detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"
    
    case "$OS" in
        Linux*)
            OS="linux"
            ;;
        Darwin*)
            OS="macos"
            ;;
        *)
            print_error "Unsupported operating system: $OS"
            exit 1
            ;;
    esac
    
    case "$ARCH" in
        x86_64|amd64)
            if [ "$OS" = "linux" ]; then
                PLATFORM="x86_64-linux"
            else
                print_error "macOS x86_64 is not supported. Please use macOS Apple Silicon version."
                exit 1
            fi
            ;;
        aarch64|arm64)
            if [ "$OS" = "linux" ]; then
                PLATFORM="arm64-linux"
            elif [ "$OS" = "macos" ]; then
                PLATFORM="macos-silicon"
            fi
            ;;
        *)
            print_error "Unsupported architecture: $ARCH"
            exit 1
            ;;
    esac
    
    print_info "Detected platform: $OS $ARCH → $PLATFORM"
}

# Get the latest release version or use provided version
get_version() {
    if [ -n "$VERSION" ]; then
        print_info "Using specified version: $VERSION"
    else
        print_info "Fetching latest release version..."
        VERSION=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
        
        if [ -z "$VERSION" ]; then
            print_error "Failed to fetch latest release version"
            exit 1
        fi
        
        print_info "Latest version: $VERSION"
    fi
}

# Download and install binary
install_silvana() {
    local url="https://github.com/$REPO/releases/download/$VERSION/silvana-$PLATFORM.tar.gz"
    local temp_dir=$(mktemp -d)
    
    print_info "Downloading Silvana $VERSION for $PLATFORM..."
    print_info "URL: $url"
    
    # Download the archive
    if ! curl -L -o "$temp_dir/silvana.tar.gz" "$url"; then
        print_error "Failed to download Silvana"
        rm -rf "$temp_dir"
        exit 1
    fi
    
    # Extract the archive
    print_info "Extracting archive..."
    if ! tar -xzf "$temp_dir/silvana.tar.gz" -C "$temp_dir"; then
        print_error "Failed to extract archive"
        rm -rf "$temp_dir"
        exit 1
    fi
    
    # Check if we need sudo for installation
    if [ -w "$INSTALL_DIR" ]; then
        SUDO=""
    else
        SUDO="sudo"
        print_info "Installation requires sudo privileges"
    fi
    
    # Install the binary
    print_info "Installing to $INSTALL_DIR/$BINARY_NAME..."
    if ! $SUDO mv "$temp_dir/silvana" "$INSTALL_DIR/$BINARY_NAME"; then
        print_error "Failed to install binary"
        rm -rf "$temp_dir"
        exit 1
    fi
    
    # Make it executable
    $SUDO chmod +x "$INSTALL_DIR/$BINARY_NAME"
    
    # Clean up
    rm -rf "$temp_dir"
    
    print_success "Silvana $VERSION installed successfully!"
}

# Verify installation
verify_installation() {
    if command -v silvana >/dev/null 2>&1; then
        print_success "Installation verified: $(silvana --version 2>/dev/null || echo 'silvana installed')"
        print_info "Run 'silvana --help' to get started"
    else
        print_error "Installation verification failed"
        print_info "You may need to add $INSTALL_DIR to your PATH"
        exit 1
    fi
}

# Main installation flow
main() {
    echo "🚀 Silvana Installer"
    echo "===================="
    echo
    
    detect_platform
    get_version
    install_silvana
    verify_installation
    
    echo
    echo "🎉 Installation complete!"
}

main "$@"