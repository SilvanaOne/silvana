FROM --platform=$BUILDPLATFORM ubuntu:22.04 AS builder

# Build arguments for cross-compilation
ARG BUILDPLATFORM
ARG TARGETPLATFORM
ARG TARGETARCH

# Install development tools and dependencies
RUN apt-get update && \
    apt-get install -y \
    build-essential \
    git \
    curl \
    pkg-config \
    perl \
    make \
    cmake \
    && rm -rf /var/lib/apt/lists/*

# Install additional tools for cross-compilation if needed
RUN if [ "$TARGETARCH" = "amd64" ] && [ "$BUILDPLATFORM" != "linux/amd64" ]; then \
        apt-get update && \
        apt-get install -y \
        gcc-x86-64-linux-gnu \
        g++-x86-64-linux-gnu && \
        rm -rf /var/lib/apt/lists/*; \
    fi

# Install Rust and Cargo
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:$PATH"

# Add the appropriate Rust target based on TARGETARCH
RUN if [ "$TARGETARCH" = "amd64" ]; then \
        rustup target add x86_64-unknown-linux-gnu; \
    fi

# Set working directory
WORKDIR /app

# Copy workspace root files
COPY Cargo.toml ./
COPY Cargo.lock ./

# Copy all crates
COPY crates/ ./crates/

# Copy xtask crate (required by workspace)
COPY xtask/ ./xtask/

# Build the health binary in release mode for the target architecture
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true
RUN if [ "$TARGETARCH" = "amd64" ]; then \
        export CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc && \
        export CXX_x86_64_unknown_linux_gnu=x86_64-linux-gnu-g++ && \
        export AR_x86_64_unknown_linux_gnu=x86_64-linux-gnu-ar && \
        export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc && \
        cargo build --release --target x86_64-unknown-linux-gnu -p health && \
        cp target/x86_64-unknown-linux-gnu/release/health /tmp/health; \
    else \
        cargo build --release -p health && \
        cp target/release/health /tmp/health; \
    fi

# Create output directory and copy binary
RUN mkdir -p /output && cp /tmp/health /output/

# List the output
RUN ls -lh /output/ && echo "Health binary built successfully for $TARGETARCH"

# The binary will be at /output/health
# This can be extracted using docker cp when the container is created
