# Health Metrics Crate

System health metrics collection and reporting with JWT authentication.

## Features

- **Dual Usage**: Works as both a library and standalone binary
- **Metrics Collection**: CPU, memory, and disk usage monitoring
- **JWT Authentication**: Ed25519-signed tokens for secure metric submission
- **Automatic Retry**: Exponential backoff on network failures
- **Configurable Intervals**: Customizable collection frequency (default: 10 minutes)
- **Production Ready**: Comprehensive error handling and logging

## Quick Start

### As Standalone Binary

```bash
# Generate keypair
cargo run -p health --example generate_keypair

# Create JWT token
cargo run -p health --example create_jwt

# Run health exporter (set JWT_HEALTH environment variable first)
export JWT_HEALTH="your-jwt-token-here"
cargo run -p health

# Or provide JWT via CLI
cargo run -p health -- --jwt "your-jwt-token" --interval 300
```

### As Library

```rust
use health::start_health_exporter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Start with default 10-minute interval
    let handle = start_health_exporter(None)?;

    // Wait for completion
    handle.await?;
    Ok(())
}
```

Add to `Cargo.toml` (minimal dependencies, no CLI):
```toml
[dependencies]
health = { path = "../health", default-features = false }
```

## CLI Options

```
Usage: health [OPTIONS]

Options:
  --jwt <JWT>              JWT token for authentication [env: JWT_HEALTH]
  --interval <INTERVAL>    Collection interval in seconds (default: 600)
  --log-level <LOG_LEVEL>  Log level: trace, debug, info, warn, error [default: info]
  -h, --help              Print help
  -V, --version           Print version
```

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `JWT_HEALTH` | JWT token containing url, id, and public key claims | *Required* |
| `RUST_LOG` | Log level (trace, debug, info, warn, error) | `info` |
| `HEALTH_INTERVAL` | Collection interval in seconds | `600` (10 min) |

## JWT Token Format

The JWT token must include these claims:

- **url**: Endpoint URL to send metrics to (e.g., `https://api.example.com/health`)
- **id**: Unique node/instance identifier (e.g., `node-001`)
- **sub**: Ed25519 public key in hex format (for signature verification)
- **exp**: Expiration timestamp (Unix seconds)
- **iat**: Issued-at timestamp (Unix seconds)

## Examples

### Generate Ed25519 Keypair

```bash
cargo run -p health --example generate_keypair
```

Output:
```
🔐 Health Metrics JWT Keypair Generator
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

🔑 PRIVATE KEY (Secret - Keep Secure!)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
<64-char-hex-string>

🔓 PUBLIC KEY
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
<64-char-hex-string>
```

### Create JWT Token

```bash
cargo run -p health --example create_jwt
```

### Run Health Exporter

```bash
# With environment variable
export JWT_HEALTH="eyJ0eXAiOiJKV1QiLCJhbGc..."
cargo run --release -p health

# Or with CLI argument
cargo run --release -p health -- --jwt "eyJ0eXAiOiJKV1QiLCJhbGc..."

# Custom interval (5 minutes)
cargo run -p health -- --interval 300

# Debug logging
RUST_LOG=debug cargo run -p health
```

## Production Deployment

### Build Release Binary

```bash
cargo build --release -p health
```

The binary will be at: `target/release/health`

### Systemd Service

Create `/etc/systemd/system/health-exporter.service`:

```ini
[Unit]
Description=Silvana Health Metrics Exporter
After=network.target

[Service]
Type=simple
User=silvana
WorkingDirectory=/opt/silvana
Environment="JWT_HEALTH=your-jwt-token-here"
Environment="RUST_LOG=info"
ExecStart=/opt/silvana/bin/health
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

Enable and start:
```bash
sudo systemctl enable health-exporter
sudo systemctl start health-exporter
sudo systemctl status health-exporter
```

## Library API

### Main Functions

```rust
// Start health metrics exporter
pub fn start_health_exporter(interval_secs: Option<u64>)
    -> Result<tokio::task::JoinHandle<()>>

// Generate Ed25519 keypair
pub fn generate_ed25519_keypair() -> Ed25519Keypair

// Create JWT token
pub fn create_health_jwt(
    secret_key_bytes: &[u8; 32],
    public_key_hex: &str,
    url: &str,
    id: &str,
    exp: u64,
) -> Result<String>

// Verify JWT token
pub fn verify_health_jwt(
    token: &str,
    public_key_bytes: &[u8; 32]
) -> Result<HealthClaims>

// Decode JWT without verification
pub fn decode_health_jwt(token: &str) -> Result<HealthClaims>

// Collect metrics once
pub fn collect_health_metrics() -> HealthMetrics

// Send metrics manually
pub async fn send_health_metrics(
    url: &str,
    jwt_token: &str,
    metrics: &HealthMetrics,
    config: &ExporterConfig,
) -> Result<()>
```

### Types

```rust
pub struct Ed25519Keypair {
    pub private_key_hex: String,
    pub public_key_hex: String,
}

pub struct HealthMetrics {
    pub timestamp: String,
    pub cpu: CpuMetrics,
    pub memory: MemoryMetrics,
    pub disks: Vec<DiskMetrics>,
}

pub struct CpuMetrics {
    pub usage_percent: f32,
}

pub struct MemoryMetrics {
    pub used_bytes: u64,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub usage_percent: f64,
}

pub struct DiskMetrics {
    pub name: String,
    pub mount_point: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub usage_percent: f64,
}

pub struct HealthClaims {
    pub url: String,
    pub id: String,
    pub sub: String,  // Public key hex
    pub exp: u64,     // Expiration
    pub iat: u64,     // Issued at
}
```

## Security Notes

1. **Private Key**: Keep secret, never commit to version control
2. **Public Key**: Included in JWT token (sub claim) for verification
3. **JWT Expiration**: Exporter automatically stops when token expires
4. **HTTPS Only**: Production deployments should use HTTPS endpoints
5. **Secrets Management**: Consider AWS Secrets Manager for production

## Testing

```bash
# Run all tests
cargo test -p health

# Run specific test
cargo test -p health test_generate_ed25519_keypair

# Test with output
cargo test -p health -- --nocapture
```

## Dependencies

- `sysinfo`: System metrics collection
- `ed25519-dalek`: Ed25519 cryptography
- `jsonwebtoken`: JWT encoding/decoding
- `reqwest`: HTTP client
- `tokio`: Async runtime
- `serde`: Serialization
- `tracing`: Logging

Optional (binary only):
- `clap`: CLI argument parsing
- `dotenvy`: .env file loading
- `tracing-subscriber`: Log formatting

## License

Apache-2.0

## Version

0.3.1
