# syntax=docker/dockerfile:1

###############################
# Stage 1: Builder
###############################
FROM --platform=$BUILDPLATFORM rust:1.83-bookworm AS builder

# Build arguments for cross-compilation
ARG BUILDPLATFORM
ARG TARGETPLATFORM
ARG TARGETARCH

# Install development tools and protobuf compiler
RUN apt-get update && \
    apt-get install -y \
    build-essential \
    pkg-config \
    protobuf-compiler \
    libprotobuf-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy everything from the agent directory (including proto if copied by Makefile)
COPY . .

# Build the agent in release mode
RUN cargo build --release --bin agent

###############################
# Stage 2: Runtime
###############################
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary from builder
COPY --from=builder /build/target/release/agent /usr/local/bin/agent

# Copy configuration file
COPY config.toml /app/config.toml

# Set environment variables
ENV RUST_LOG=info

# Default entrypoint
ENTRYPOINT ["agent"]

# Run agent by default (agent command)
CMD ["agent"]
