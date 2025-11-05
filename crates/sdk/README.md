# Silvana Agent SDK

A Rust SDK for building agents that interact with the Silvana OS.

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
silvana-sdk = { path = "../../../crates/sdk" }
tokio = { version = "1.40", features = ["full"] }
anyhow = "1.0"
```

### Basic Usage

```rust
use silvana_sdk::{get_job, complete_job, agent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Environment variables are read automatically:
    // SESSION_ID, DEVELOPER, AGENT, AGENT_METHOD, COORDINATOR_URL

    loop {
        // Get a job from the coordinator
        if let Some(job) = get_job().await? {
            // Log job receipt
            agent::info(&format!("Processing job: {}", job.job_id)).await?;

            // Process the job...

            // Complete the job
            complete_job().await?;
        } else {
            // No job available
            break;
        }
    }

    Ok(())
}
```

## Environment Variables

The SDK reads all environment variables once on startup and caches them.

### Required Variables

- `SESSION_ID`: Agent session identifier
- `DEVELOPER`: Developer identifier
- `AGENT`: Agent name
- `AGENT_METHOD`: Agent method name

### Optional Variables (provided by coordinator)

- `CHAIN`: Blockchain identifier (always provided by coordinator)
- `COORDINATOR_ID`: Coordinator instance ID (always provided by coordinator)
- `SESSION_PRIVATE_KEY`: Session private key (always provided by coordinator)
- `COORDINATOR_URL`: gRPC endpoint (default: `http://host.docker.internal:50051`)

### Runtime State

- `jobId`: Current job ID (managed internally, changes with each job)

All environment variables are read and cached once when the SDK is first initialized. The coordinator always provides all these values at container startup.

## API Overview

### Job Management

```rust
use silvana_sdk::{get_job, complete_job, fail_job, terminate_job};

// Get next job
let job = get_job().await?;

// Complete job
complete_job().await?;

// Fail job with error
fail_job("Error message").await?;

// Terminate job
terminate_job().await?;
```

### State Management

```rust
use silvana_sdk::{get_sequence_states, submit_state, SubmitStateParams};

// Get sequence states
let states = get_sequence_states(sequence).await?;

// Submit state
let params = SubmitStateParams {
    sequence: 100,
    new_state_data: Some(data),
    serialized_state: None,
};
submit_state(params).await?;
```

### Proof Management

```rust
use silvana_sdk::{submit_proof, get_proof, SubmitProofParams, GetProofParams};

// Submit proof
let params = SubmitProofParams {
    block_number: 1,
    sequences: vec![1, 2, 3],
    proof: proof_string,
    cpu_time: 1000,
    merged_sequences_1: None,
    merged_sequences_2: None,
};
submit_proof(params).await?;

// Get proof
let params = GetProofParams {
    block_number: 1,
    sequences: vec![1, 2, 3],
};
let proof = get_proof(params).await?;
```

### Block Management

```rust
use silvana_sdk::{get_block, try_create_block};

// Get block
let block = get_block(block_number).await?;

// Try to create block
let new_block_number = try_create_block().await?;
```

### KV Store

```rust
use silvana_sdk::{set_kv, get_kv, delete_kv};

// Set value
let tx_hash = set_kv("key", "value").await?;

// Get value
let value = get_kv("key").await?;

// Delete key
let tx_hash = delete_kv("key").await?;
```

### Metadata

```rust
use silvana_sdk::{add_metadata, get_metadata, get_app_instance};

// Add metadata (write-once)
let tx_hash = add_metadata("key", "value").await?;

// Get metadata
let metadata = get_metadata(Some("key".into())).await?;

// Get full app instance data
let app = get_app_instance().await?;
```

### Agent Logging

```rust
use silvana_sdk::agent;

// Log to both console and coordinator
agent::debug("Debug message").await?;
agent::info("Info message").await?;
agent::warn("Warning message").await?;
agent::error("Error message").await?;
agent::fatal("Fatal error").await?;
```

### Secrets Management

```rust
use silvana_sdk::get_secret;

// Retrieve secret
let api_key = get_secret("API_KEY").await?;
```

## Advanced Usage

### Instance-based API

For more control, you can create and manage the client instance directly:

```rust
use silvana_sdk::{CoordinatorClient, ClientConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ClientConfig::from_env()?;
    let mut client = CoordinatorClient::new(config).await?;

    // Use the client directly
    if let Some(job) = client.get_job().await? {
        // Process job...
        client.complete_job().await?;
    }

    Ok(())
}
```

### Builder Patterns

Complex requests use builder patterns for ergonomics:

```rust
use silvana_sdk::{CreateAppJobParams, create_app_job};

let params = CreateAppJobParams::new("method_name".into(), data)
    .with_description("Job description".into())
    .with_block_number(100)
    .with_interval_ms(5000)
    .with_settlement_chain("ethereum".into());

let job_sequence = create_app_job(params).await?;
```

## Example

See [`/examples/add-ethereum/agent`](../../../examples/add-ethereum/agent) for a complete example agent implementation.

## Documentation

Run `cargo doc --open` to view the full API documentation.

## License

Apache-2.0
