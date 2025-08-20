# Silvana Coordinator

A distributed job coordination service that manages Docker-based agent execution on the Sui blockchain. The coordinator monitors blockchain events, manages job lifecycles, and provides concurrent execution of multiple agents with session-based isolation.

## Architecture Overview

The coordinator acts as a bridge between the Sui blockchain and Docker-based agents, providing:

- **Event-driven job discovery** from Sui coordination contracts
- **Concurrent agent execution** with configurable limits
- **Session-based isolation** for multi-tenant safety
- **gRPC API** for agent-coordinator communication
- **Automatic job lifecycle management** with blockchain integration

## Core Components

### 1. Event System
- **Real-time Sui Monitoring**: Streams blockchain events via gRPC
- **Job Creation Detection**: Monitors `JobCreatedEvent` from coordination contracts
- **State Synchronization**: Maintains job state consistency with blockchain
- **Automatic Reconnection**: Resilient event streaming with error recovery

### 2. Job Management
- **Job Discovery**: Finds pending jobs for different agent combinations
- **Job Lifecycle**: Tracks jobs from creation through completion/failure
- **Memory Database**: Thread-safe in-memory job tracking with unique `job_id` generation
- **Blockchain Integration**: Executes `start_job`, `complete_job`, `fail_job` transactions

### 3. Concurrent Agent Execution
- **Multi-Agent Support**: Runs multiple agents simultaneously (configurable limit)
- **Agent Isolation**: Prevents duplicate agents from running the same `(developer, agent, agent_method)`
- **Session Management**: Each Docker container gets unique session credentials
- **Resource Management**: Configurable concurrent execution limits

### 4. Docker Integration
- **Dynamic Container Launch**: Starts Docker containers based on registry configurations
- **Environment Injection**: Passes blockchain and session data to containers
- **TEE Support**: Trusted Execution Environment compatibility
- **Resource Control**: Memory, CPU, and network configuration per agent

### 5. gRPC API
- **Job Retrieval**: Agents request jobs via `GetJob` with session validation
- **Job Completion**: Agents complete jobs via `CompleteJob` with blockchain updates
- **Job Failure**: Agents fail jobs via `FailJob` with error reporting
- **Session Security**: All operations validated against session credentials

## Configuration

### Required Environment Variables
```bash
# Sui Blockchain Configuration
export SUI_RPC_URL="https://fullnode.testnet.sui.io:443"
export SUI_ADDRESS="0x..."  # Coordinator's Sui address
export SUI_CHAIN="testnet"  # Chain identifier
export SUI_PRIVATE_KEY="..."  # Coordinator's private key

# Registry Configuration
export SILVANA_REGISTRY_PACKAGE="0x..."  # Registry package ID

# Optional Configuration
export DOCKER_USE_TEE=false  # Enable TEE mode
export CONTAINER_TIMEOUT_SECS=300  # Container timeout
export LOG_LEVEL=info  # Logging level
```

### Runtime Configuration
- **MAX_CONCURRENT_AGENTS**: Maximum simultaneous Docker containers (default: 2)
- **Modifiable** via constant in `src/state.rs`

## Usage

### Starting the Coordinator
```bash
# Basic usage
cargo run --release --bin coordinator

# With custom settings
CONTAINER_TIMEOUT_SECS=600 \
SUI_RPC_URL="https://mainnet.sui.io:443" \
cargo run --release --bin coordinator
```

### Agent Development
Agents interact with the coordinator via gRPC. Example TypeScript agent:

```typescript
import { createClient } from "@connectrpc/connect";
import { CoordinatorService } from "./proto/coordinator_pb.js";

const client = createClient(CoordinatorService, transport);
const sessionId = process.env.SESSION_ID; // Set by coordinator

// Get job
const job = await client.getJob({
  developer: "MyDev",
  agent: "MyAgent", 
  agentMethod: "myMethod",
  sessionId: sessionId
});

// Process job...

// Complete job
await client.completeJob({
  jobId: job.jobId,
  sessionId: sessionId
});
```

## gRPC API Reference

### GetJob
Retrieves available jobs for an agent.

```bash
grpcurl -plaintext -unix=true \
  -import-path /path/to/proto \
  -proto silvana/coordinator/v1/coordinator.proto \
  -d '{"developer": "AddExampleDev", "agent": "AddAgent", "agent_method": "prove", "session_id": "session123"}' \
  unix:/tmp/coordinator.sock \
  silvana.coordinator.v1.CoordinatorService/GetJob
```

**Request Fields:**
- `developer`: Agent developer identifier
- `agent`: Agent name
- `agent_method`: Specific method to execute
- `session_id`: Unique session identifier (provided by coordinator)

**Response:**
- `job`: Job object with `job_id`, `job_sequence`, metadata, and execution data

### CompleteJob
Marks a job as successfully completed.

```bash
grpcurl -plaintext -unix=true \
  -import-path /path/to/proto \
  -proto silvana/coordinator/v1/coordinator.proto \
  -d '{"job_id": "snABC123...", "session_id": "session123"}' \
  unix:/tmp/coordinator.sock \
  silvana.coordinator.v1.CoordinatorService/CompleteJob
```

### FailJob
Marks a job as failed with error message.

```bash
grpcurl -plaintext -unix=true \
  -import-path /path/to/proto \
  -proto silvana/coordinator/v1/coordinator.proto \
  -d '{"job_id": "snABC123...", "error_message": "Processing failed", "session_id": "session123"}' \
  unix:/tmp/coordinator.sock \
  silvana.coordinator.v1.CoordinatorService/FailJob
```

## Job Lifecycle

### 1. Job Discovery
- Coordinator monitors Sui blockchain for `JobCreatedEvent`
- Jobs are indexed by `(developer, agent, agent_method)` combination
- Pending jobs are tracked in memory database

### 2. Agent Matching
- When jobs are available, coordinator checks for running agents
- If agent combination isn't running and under concurrent limit:
  - Fetches agent configuration from registry
  - Generates unique session credentials
  - Starts Docker container

### 3. Job Execution
- Docker container starts with environment variables:
  - `CHAIN`: Blockchain identifier
  - `COORDINATOR_ID`: Coordinator's address
  - `SESSION_ID`: Unique session identifier
  - `SESSION_PRIVATE_KEY`: Session private key
  - `DEVELOPER`, `AGENT`, `AGENT_METHOD`: Agent identifiers
- Agent requests jobs via gRPC `GetJob`
- Coordinator returns jobs with generated `job_id`

### 4. Job Completion
- Agent calls `CompleteJob` or `FailJob` with `job_id` and `session_id`
- Coordinator validates session and executes blockchain transaction
- Job is removed from memory database
- Blockchain state is updated

### 5. Cleanup
- When Docker container terminates, coordinator cleans up:
  - Removes agent from active sessions
  - Fails any incomplete jobs for that agent
  - Frees resources for new agents

## Concurrent Execution

### Session Isolation
Each Docker container receives:
- **Unique Session ID**: Ed25519 keypair-based identifier
- **Private Session Key**: For agent authentication
- **Isolated Job Scope**: Jobs can only be completed by originating session

### Concurrency Controls
- **Agent Deduplication**: Only one instance per `(developer, agent, agent_method)`
- **Capacity Limiting**: Respects `MAX_CONCURRENT_AGENTS` limit
- **Resource Queuing**: Agents wait when capacity is reached
- **Independent Lifecycles**: Each agent session is managed independently

### Example Concurrent Scenario
```
Agent A (Dev1/AgentA/method1) - Session: sess_001
Agent B (Dev2/AgentB/method2) - Session: sess_002
```
Both can run simultaneously, each processing their own jobs with complete isolation.

## Security Features

### Session Validation
- All gRPC calls require valid `session_id`
- Jobs can only be completed by the session that received them
- Cross-session job manipulation is prevented

### Blockchain Security
- All job state changes go through Sui smart contracts
- Transactions are signed by coordinator's private key
- Job state transitions are enforced by Move contracts

### Container Isolation
- Each Docker container runs with unique credentials
- Network isolation (optional TEE mode)
- Resource limits per container

## Monitoring & Logging

### Structured Logging
- All operations logged with context (session_id, job_id, agent info)
- Trace job lifecycle from discovery to completion
- Error tracking with detailed context

### Metrics
- Active agent count
- Job processing rates
- Container execution times
- Error rates and types

### Health Monitoring
- Blockchain connection status
- Docker daemon connectivity
- Memory database statistics
- gRPC service health

## Error Handling

### Blockchain Errors
- **Connection Issues**: Automatic reconnection with exponential backoff
- **Transaction Failures**: Detailed error logging and job state preservation
- **Contract Errors**: Move abort code interpretation and debugging

### Docker Errors
- **Image Pull Failures**: Retry logic with fallback
- **Container Failures**: Automatic job failure and cleanup
- **Resource Exhaustion**: Graceful degradation and queuing

### Agent Errors
- **Communication Failures**: Session timeout and cleanup
- **Processing Errors**: Job failure with error propagation to blockchain
- **Authentication Errors**: Session validation and rejection

## Development

### Building
```bash
cargo build --release --bin coordinator
```

### Testing
```bash
cargo test
```

### Proto Generation
```bash
# For Rust (automatic via build.rs)
cargo build

# For TypeScript agents
cd examples/add/agent
npm run proto:generate
```

### Adding New Agent Types
1. Deploy agent method to registry contract
2. Ensure Docker image is accessible
3. Agent implements gRPC client with session handling
4. Coordinator automatically discovers and manages new agent types

## Performance Characteristics

- **Job Discovery**: Real-time blockchain event streaming
- **Concurrent Agents**: Up to `MAX_CONCURRENT_AGENTS` simultaneous containers
- **gRPC Throughput**: High-performance Unix domain socket communication
- **Memory Usage**: Efficient in-memory job database with cleanup
- **Blockchain I/O**: Optimized transaction batching and parallel coin management

## Troubleshooting

### Common Issues

1. **No Jobs Available**
   - Check blockchain connection (`SUI_RPC_URL`)
   - Verify registry package ID (`SILVANA_REGISTRY_PACKAGE`)
   - Ensure agent method is registered

2. **Docker Container Fails**
   - Check Docker daemon connectivity
   - Verify agent image accessibility
   - Review container logs and resource limits

3. **gRPC Errors**
   - Validate session_id in agent requests
   - Check Unix socket permissions (`/tmp/coordinator.sock`)
   - Verify proto file compatibility

4. **Job Completion Failures**
   - Ensure sufficient SUI balance for transactions
   - Check Move contract state and job status
   - Verify session validation logic

### Debug Commands
```bash
# Check coordinator health
curl -X GET http://localhost:8080/health  # If HTTP endpoint enabled

# Monitor blockchain events
tail -f coordinator.log | grep "JobCreatedEvent"

# Test gRPC connectivity
grpcurl -plaintext -unix=true unix:/tmp/coordinator.sock describe
```

This coordinator provides a robust, scalable foundation for distributed agent execution on the Sui blockchain with strong isolation guarantees and comprehensive lifecycle management.