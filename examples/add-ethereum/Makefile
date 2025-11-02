# Add Ethereum Agent Makefile
# This Makefile helps with building, running, and deploying the agent

.PHONY: help run build-local build push test clean deploy add

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
	@echo "Deployment:"
	@echo "  deploy         - Deploy AddApp contract to local Anvil"
	@echo ""
	@echo "Usage:"
	@echo "  add            - Call add(1, 1) on deployed AddApp contract"
	@echo ""
	@echo "Utilities:"
	@echo "  clean          - Clean build artifacts"
	@echo ""
	@echo "Docker Commands:"
	@echo "  docker-shell   - Open interactive shell in Docker container"

# Running
run:
	@echo "🚀 Running Add Ethereum Agent..."
	cd agent && cargo run --bin agent

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
	cd agent && cargo build --release --bin agent
	@echo "✅ Binary built: agent/target/release/agent"

build:
	@echo "🐳 Building Add Ethereum Agent Docker image..."
	@echo "Image: $(DOCKER_IMAGE)"
	@echo "Platform: linux/arm64 (AWS Graviton, Apple Silicon servers)"
	@echo "⏱️  Start time: $$(date '+%Y-%m-%d %H:%M:%S')"
	@echo "📦 Copying proto files for build..."
	@rm -rf proto && cp -r ../../proto . && \
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
	cd agent && cargo test

# Utilities
clean:
	@echo "🧹 Cleaning build artifacts..."
	cd agent && cargo clean
	@echo "✅ Clean complete"

docker-shell:
	@echo "🐚 Opening shell in Docker container..."
	docker run --rm -it \
		--network host \
		-v $(PWD)/config.toml:/app/config.toml \
		--entrypoint /bin/bash \
		$(DOCKER_IMAGE)

# Deployment
deploy:
	@echo "📦 Deploying AddApp to local Anvil..."
	@echo ""
	@if ! nc -z 127.0.0.1 8545 2>/dev/null; then \
		echo "❌ ERROR: Anvil not running on port 8545"; \
		echo ""; \
		echo "Please start Anvil first:"; \
		echo "  anvil"; \
		echo ""; \
		exit 1; \
	fi
	@if [ -z "$(SILVANA_COORDINATION_CONTRACT)" ]; then \
		echo "❌ ERROR: SILVANA_COORDINATION_CONTRACT not set in .env"; \
		echo ""; \
		echo "Please add to .env:"; \
		echo "  SILVANA_COORDINATION_CONTRACT=0x7B4f352Cd40114f12e82fC675b5BA8C7582FC513"; \
		echo ""; \
		exit 1; \
	fi
	@if [ -z "$(ETHEREUM_PRIVATE_KEY)" ]; then \
		echo "❌ ERROR: ETHEREUM_PRIVATE_KEY not set in .env"; \
		echo ""; \
		echo "Please add to .env:"; \
		echo "  ETHEREUM_PRIVATE_KEY=0x..."; \
		echo ""; \
		exit 1; \
	fi
	@echo "📡 Using coordination contract: $(SILVANA_COORDINATION_CONTRACT)"
	@echo "🔑 Using deployer: $(ETHEREUM_ADDRESS)"
	@echo "📦 App Name: $(APP_NAME)"
	@echo "🤖 Agent Name: $(AGENT_NAME)"
	@echo "👤 Developer Name: $(DEVELOPER_NAME)"
	@echo ""
	@cd solidity && \
	SILVANA_COORDINATION_ADDRESS=$(SILVANA_COORDINATION_CONTRACT) \
	PRIVATE_KEY=$(ETHEREUM_PRIVATE_KEY) \
	APP_NAME=$(APP_NAME) \
	AGENT_NAME=$(AGENT_NAME) \
	DEVELOPER_NAME=$(DEVELOPER_NAME) \
	forge script script/Deploy.s.sol:DeployAddApp \
		--rpc-url localhost \
		--broadcast \
		--legacy | tee /tmp/add-ethereum-deploy.log
	@echo ""
	@echo "================================================"
	@echo "✅ Deployment Complete!"
	@echo "================================================"
	@APP_ADDR=$$(grep "AddApp deployed at:" /tmp/add-ethereum-deploy.log | awk '{print $$4}'); \
	APP_INSTANCE=$$(grep "App instance created:" /tmp/add-ethereum-deploy.log | awk '{print $$4}'); \
	echo ""; \
	echo "📝 Add these to your .env file:"; \
	echo ""; \
	echo "APP_CONTRACT=$$APP_ADDR"; \
	echo "APP_INSTANCE_ID=$$APP_INSTANCE"; \
	echo ""; \
	echo "Or run:"; \
	echo "  echo 'APP_CONTRACT=$$APP_ADDR' >> .env"; \
	echo "  echo 'APP_INSTANCE_ID=$$APP_INSTANCE' >> .env"; \
	echo ""

# Usage
add:
	@echo "➕ Calling add(1, 1) on AddApp..."
	@echo ""
	@if ! nc -z 127.0.0.1 8545 2>/dev/null; then \
		echo "❌ ERROR: Anvil not running on port 8545"; \
		echo ""; \
		exit 1; \
	fi
	@if [ -z "$(APP_CONTRACT)" ]; then \
		echo "❌ ERROR: APP_CONTRACT not set in .env"; \
		echo ""; \
		echo "Please deploy first:"; \
		echo "  make deploy"; \
		echo ""; \
		exit 1; \
	fi
	@if [ -z "$(ETHEREUM_PRIVATE_KEY)" ]; then \
		echo "❌ ERROR: ETHEREUM_PRIVATE_KEY not set in .env"; \
		echo ""; \
		exit 1; \
	fi
	@echo "📝 Contract: $(APP_CONTRACT)"
	@echo "➕ Adding value 1 at index 1..."
	@echo ""
	@cast send $(APP_CONTRACT) "add(uint32,uint256)" 1 1 \
		--rpc-url http://localhost:8545 \
		--private-key $(ETHEREUM_PRIVATE_KEY) \
		--legacy
	@echo ""
	@echo "✅ Transaction sent!"
	@echo ""
	@echo "📊 Query current state:"
	@echo "  Sum:      $$(cast call $(APP_CONTRACT) 'getSum()(uint256)' --rpc-url http://localhost:8545)"
	@echo "  Value[1]: $$(cast call $(APP_CONTRACT) 'getValue(uint32)(uint256)' 1 --rpc-url http://localhost:8545)"
	@echo "  Sequence: $$(cast call $(APP_CONTRACT) 'getSequence()(uint256)' --rpc-url http://localhost:8545)"
	@echo ""
