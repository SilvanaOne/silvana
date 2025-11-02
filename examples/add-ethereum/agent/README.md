# Add Ethereum Agent

A Rust agent that processes jobs from the Silvana coordinator service via gRPC for the AddApp example.

## Overview

This agent demonstrates the Silvana agent pattern for processing jobs through the coordinator service. It communicates via gRPC (similar to the TypeScript agent) and processes three types of operations:

- **init**: Initialize the application state (mocked with 10s delay)
- **add**: Process addition operations (mocked with 10s delay)
- **settle**: Settle state to settlement chain (mocked with 10s delay)

All jobs are mocked implementations that simply wait 10 seconds before marking the job as complete. This serves as a template for building real agents with actual business logic.

## Architecture

```
┌─────────────────────┐
│  AddApp (Ethereum)  │
│  Creates Jobs       │
└──────────┬──────────┘
           │
           │ On-chain job creation
           │
┌──────────▼──────────┐
│  Coordinator        │
│  Service (gRPC)     │
└──────────┬──────────┘
           │
           │ gRPC: getJob(), completeJob(), failJob()
           │
┌──────────▼──────────┐
│  Agent (Rust)       │
│  - Polls for jobs   │
│  - Processes jobs   │
│  - Reports results  │
└─────────────────────┘
```

The agent **does not** interact directly with Ethereum. Instead:
1. Jobs are created on-chain by the AddApp contract
2. The coordinator service monitors the blockchain and makes jobs available via gRPC
3. The agent polls the coordinator for jobs, processes them, and reports completion

## Prerequisites

- Rust 1.75 or later
- Running coordinator service (gRPC server on port 50051)
- Deployed AddApp contract (creates jobs on-chain)

## Quick Start

### 1. Start the Coordinator Service

The coordinator service must be running and accessible:

```bash
# Default: http://localhost:50051
# Or configure in config.toml
```

### 2. Configure the Agent

Edit `config.toml`:

```toml
# Coordinator gRPC URL
coordinator_url = "http://localhost:50051"  # or "http://host.docker.internal:50051" for Docker

# Session ID (unique per agent instance)
session_id = "add-ethereum-session-1"

# Agent identification
developer = "silvana"
agent = "add-ethereum-agent"
agent_method = "process"

# Polling settings
poll_interval_secs = 5
max_runtime_secs = 0  # 0 = run forever
```

### 3. Run the Agent

```bash
cargo run --bin agent
```

The agent will:
- Connect to the coordinator service via gRPC
- Poll for pending jobs every 5 seconds
- Process jobs (mock: wait 10 seconds)
- Mark jobs as completed or failed
- Continue running until you press Ctrl+C

## Configuration

The agent can be configured via `config.toml` or environment variables.

### config.toml

```toml
# Coordinator gRPC URL
coordinator_url = "http://host.docker.internal:50051"

# Session ID for this agent instance
session_id = "add-ethereum-session-1"

# Developer identifier
developer = "silvana"

# Agent identifier
agent = "add-ethereum-agent"

# Agent method name
agent_method = "process"

# Polling interval in seconds
poll_interval_secs = 5

# Maximum runtime in seconds (0 = run forever)
max_runtime_secs = 0
```

### Environment Variables

For Docker or production deployments:

```bash
export COORDINATOR_URL="http://host.docker.internal:50051"
export SESSION_ID="add-ethereum-session-1"
export DEVELOPER="silvana"
export AGENT="add-ethereum-agent"
export AGENT_METHOD="process"
export POLL_INTERVAL_SECS="5"
export MAX_RUNTIME_SECS="0"
```

## Docker

### Build Image

From the silvana repository root:

```bash
cd examples/add-ethereum/agent
make build  # Builds docker.io/dfstio/add-ethereum-devnet:latest
```

### Run Container

```bash
docker run --rm \
  --network host \
  -e SESSION_ID="add-ethereum-session-1" \
  -e DEVELOPER="silvana" \
  -e AGENT="add-ethereum-agent" \
  -e AGENT_METHOD="process" \
  -e RUST_LOG=info \
  docker.io/dfstio/add-ethereum-devnet:latest
```

Or with config file:

```bash
docker run --rm \
  --network host \
  -v $(pwd)/config.toml:/app/config.toml \
  -e RUST_LOG=info \
  docker.io/dfstio/add-ethereum-devnet:latest
```

## Code Structure

### coordinator-client/

gRPC client library for communicating with the coordinator service.

- `build.rs` - Generates Rust code from `coordinator.proto`
- `src/lib.rs` - gRPC client implementation
  - `CoordinatorClient` - Main client struct
  - `get_job()` - Fetch pending job from coordinator
  - `complete_job()` - Mark job as successfully completed
  - `fail_job()` - Mark job as failed with error message
  - `send_log()` - Send log messages to coordinator

### src/main.rs

Main agent implementation:

1. **Configuration Loading**: Load from config.toml or environment variables
2. **Coordinator Connection**: Connect to gRPC service
3. **Job Loop**:
   - Poll for jobs via `get_job()`
   - Process job based on `app_instance_method`
   - Call `complete_job()` on success or `fail_job()` on error
4. **Graceful Shutdown**: Handle Ctrl+C and stop cleanly

## Job Processing Flow

```rust
// 1. Get job from coordinator
let job = client.get_job().await?;

// 2. Process based on method
match job.app_instance_method {
    "init" => { /* mock processing */ }
    "add" => { /* mock processing */ }
    "settle" => { /* mock processing */ }
    _ => { client.fail_job("Unknown method").await?; }
}

// 3. Mark as completed
client.complete_job().await?;
```

## Extending the Agent

This is a mocked implementation. To add real functionality:

### 1. Parse Job Data

Jobs contain application-specific data in the `data` field:

```rust
// Deserialize job data
let transition_data: TransitionData = serde_json::from_slice(&job.data)?;
```

### 2. Implement Business Logic

Replace the 10s sleep with actual processing:

```rust
match job.app_instance_method.as_str() {
    "add" => {
        // Parse transition data
        // Verify the addition operation
        // Generate proof (if applicable)
        // Submit results back to coordinator
    }
    "settle" => {
        // Get block proof
        // Submit to settlement chain
        // Update settlement status
    }
}
```

### 3. Use Coordinator API

The coordinator client provides many useful methods:

```rust
// Get metadata
let metadata = client.get_metadata(None).await?;

// Send structured logs
client.send_log(LogLevel::Info, "Processing...").await?;

// Multiple operations available in coordinator-client
```

### 4. Add Metrics

Track job processing times, success rates, etc:

```rust
let start = Instant::now();
// ... process job ...
let duration = start.elapsed();
info!("Job processed in {:?}", duration);
```

## Troubleshooting

### Can't connect to coordinator

```
Error: Failed to connect to coordinator service
```

**Solution**: Make sure the coordinator service is running on port 50051:
- For local: `http://localhost:50051`
- For Docker: `http://host.docker.internal:50051`

### Proto compilation errors

```
Error: Failed to compile proto file
```

**Solution**: Ensure the proto file exists at the correct path:
```bash
ls ../../../../proto/silvana/coordinator/v1/coordinator.proto
```

### No jobs available

```
[INFO] No pending jobs, waiting 5s...
```

**Solution**: This is normal. Create jobs by calling the AddApp contract methods, which will create jobs that the coordinator makes available.

### Unknown job method

```
[WARN]   → Unknown job method: foo, failing job
```

**Solution**: The agent only handles `init`, `add`, and `settle` methods. Update the agent code to handle additional methods.

## Logging

Set the log level with the `RUST_LOG` environment variable:

```bash
# Info level (default)
RUST_LOG=info cargo run --bin agent

# Debug level (verbose)
RUST_LOG=debug cargo run --bin agent

# Trace level (very verbose)
RUST_LOG=trace cargo run --bin agent
```

## Comparison with TypeScript Agent

This Rust agent mirrors the TypeScript agent at `examples/add/agent`:

| Feature | TypeScript | Rust |
|---------|------------|------|
| **Communication** | gRPC via @silvana-one/agent | gRPC via coordinator-client |
| **Connection** | `createGrpcTransport()` | `CoordinatorClient::new()` |
| **Get Job** | `getJob()` | `client.get_job().await` |
| **Complete** | `completeJob()` | `client.complete_job().await` |
| **Fail** | `failJob(error)` | `client.fail_job(error).await` |
| **Logging** | `agent.info()`, `agent.error()` | `client.send_log()` + `log::info!()` |

## License

Apache 2.0
