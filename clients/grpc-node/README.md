# Silvana gRPC Node.js Test Examples

This directory contains Node.js TypeScript test examples demonstrating how to use Silvana's gRPC and gRPC-Web services with NATS integration.

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
TEST_SERVER=https://rpc.silvana.dev
```

## Running Test Examples

```bash
npm run grpc
npm run grpc:web
```
