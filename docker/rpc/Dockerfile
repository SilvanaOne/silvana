FROM --platform=linux/arm64 amazonlinux:2023

# Accept chain as build argument (devnet, testnet, mainnet)
ARG CHAIN=devnet

# Install development tools, dependencies, and AWS CLI
RUN dnf update -y && \
    dnf groupinstall -y "Development Tools" && \
    dnf install -y git openssl-devel pkg-config protobuf-compiler protobuf-devel awscli

# Install Rust and Cargo with ARM64 support
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:$PATH"

# Explicitly set target for ARM64 compilation
RUN rustup target add aarch64-unknown-linux-gnu
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=gcc

# Set working directory
WORKDIR /app

# Copy workspace root files
COPY Cargo.toml ./
COPY Cargo.lock ./

# Copy proto files (needed by proto crate build script)
COPY proto/ ./proto/

# Copy all crates
COPY crates/ ./crates/

# Copy xtask crate (required by workspace)
COPY xtask/ ./xtask/

# Build the RPC server in release mode for ARM64
RUN cargo build --release --target aarch64-unknown-linux-gnu -p rpc

# Create build directory and copy contents
RUN mkdir -p /app/rpc
COPY docker/rpc/build/start.sh /app/rpc/start.sh
RUN cp target/aarch64-unknown-linux-gnu/release/rpc /app/rpc/

# Verify ARM64 binary architecture
RUN file /app/rpc/rpc && echo "✅ Binary architecture verified"
RUN ls -lh /app/rpc/

# Create tar archive
RUN cd /app && tar -czf rpc.tar.gz -C /app rpc

# Upload to S3 using chain-specific bucket
RUN --mount=type=secret,id=aws \
    export AWS_SHARED_CREDENTIALS_FILE=/run/secrets/aws && \
    echo "Uploading to s3://silvana-images-${CHAIN}/rpc.tar.gz" && \
    aws s3 cp /app/rpc.tar.gz s3://silvana-images-${CHAIN}/rpc.tar.gz