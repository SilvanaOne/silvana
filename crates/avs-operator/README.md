# AVS Operator Crate

Rust implementation of AVS (Actively Validated Services) operator functionality for EigenLayer integration.

## Features

- Register operators to EigenLayer and AVS
- Create and submit tasks to the AVS
- Monitor and respond to tasks with configurable response percentage
- WebSocket and polling-based event monitoring

## Environment Variables

Required environment variables (note: the crate supports both spellings for operator vars):

```bash
AVS_OPERATOR_PRIVATE_KEY=0x...  # or AVS_OPERATOR_PRIVATE_KEY
AVS_OPERATOR_ADDRESS=0x...      # or AVS_OPERATOR_ADDRESS
ADMIN_PRIVATE_KEY=0x...
ADMIN_ADDRESS=0x...
OPERATOR_RESPONSE_PERCENTAGE=80
RPC_URL=https://...
WS_URL=wss://...
CHAIN_ID=17000  # Optional, defaults to 17000 (Holesky)
```

## Usage

### Register Operator

```bash
cargo run --bin avs-register
```

### Create Tasks

```bash
# Create a single task
cargo run --bin avs-task single --name "MyTask"

# Create tasks continuously
cargo run --bin avs-task continuous --interval 120
```

### Monitor Tasks

```bash
# Monitor with WebSocket (default)
cargo run --bin avs-monitor

# Monitor with polling
cargo run --bin avs-monitor --mode polling
```

## Architecture

The crate provides wrapper functions around Ethereum/Alloy operations through the `ethereum` crate, avoiding direct Alloy usage. Key modules:

- `config.rs` - Environment configuration and deployment data loading
- `register.rs` - Operator registration logic
- `task.rs` - Task creation functionality
- `monitor.rs` - Task monitoring and response logic
- `error.rs` - Error handling

## Note

This is a simplified implementation that demonstrates the structure. Full contract interaction would require additional ABI encoding/decoding and transaction submission logic.
