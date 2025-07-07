# Silvana

![Silvana Networking](docs/Silvana%20Networking.png)

## Silvana RPC

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
make build
```

#### Run pulumi script to:

- Create AWS Stack, including EC2 instance
- Install certificates, NATS, Silvana RPC
- Configure and run services

```sh
pulumi up
```

### Protobuf workflow

- Create proto definitions
- Compile with `make regen` for Rust and `buf lint && buf generate` for TypeScript - definitions will be compiled to SQL, SQL migrations, Rust interfaces with reflection and server/client, TypeScript interfaces and client, sea-orm interfaces for TiDB

## Examples of clients

- node example
- web example - https://grpc-web.silvana.dev
