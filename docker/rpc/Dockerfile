FROM --platform=linux/arm64 amazonlinux:2023

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

# Limited number of crates
COPY docker/rpc/Cargo.toml ./

# Copy dependency lock file
COPY Cargo.lock ./

# Copy proto files (needed by proto crate build script)
COPY proto/ ./proto/

# Copy all required crates
COPY crates/proto/ ./crates/proto/
COPY crates/tidb/ ./crates/tidb/
COPY crates/rpc/ ./crates/rpc/
COPY crates/monitoring/ ./crates/monitoring/
COPY crates/buffer/ ./crates/buffer/
COPY crates/nats/ ./crates/nats/
COPY crates/db/ ./crates/db/
COPY crates/kms/ ./crates/kms/

# Build the RPC server in release mode for ARM64
RUN cargo build --release --target aarch64-unknown-linux-gnu -p rpc

# Create build directory and copy contents
RUN mkdir -p /app/rpc
COPY docker/rpc/build/ /app/rpc/
RUN cp target/aarch64-unknown-linux-gnu/release/rpc /app/rpc/

# Verify ARM64 binary architecture
RUN file /app/rpc/rpc && echo "✅ Binary architecture verified"
RUN ls -lh /app/rpc/

# Create tar archive
RUN cd /app && tar -czf rpc.tar.gz -C /app rpc

# Upload to S3 
RUN --mount=type=secret,id=aws \
    export AWS_SHARED_CREDENTIALS_FILE=/run/secrets/aws && \
    echo $AWS_SHARED_CREDENTIALS_FILE && \
    aws s3 cp /app/rpc.tar.gz s3://silvana-tee-images/rpc.tar.gz