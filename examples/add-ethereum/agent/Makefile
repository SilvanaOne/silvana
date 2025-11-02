# Add Ethereum Agent Makefile
# This Makefile helps with building, running, and deploying the agent

.PHONY: help run build-local build push test clean

# Load environment variables from .env file
-include .env
export

# Docker image configuration (defaults to local if not set in .env)
DOCKER_IMAGE ?= add-ethereum-agent:latest

# Default target
help:
	@echo "Add Ethereum Agent Commands:"
	@echo ""
	@echo "Running:"
	@echo "  run            - Run agent locally (requires Rust)"
	@echo "  run-docker     - Run agent in Docker container"
	@echo ""
	@echo "Building:"
	@echo "  build-local    - Build Rust binary locally (no Docker)"
	@echo "  build          - Build Docker image for ARM64"
	@echo "  push           - Push Docker image to registry (run after build)"
	@echo ""
	@echo "Testing:"
	@echo "  test           - Run Rust tests"
	@echo ""
	@echo "Utilities:"
	@echo "  clean          - Clean build artifacts"
	@echo ""
	@echo "Docker Commands:"
	@echo "  docker-shell   - Open interactive shell in Docker container"

# Running
run:
	@echo "🚀 Running Add Ethereum Agent..."
	cargo run --bin agent

run-docker:
	@echo "🐳 Running agent in Docker container..."
	@echo "Image: $(DOCKER_IMAGE)"
	docker run --rm \
		--network host \
		-v $(PWD)/config.toml:/app/config.toml \
		-e RUST_LOG=info \
		$(DOCKER_IMAGE)

# Building
build-local:
	@echo "🔨 Building Rust binary locally..."
	cargo build --release --bin agent
	@echo "✅ Binary built: target/release/agent"

build:
	@echo "🐳 Building Add Ethereum Agent Docker image..."
	@echo "Image: $(DOCKER_IMAGE)"
	@echo "Platform: linux/arm64 (AWS Graviton, Apple Silicon servers)"
	@echo "⏱️  Start time: $$(date '+%Y-%m-%d %H:%M:%S')"
	@echo "📦 Copying proto files for build..."
	@rm -rf proto && cp -r ../../../proto . && \
	trap "rm -rf proto" EXIT; \
	start_time=$$(date +%s); \
	DOCKER_BUILDKIT=1 docker build \
		--platform linux/arm64 \
		-f Dockerfile \
		-t $(DOCKER_IMAGE) \
		--progress=plain \
		. && \
	end_time=$$(date +%s); \
	duration=$$((end_time - start_time)); \
	echo "⏱️  End time: $$(date '+%Y-%m-%d %H:%M:%S')"; \
	echo "⏱️  Duration: $${duration} seconds"; \
	echo "✅ Docker image built: $(DOCKER_IMAGE)"

push:
	@echo "🚀 Pushing Docker image to registry..."
	@echo "Image: $(DOCKER_IMAGE)"
	@if [ "$(DOCKER_IMAGE)" = "add-ethereum-agent:latest" ]; then \
		echo "⚠️  WARNING: Using default local image name"; \
		echo "Set DOCKER_IMAGE in .env to push to a registry"; \
		exit 1; \
	fi
	@if [ -n "$(DOCKER_PASSWORD)" ]; then \
		echo "Logging in to Docker registry..."; \
		echo "$(DOCKER_PASSWORD)" | docker login --username $(DOCKER_USERNAME) --password-stdin; \
	fi
	docker push $(DOCKER_IMAGE)
	@echo "✅ Image pushed: $(DOCKER_IMAGE)"

# Testing
test:
	@echo "🧪 Running Rust tests..."
	cargo test

# Utilities
clean:
	@echo "🧹 Cleaning build artifacts..."
	cargo clean
	@echo "✅ Clean complete"

docker-shell:
	@echo "🐚 Opening shell in Docker container..."
	docker run --rm -it \
		--network host \
		-v $(PWD)/config.toml:/app/config.toml \
		--entrypoint /bin/bash \
		$(DOCKER_IMAGE)
