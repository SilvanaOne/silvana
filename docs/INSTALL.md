# Installation

## Quick Install (Recommended)

Install the latest version of Silvana with a single command:

```bash
curl -sSL https://raw.githubusercontent.com/SilvanaOne/silvana/main/install.sh | bash
```

Or with wget:

```bash
wget -qO- https://raw.githubusercontent.com/SilvanaOne/silvana/main/install.sh | bash
```

This script will:
- Automatically detect your OS (Linux or macOS) and architecture (ARM64 or x86_64)
- Download the appropriate binary from the latest GitHub release
- Install it to `/usr/local/bin/silvana`
- Verify the installation

### Supported Platforms

- **Linux ARM64** (aarch64)
- **Linux x86_64** (amd64)
- **macOS Apple Silicon** (M1/M2/M3)

## Manual Installation

If you prefer to install manually:

1. Go to the [releases page](https://github.com/SilvanaOne/silvana/releases)
2. Download the appropriate archive for your platform:
   - `silvana-arm64-linux.tar.gz` for Linux ARM64
   - `silvana-x86_64-linux.tar.gz` for Linux x86_64  
   - `silvana-macos-silicon.tar.gz` for macOS Apple Silicon
3. Extract and install:

```bash
# Download (replace with your platform's file)
curl -LO https://github.com/SilvanaOne/silvana/releases/latest/download/silvana-arm64-linux.tar.gz

# Extract
tar -xzf silvana-arm64-linux.tar.gz

# Install
sudo mv silvana /usr/local/bin/

# Make executable
sudo chmod +x /usr/local/bin/silvana

# Verify
silvana --version
```

## Install Specific Version

To install a specific version instead of the latest:

```bash
curl -sSL https://raw.githubusercontent.com/SilvanaOne/silvana/main/install.sh | VERSION=v1.0.0 bash
```

## Uninstall

To remove Silvana:

```bash
sudo rm /usr/local/bin/silvana
```

## Docker Alternative

You can also run Silvana using Docker without installing:

```bash
docker run --rm ghcr.io/silvanaone/silvana:latest --help
```

## Verify Installation

After installation, verify it works:

```bash
silvana --version
silvana --help
```

## Troubleshooting

### Permission Denied
If you get a permission error, make sure the binary is executable:
```bash
sudo chmod +x /usr/local/bin/silvana
```

### Command Not Found
If `silvana` is not found after installation, add `/usr/local/bin` to your PATH:
```bash
export PATH=$PATH:/usr/local/bin
echo 'export PATH=$PATH:/usr/local/bin' >> ~/.bashrc  # or ~/.zshrc
```

### SSL/TLS Errors
If you encounter SSL errors during download, you can use the insecure flag (not recommended for production):
```bash
curl -sSLk https://raw.githubusercontent.com/SilvanaOne/silvana/main/install.sh | bash
```