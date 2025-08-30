FROM --platform=linux/arm64 ubuntu:22.04

# Install development tools and dependencies
RUN apt-get update && \
    apt-get install -y \
    build-essential \
    git \
    curl \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    libprotobuf-dev \
    && rm -rf /var/lib/apt/lists/*

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

# Build the coordinator in release mode for ARM64
RUN cargo build --release --target aarch64-unknown-linux-gnu -p coordinator

# Create output directory and copy binary
RUN mkdir -p /output
RUN cp target/aarch64-unknown-linux-gnu/release/silvana /output/

# List the output
RUN ls -lh /output/ && echo "✅ Binary built successfully"

# The binary will be at /output/coordinator
# This can be extracted using docker cp when the container is created