# Silvana

## Silvana node

### Features

- Coordination of proving and execution for Silvana Apps
- Silvana OS interface available thru gPRC locally for running agents
- Monitoring of the blockchain for new Silvana App jobs, and running them
- Monitoring: logs to file/console, logs/metrics via OpenTelemetry gRPC push to New Relic

### Hardware requirements

### Minimal hardware requirements

- 8 CPU
- 16 GB RAM
- 100 GB disk to hold docker images of the agents

### Optimal hardware requirements

- 16-32 CPU
- 32-64 GB RAM
- 200 GB disk to hold docker images of the agents

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
- **macOS Apple Silicon** (M1/M2/M3/M4)

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
curl -sSL https://raw.githubusercontent.com/SilvanaOne/silvana/main/install.sh | VERSION=v0.1.0 bash
```

## Uninstall

To remove Silvana:

```bash
sudo rm /usr/local/bin/silvana
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

## Running Silvana Node

### Quick Start

The simplest way to start a Silvana coordinator node:

```bash
silvana start
```

This command will:

1. **Fetch configuration** from the Silvana RPC server for your network (devnet by default)
2. **Auto-generate Sui keypair** if not present (saved to `.env`)
3. **Request funds** automatically from the devnet faucet (10 SUI)
4. **Initialize services** including job searcher, Docker processor, and gRPC server
5. **Start monitoring** for jobs on the blockchain

### First-Time Setup

When running `silvana start` for the first time without credentials:

```
🔄 Fetching configuration for chain: devnet
✅ Successfully fetched 16 configuration items
🔑 SUI credentials not found, generating new keypair...
✅ Generated new Sui keypair:
   Address: 0x3f176926a223d730fea3998da1791f4c7517e73bf3472e233a316d8672275683
📝 Saved credentials to .env file
💰 Requesting funds from devnet faucet...
✅ Faucet request successful!
   Transaction: GGRwF1ybif9nRsjiJEneBguQppzKLkWVzxSZmToj1mLH
   🔗 Explorer: https://suiscan.xyz/devnet/tx/GGRwF1ybif9nRsjiJEneBguQppzKLkWVzxSZmToj1mLH
```

### Configuration Options

#### Network Selection

Specify which network to use:

```bash
# Devnet (default)
silvana start --chain devnet

# Testnet
silvana start --chain testnet

# Mainnet
silvana start --chain mainnet
```

#### Using Existing Credentials

If you have existing Sui credentials, create a `.env` file:

```bash
# .env
SUI_ADDRESS=0x...your-address...
SUI_SECRET_KEY=suiprivkey1...your-private-key...
SUI_CHAIN=devnet
```

#### Registry Configuration

To join a specific Silvana registry (e.g., shared devnet):

```bash
# Get the shared devnet registry
silvana config

# Add to your .env:
SILVANA_REGISTRY=0x916a3b24de8165fb6fb25d060ec82b50683dc9e9cebf0dfae559f943ee32adb2
SILVANA_REGISTRY_PACKAGE=0x32f8ad21df94c28401912c8ffebcc3bd186f5bf7da0995057a63755005937025
```

### Running Modes

#### Standard Mode

Process all jobs from your registry:

```bash
silvana start
```

#### App Instance Filter

Process jobs only from a specific app instance:

```bash
silvana start --instance 0xaacf350ac6ae669ebf9804b455b0bc75a71f28a34bdc48d87ca78f1f90ba0f3b
```

#### Settlement-Only Mode

Run as a dedicated settlement node:

```bash
silvana start --settle
```

### What Happens During Startup

The coordinator performs these initialization steps:

1. **Configuration Loading**: Fetches and injects environment variables from RPC server
2. **Sui Connection**: Connects to the Sui blockchain RPC endpoint
3. **Balance Check**: Verifies sufficient SUI balance for operations
4. **Gas Coin Pool**: Splits large coins into smaller ones for better transaction performance
5. **Service Startup**:
   - **Job Searcher**: Monitors blockchain for new jobs
   - **Multicall Processor**: Batches operations for efficiency
   - **Docker Buffer**: Manages container execution
   - **Event Monitor**: Watches blockchain events
   - **gRPC Server**: Provides API for agent communication
   - **Periodic Tasks**: Reconciliation, block creation, proof analysis

### Monitoring Operations

Once running, you'll see logs indicating job processing:

```
📝 JobCreated: seq=4, dev=AddDeveloper, agent=AddAgent/prove, app_method=add
🐳 Starting Docker container for buffered job 1: AddDeveloper/AddAgent/prove
✅ Job 1 started and reserved for Docker session
```

### Troubleshooting

If the coordinator fails to start:

- **Missing credentials**: Will auto-generate on devnet, or check your `.env` file
- **Insufficient balance**: Run `silvana faucet sui --address 0x...`
- **Connection issues**: Verify network connectivity to RPC endpoints, gRPC TCP protocol is used for connection that requires streaming support and permanent TCP connection
- **Docker errors**: Ensure Docker daemon is running
- **Registry not found**: Check `SILVANA_REGISTRY` and `SILVANA_REGISTRY_PACKAGE` values

### Next Steps

After starting your coordinator:

1. **Create a new project**: `silvana new myproject`
2. **Deploy an application**: Follow the [Add Example](examples/add/README.md)
3. **Monitor jobs**: Use `silvana jobs --instance <your-app-instance>`
4. **Check balances**: `silvana balance sui`

For detailed application development, see the [examples documentation](examples/add/README.md).

## Silvana RPC

![Silvana Networking](docs/Silvana%20Networking.png)

### Features

- gRPC server and client: gRPC, gRPC-Web
- TiDB Serverless database: store events, query, fulltext search
- NATS JetStream on nats and wss
- Internal buffering in memory for batch processing
- Protobuf definitions and reflection on endpoint and in rust code
- Monitoring: logs to file/console, logs/metrics via OpenTelemetry gRPC push/REST pull, Grafana/Prometheus/BetterStack support for logs and dashboards

### Hardware requirements

- 1 CPU
- 500 MB RAM
- 10 GB disk

Can run on t4g.nano ($3 per month)

### Performance

- 100-200 ms for any operation, including adding event, query event, fulltext search
- 10,000 events per second
- Consumes 200 MB RAM on low load, 300 MB RAM for a million events
- Typical CPU load is less than 20-30% on thousands of events per second

### Deployment

#### Cross-build rust executable using docker and upload it to S3

```sh
make build-rpc
```

#### Run [pulumi](infra/pulumi-rpc/index.ts) script to:

- Create AWS Stack, including EC2 instance
- Install certificates, NATS, Silvana RPC
- Configure and run services

```sh
pulumi up
```

### Protobuf workflow

- Create [proto definitions](proto)
- Compile with `make regen` for Rust and `buf lint && buf generate` for TypeScript - definitions will be compiled to [SQL](proto/sql/events.sql), [SQL migrations](migrations), Rust [interfaces](crates/proto) with reflection and server/client, TypeScript [interfaces](clients/grpc-node/src/proto) and client, [sea-orm interfaces for TiDB](crates/tidb/src/entity)

### Examples of clients

- [node example](clients/grpc-node)
- [web example](clients/grpc-web) - https://grpc-web.silvana.dev
