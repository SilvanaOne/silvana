# Silvana Coordinator

The coordinator service monitors Sui blockchain events via gRPC and automatically launches Docker containers based on coordination contract events.

## Features

- Monitors Sui coordination contracts using gRPC streaming
- Automatically fetches and starts Docker images from registry events
- Supports both standard and TEE (Trusted Execution Environment) modes
- Configurable timeout handling for containers
- Resilient event streaming with automatic reconnection

## Configuration

The coordinator can be configured via environment variables or command-line arguments:

- `SUI_RPC_URL`: Sui gRPC endpoint (default: `http://148.251.75.59:9000`)
- `COORDINATION_PACKAGE_ID`: The Sui package ID to monitor (required)
- `COORDINATION_MODULE`: The module name to monitor (default: `agent`)
- `DOCKER_USE_TEE`: Enable TEE mode for Docker containers (default: `false`)
- `CONTAINER_TIMEOUT_SECS`: Container execution timeout in seconds (default: `300`)
- `LOG_LEVEL`: Logging level (default: `info`)

## Usage

```bash
# Set required environment variables
export COORDINATION_PACKAGE_ID="0xa34907868de25ec7e2bbb8e22021a3e702eb408bf87ec2bc3141a4c6b498ca01"

# Run the coordinator
cargo run --bin coordinator

# Or with custom settings
cargo run --bin coordinator -- \
  --rpc-url http://fullnode.testnet.sui.io:9000 \
  --package-id 0xYOUR_PACKAGE_ID \
  --module agent \
  --container-timeout 600
```

## Architecture

The coordinator consists of several components:

1. **Event Processor**: Main loop that streams checkpoints from Sui
2. **Event Parser**: Extracts coordination events from checkpoints
3. **Docker Manager**: Handles container lifecycle (load, run, cleanup)
4. **Error Handling**: Automatic retry logic with exponential backoff

## Event Types

The coordinator monitors for these coordination contract events:

- `MethodAddedEvent`: New Docker image registered
- `AppInstanceEvent`: User action requiring container execution
- `AgentUpdatedEvent`: Agent configuration changes

## Docker Integration

When a relevant event is detected, the coordinator:

1. Extracts Docker configuration from the event
2. Pulls the Docker image from registry (or loads from tar)
3. Verifies SHA256 digest if provided
4. Creates and runs container with specified resources
5. Monitors container execution with timeout
6. Collects logs and cleanup resources

## TEE Support

When `DOCKER_USE_TEE` is enabled:

- Uses host network mode for containers
- Performs polling-based container monitoring (for Nitro Enclave compatibility)
- Cleans up system resources after container execution

## gRPC service

grpcurl -plaintext -unix=true -import-path /Users/mike/Documents/Silvana/silvana/proto -proto silvana/coordinator/v1/coordinator.proto -d '{"developer": "test_dev", "agent": "test_agent", "agent_method": "test_method"}' unix:/tmp/coordinator.sock silvana.coordinator.v1.CoordinatorService/GetJob

AddExampleDev/AddAgent/prove

grpcurl -plaintext -unix=true -import-path /Users/mike/Documents/Silvana/silvana/proto -proto silvana/coordinator/v1/coordinator.proto -d '{"developer": "AddExampleDev", "agent": "AddAgent", "agent_method": "prove"}' unix:/tmp/coordinator.sock silvana.coordinator.v1.CoordinatorService/GetJob
