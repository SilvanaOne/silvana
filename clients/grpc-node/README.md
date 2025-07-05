# Silvana gRPC Node.js Test Examples

This directory contains Node.js TypeScript test examples demonstrating how to use Silvana's gRPC and gRPC-Web services with NATS integration.

## 📋 Test Examples Overview

1. **Example 1**: Send `AgentMessageEvent` to gRPC, read it from gRPC and NATS
2. **Example 2**: Send `AgentMessageEvent` to gRPC-Web, get it from gRPC-Web and NATS
3. **Example 3**: Search `CoordinatorMessageEvents` for "error" string using gRPC
4. **Example 4**: Search `CoordinatorMessageEvents` for "error" string using gRPC-Web

## 🚀 Quick Start

### Prerequisites

- Node.js >= 22.x
- Access to Silvana services:
  - gRPC server: `https://rpc.silvana.dev:443`
  - NATS server: `nats://rpc.silvana.dev:4222`

### Installation

```bash
cd clients/grpc-node
npm install
```

### Generate Proto Types

```bash
npm run proto:generate
```

This will generate TypeScript types from the proto files in `../../proto/`.

### Environment Configuration

The project uses environment variables from the root `.env` file (`../../.env`):

```bash
NATS_URL=nats://rpc.silvana.dev:4222
NATS_SUBJECT=silvana.events
TEST_SERVER=https://rpc.silvana.dev
```

## 🎯 Running Test Examples

### Run All Tests

```bash
npm test
```

### Run Individual Examples

```bash
npm run example1    # gRPC AgentMessageEvent with NATS
npm run example2    # gRPC-Web AgentMessageEvent with NATS
npm run example3    # gRPC CoordinatorMessageEvent Search
npm run example4    # gRPC-Web CoordinatorMessageEvent Search
```

## 📖 Test Example Details

### Example 1: gRPC AgentMessageEvent with NATS

**File**: `test/example1-grpc-agent-message.test.ts`

**What it tests**:

- Creates an `AgentMessageEvent` with sample data
- Sends it to the Silvana gRPC service using `SubmitEvent`
- Queries it back using `GetAgentMessageEventsBySequence`
- Publishes and listens for the event on NATS
- Validates event structure and responses

**Key Features**:

- Direct gRPC communication
- NATS pub/sub integration
- Event sequence tracking
- Comprehensive assertions and error handling

### Example 2: gRPC-Web AgentMessageEvent with NATS

**File**: `test/example2-grpc-web-agent-message.test.ts`

**What it tests**:

- Same as Example 1 but uses gRPC-Web protocol
- Demonstrates browser-compatible gRPC communication
- Shows graceful fallback when proxy is unavailable
- Validates NATS integration works independently

**Key Features**:

- gRPC-Web protocol usage
- Browser compatibility simulation
- Graceful degradation testing
- NATS integration validation

### Example 3: gRPC Search CoordinatorMessageEvents

**File**: `test/example3-grpc-search-coordinator.test.ts`

**What it tests**:

- Creates multiple `CoordinatorMessageEvent` samples with error messages
- Sends them via gRPC `SubmitEvent`
- Performs full-text search for "error" using `SearchCoordinatorMessageEvents`
- Validates search results and relevance scores

**Key Features**:

- Full-text search capabilities
- Multiple event submission
- Search result validation
- Database indexing awareness

### Example 4: gRPC-Web Search CoordinatorMessageEvents

**File**: `test/example4-grpc-web-search-coordinator.test.ts`

**What it tests**:

- Same as Example 3 but uses gRPC-Web protocol
- Demonstrates browser-based searching
- Shows differences between gRPC and gRPC-Web
- Validates NATS fallback behavior

**Key Features**:

- gRPC-Web search functionality
- Browser-specific error scenarios
- CORS consideration testing
- Protocol comparison validation

## 🏗️ Project Structure

```
clients/grpc-node/
├── src/
│   ├── clients/                 # Client wrappers
│   │   ├── grpc-client.ts       # Standard gRPC client
│   │   ├── grpc-web-client.ts   # gRPC-Web client
│   │   └── nats-client.ts       # NATS pub/sub client
│   ├── config/                  # Configuration
│   │   └── config.ts            # Environment configuration
│   ├── generated/               # Generated proto types (after build)
│   ├── utils/                   # Utility functions
│   │   └── event-factory.ts     # Event creation helpers
│   └── index.ts                 # Main entry point
├── test/                        # Test examples
│   ├── example1-grpc-agent-message.test.ts
│   ├── example2-grpc-web-agent-message.test.ts
│   ├── example3-grpc-search-coordinator.test.ts
│   └── example4-grpc-web-search-coordinator.test.ts
├── log.cjs                      # Logging configuration
├── package.json                 # Dependencies and scripts
├── tsconfig.json               # TypeScript configuration
└── README.md                   # This file
```

## 🔧 Dependencies

### Production Dependencies

- `@grpc/grpc-js`: gRPC client library
- `@grpc/proto-loader`: Protocol buffer loader
- `grpc-web`: gRPC-Web client library
- `nats`: NATS client library
- `google-protobuf`: Protocol buffer runtime
- `node-fetch`: HTTP client for gRPC-Web simulation

### Development Dependencies

- `typescript`: TypeScript compiler
- `ts-node`: TypeScript execution with ESM support
- `dotenv`: Environment variable loading
- `log-timestamp`: Console logging with timestamps
- `grpc-tools`: Protocol buffer compiler
- `grpc_tools_node_protoc_ts`: TypeScript proto generation
- `@types/node`: Node.js type definitions

## 🌐 gRPC vs gRPC-Web

### gRPC (Examples 1 & 3)

- **Protocol**: HTTP/2 with binary encoding
- **Use Case**: Server-to-server, native applications
- **Features**: Full duplex streaming, excellent performance
- **Requirements**: Direct HTTP/2 support

### gRPC-Web (Examples 2 & 4)

- **Protocol**: HTTP/1.1 or HTTP/2 with base64 encoding
- **Use Case**: Browser applications, web clients
- **Features**: Unary and server streaming only
- **Requirements**: Envoy proxy or similar for protocol translation

## 🔍 Protocol Buffer Messages

### AgentMessageEvent

```protobuf
message AgentMessageEvent {
  string coordinator_id = 1;
  string developer = 2;
  string agent = 3;
  string app = 4;
  string job_id = 5;
  repeated uint64 sequences = 6;
  uint64 event_timestamp = 7;
  LogLevel level = 8;
  string message = 9;
}
```

### CoordinatorMessageEvent

```protobuf
message CoordinatorMessageEvent {
  string coordinator_id = 1;
  uint64 event_timestamp = 2;
  LogLevel level = 3;
  string message = 4;
}
```

## 🚨 Troubleshooting

### gRPC Connection Issues

```
Error: 14 UNAVAILABLE: No connection established
```

**Solution**: Ensure the Silvana gRPC server is running at `rpc.silvana.dev:443`.

### gRPC-Web Proxy Issues

```
Error: gRPC-Web request failed: Service Unavailable
```

**Solution**: gRPC-Web may fail if proxy is not configured (this is expected in tests).

### NATS Connection Issues

```
Error: Could not connect to server
```

**Solution**: Ensure NATS server is accessible at `nats://rpc.silvana.dev:4222`.

### Proto Generation Issues

```
Error: protoc command not found
```

**Solution**: Install Protocol Buffers compiler and ensure it's in your PATH.

### Node.js Version Issues

```
Error: Unsupported Node.js version
```

**Solution**: Ensure you're using Node.js >= 22.x.

### Environment Variable Issues

```
Error: Cannot find .env file
```

**Solution**: Ensure the root `.env` file exists at `../../.env` with required variables.

## 📝 Development Notes

- Tests include comprehensive assertions and error handling
- gRPC-Web tests may fail if no proxy is configured (this is expected)
- NATS integration works independently of gRPC/gRPC-Web
- All tests include proper cleanup procedures
- Event timestamps are in nanoseconds (JavaScript `Date.now() * 1000000`)
- Uses Node.js native test runner with TypeScript ESM support
- Automatic timestamp logging for better debugging
- Environment variables loaded from root `.env` file

## 🧪 Test Structure

Each test follows the Node.js test format:

```typescript
import { describe, it } from "node:test";
import assert from "node:assert";

describe("Test Description", async () => {
  it("should do something", async () => {
    // Test implementation with assertions
    assert.strictEqual(actual, expected);
  });
});
```

### Test Scripts

All test scripts use the following pattern:

```bash
NODE_NO_WARNINGS=1 node -r ./log.cjs --loader=ts-node/esm --enable-source-maps -r dotenv/config --require dotenv/config --env-file=../../.env --test
```

This provides:

- Suppressed Node.js warnings
- Timestamp logging via `log.cjs`
- TypeScript ESM loader
- Source map support
- Environment variable loading from root `.env`
- Native Node.js test runner

## 🤝 Contributing

1. Follow the existing test structure and patterns
2. Add comprehensive assertions for all operations
3. Include proper error handling and graceful degradation
4. Use descriptive test names and console logging
5. Update this README for new test examples
6. Test with both successful and failure scenarios
7. Ensure compatibility with Node.js >= 22.x
8. Use ESM imports with `.js` extensions for TypeScript compatibility

## 📄 License

MIT License - see the main project LICENSE file for details.
