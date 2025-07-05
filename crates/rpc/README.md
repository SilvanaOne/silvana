# Silvana RPC

A gRPC service that accepts event streams and efficiently batches them to TiDB serverless and NATS JetStream.

## Features

- **gRPC Event Streaming**: High-performance event ingestion with protobuf
- **Multi-Coordinator Support**: All events include `coordinator_id` for tracking events per coordinator
- **Event Buffering**: Configurable in-memory buffering with automatic batch processing
- **TiDB Integration**: Efficient batch insertion to TiDB serverless with transaction support
- **NATS JetStream**: Event streaming to NATS for real-time processing (placeholder implementation)
- **Type Safety**: Sea-ORM entities for all event types with compile-time validation
- **Monitoring**: Built-in statistics and performance tracking
- **Production Ready**: Configurable batch sizes, timeouts, and connection pooling
- **Memory Safety**: Bounded channels, circuit breakers, and backpressure to prevent OOM

## Monitoring

The service includes comprehensive monitoring with Prometheus metrics:

### Available Metrics

**Buffer Metrics:**

- `silvana_buffer_events_total` - Total events received
- `silvana_buffer_events_processed_total` - Total events processed
- `silvana_buffer_events_dropped_total` - Total events dropped
- `silvana_buffer_events_error_total` - Total processing errors
- `silvana_buffer_size_current` - Current buffer size
- `silvana_buffer_memory_bytes` - Current memory usage
- `silvana_buffer_backpressure_events_total` - Total backpressure events
- `silvana_buffer_health_status` - Buffer health (1=healthy, 0=unhealthy)
- `silvana_circuit_breaker_status` - Circuit breaker status (1=open, 0=closed)

**gRPC Metrics:**

- `silvana_grpc_requests_total` - Total gRPC requests
- `silvana_grpc_request_duration_seconds` - Request duration histogram

### Configuration

Set these environment variables:

```bash
# Metrics server address (default: 0.0.0.0:9090)
METRICS_ADDR=0.0.0.0:9090
```

### Accessing Metrics

The metrics endpoint is available at:

```
http://localhost:9090/metrics
```

### Grafana Integration

The metrics follow Prometheus best practices and can be easily visualized in Grafana:

1. Configure Prometheus to scrape `http://your-server:9090/metrics`
2. Import a gRPC dashboard or create custom dashboards
3. Monitor buffer performance, error rates, and system health

### Manual Metrics Collection

For custom gRPC metrics, use the `record_grpc_request` function:

```rust
use crate::monitoring::record_grpc_request;
use std::time::Instant;

async fn my_grpc_method(&self, request: Request<T>) -> Result<Response<R>, Status> {
    let start_time = Instant::now();

    // Your business logic here...

    let duration = start_time.elapsed();
    let status_code = if success { "200" } else { "500" };
    record_grpc_request("method_name", status_code, duration.as_secs_f64());

    // Return response...
}
```

## Environment Variables

```bash
# Database
DATABASE_URL=mysql://user:pass@host:port/database

# Server Configuration
SERVER_ADDR=0.0.0.0:50051
METRICS_ADDR=0.0.0.0:9090

# Buffer Configuration
BATCH_SIZE=100
FLUSH_INTERVAL_MS=500
CHANNEL_CAPACITY=500000
```

## Running

```bash
cargo run
```

The service will start:

- gRPC server on `SERVER_ADDR` (default: 0.0.0.0:50051)
- Metrics server on `METRICS_ADDR` (default: 0.0.0.0:9090)

## Quick Start

1. **Install Dependencies**

   ```bash
   # Install Rust and Cargo
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **Clone and Build**

   ```bash
   git clone <repository>
   cd rpc
   cargo build --release
   ```

3. **Test**

```sh
cargo test --release -p rpc --test integration_test -- --nocapture
cargo test --release -p rpc --test sequence_test -- --nocapture
cargo test --release -p rpc --test fulltext_search_test -- --nocapture
cargo test --release -p rpc --test nats_test -- --nocapture
```
