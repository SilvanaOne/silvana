# Silvana
# =======================================================
# This Makefile implements the workflow where proto files are the single source of truth

# Configuration
PROTO_FILE := proto/silvana/rpc/v1/rpc.proto
SQL_DIR := proto/sql
MIGR_DIR := infra/tidb/migration/sql
ENTITY_DIR := crates/tidb/src/entity

# Function to load DATABASE_URL from .env file
define load_database_url
	$(shell if [ -f .env ] && grep -q "^DATABASE_URL=" .env; then \
		grep "^DATABASE_URL=" .env | cut -d'=' -f2- | sed 's/^["'"'"']//;s/["'"'"']$$//'; \
	fi)
endef

# Function to load STATE_DATABASE_URL from .env file
define load_state_database_url
	$(shell if [ -f .env ] && grep -q "^STATE_DATABASE_URL=" .env; then \
		grep "^STATE_DATABASE_URL=" .env | cut -d'=' -f2- | sed 's/^["'"'"']//;s/["'"'"']$$//'; \
	fi)
endef

# Function to load ETHEREUM_COORDINATION_URL from .env file
define load_ethereum_url
	$(shell if [ -f .env ] && grep -q "^ETHEREUM_COORDINATION_URL=" .env; then \
		grep "^ETHEREUM_COORDINATION_URL=" .env | cut -d'=' -f2- | sed 's/^["'"'"']//;s/["'"'"']$$//'; \
	fi)
endef

# Database configuration check
check-database-url:
	@if [ ! -f .env ]; then \
		echo "❌ ERROR: .env file not found"; \
		echo ""; \
		echo "Please create a .env file with DATABASE_URL:"; \
		echo "  echo 'DATABASE_URL=mysql://user:pass@tcp(host:port)/database' > .env"; \
		echo ""; \
		echo "Examples:"; \
		echo "  echo 'DATABASE_URL=mysql://root:@tcp(localhost:4000)/silvana_rpc' > .env"; \
		echo "  echo 'DATABASE_URL=mysql://user:pass@tcp(myhost.com:3306)/mydb' > .env"; \
		echo ""; \
		exit 1; \
	fi
	@if ! grep -q "^DATABASE_URL=" .env; then \
		echo "❌ ERROR: DATABASE_URL not found in .env file"; \
		echo ""; \
		echo "Please add DATABASE_URL to your .env file:"; \
		echo "  echo 'DATABASE_URL=mysql://user:pass@tcp(host:port)/database' >> .env"; \
		echo ""; \
		echo "Examples:"; \
		echo "  echo 'DATABASE_URL=mysql://root:@tcp(localhost:4000)/silvana_rpc' >> .env"; \
		echo "  echo 'DATABASE_URL=mysql://user:pass@tcp(myhost.com:3306)/mydb' >> .env"; \
		echo ""; \
		exit 1; \
	fi
	@DB_URL="$(call load_database_url)"; \
	if [ -z "$$DB_URL" ]; then \
		echo "❌ ERROR: DATABASE_URL value is empty in .env file"; \
		echo ""; \
		echo "Please set a valid DATABASE_URL value in .env:"; \
		echo "  DATABASE_URL=mysql://user:pass@tcp(host:port)/database"; \
		echo ""; \
		exit 1; \
	fi

# State database configuration check
check-state-database-url:
	@if [ ! -f .env ]; then \
		echo "❌ ERROR: .env file not found"; \
		echo ""; \
		echo "Please create a .env file with STATE_DATABASE_URL:"; \
		echo "  echo 'STATE_DATABASE_URL=mysql://user:pass@host:port/database' >> .env"; \
		echo ""; \
		exit 1; \
	fi
	@if ! grep -q "^STATE_DATABASE_URL=" .env; then \
		echo "❌ ERROR: STATE_DATABASE_URL not found in .env file"; \
		echo ""; \
		echo "Please add STATE_DATABASE_URL to your .env file:"; \
		echo "  echo 'STATE_DATABASE_URL=mysql://469eCuUizFHZ892.root:pass@host:port/test' >> .env"; \
		echo ""; \
		exit 1; \
	fi
	@STATE_DB_URL="$(call load_state_database_url)"; \
	if [ -z "$$STATE_DB_URL" ]; then \
		echo "❌ ERROR: STATE_DATABASE_URL value is empty in .env file"; \
		echo ""; \
		echo "Please set a valid STATE_DATABASE_URL value in .env"; \
		exit 1; \
	fi

# Ethereum deployment configuration check
check-ethereum-url:
	@if [ ! -f .env ]; then \
		echo "❌ ERROR: .env file not found"; \
		echo ""; \
		echo "Please create a .env file with ETHEREUM_COORDINATION_URL:"; \
		echo "  echo 'ETHEREUM_COORDINATION_URL=https://rpc-amoy.polygon.technology' > .env"; \
		echo ""; \
		exit 1; \
	fi
	@if ! grep -q "^ETHEREUM_COORDINATION_URL=" .env; then \
		echo "❌ ERROR: ETHEREUM_COORDINATION_URL not found in .env file"; \
		echo ""; \
		echo "Please add ETHEREUM_COORDINATION_URL to your .env file:"; \
		echo "  echo 'ETHEREUM_COORDINATION_URL=https://rpc-amoy.polygon.technology' >> .env"; \
		echo ""; \
		exit 1; \
	fi
	@ETH_URL="$(call load_ethereum_url)"; \
	if [ -z "$$ETH_URL" ]; then \
		echo "❌ ERROR: ETHEREUM_COORDINATION_URL value is empty in .env file"; \
		echo ""; \
		echo "Please set a valid ETHEREUM_COORDINATION_URL value in .env"; \
		exit 1; \
	fi

check-ethereum-private-key:
	@if ! grep -q "^ETHEREUM_PRIVATE_KEY=" .env; then \
		echo "❌ ERROR: ETHEREUM_PRIVATE_KEY not found in .env file"; \
		echo ""; \
		echo "Deployment requires a private key for signing transactions."; \
		echo "Add to .env:"; \
		echo "  echo 'ETHEREUM_PRIVATE_KEY=0x...' >> .env"; \
		echo ""; \
		exit 1; \
	fi

# mysqldef supports DATABASE_URL directly, no parsing needed

.PHONY: help install-tools regen proto2sql entities clean-dev clean-dev-state setup check-tools check-database-url check-state-database-url check-ethereum-url check-ethereum-private-key validate-schema check-schema show-tables show-state-tables show-schema show-state-schema apply-ddl apply-ddl-state proto2entities dev-reset build store-secret retrieve-secret write-config read-config analyze run-state deploy-ethereum deploy-ethereum-dry deploy-ethereum-local test-ethereum-coordination

# Default target when no arguments are provided
.DEFAULT_GOAL := help

# Default target
help: ## Show this help message
	@echo "Silvana"
	@echo "======="
	@echo ""
	@echo "📋 GENERAL COMMANDS:"
	@grep -E '^(help|install-tools|check-tools|setup):.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'
	@echo ""
	@echo "🏗️  BUILD & DEPLOYMENT:"
	@grep -E '^(build-rpc|build-arm|build-x86|build-mac|build-all|release-archives|github-release|deploy-ethereum|deploy-ethereum-dry|deploy-ethereum-local|test-ethereum-coordination):.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-28s\033[0m %s\n", $$1, $$2}'
	@echo ""
	@echo "🗃️  DATABASE MANAGEMENT:"
	@grep -E '^(regen|proto2sql|proto2entities|apply-ddl|apply-ddl-state|entities):.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'
	@echo ""
	@echo "🔧 DEVELOPMENT & DEBUGGING:"
	@grep -E '^(clean-dev|dev-reset|show-tables|show-state-tables|show-schema|show-state-schema|validate-schema|check-schema|run-state):.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'
	@echo ""
	@echo "🔐 SECRET MANAGEMENT & CONFIG:"
	@grep -E '^(store-secret|retrieve-secret|write-config|read-config):.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'
	@echo ""
	@echo "📊 ANALYSIS:"
	@grep -E '^(analyze):.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'
	@echo ""
	@echo "📚 EXAMPLES:"
	@grep -E '^(example-archive):.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'
	@echo ""
	@echo "💡 USAGE EXAMPLES:"
	@echo ""
	@echo "  Secret Management:"
	@echo "    make store-secret DEVELOPER=alice AGENT=my-agent NAME=api-key SECRET=sk-123"
	@echo "    make store-secret DEVELOPER=bob AGENT=bot APP=trading NAME=db-pass SECRET=secret123"
	@echo "    make retrieve-secret DEVELOPER=alice AGENT=my-agent NAME=api-key"
	@echo ""
	@echo "  Configuration Management:"
	@echo "    make write-config CHAIN=devnet"
	@echo "    make read-config CHAIN=testnet ENDPOINT=http://localhost:8080"
	@echo "    make build-rpc CHAIN=testnet      # Build for testnet"
	@echo ""
	@echo "  Build and Release:"
	@echo "    make build-all                    # Build for all platforms"
	@echo "    make github-release VERSION=v0.1.5 # Create GitHub release"
	@echo ""
	@echo "⚙️  CONFIGURATION:"
	@echo "  .env              Contains DATABASE_URL (REQUIRED)"
	@echo "                    Format: DATABASE_URL=mysql://user:pass@tcp(host:port)/database"
	@echo "  docker/rpc/.env   Contains AWS credentials for build (OPTIONAL for S3 upload)"
	@echo "                    Format: AWS_ACCESS_KEY_ID=key, AWS_SECRET_ACCESS_KEY=secret"
	@echo ""
	@echo "🏗️  BUILD INFORMATION:"
	@echo "  The 'build' target creates ARM64 binaries for AWS Graviton processors"
	@echo "  Requires Docker with BuildKit and cross-compilation support"
	@echo ""
	@echo ""
	@if [ -f .env ] && grep -q "^DATABASE_URL=" .env; then \
		echo "Current DATABASE_URL: $$(grep "^DATABASE_URL=" .env | cut -d'=' -f2- | sed 's/^["'"'"']//;s/["'"'"']$$//')"; \
	elif [ -f .env ]; then \
		echo "❌ .env file exists but DATABASE_URL is not set"; \
	else \
		echo "❌ .env file not found"; \
	fi

install-tools: ## Install required tools (mysqldef only - proto-to-ddl builds automatically)
	@echo "🔧 Installing required tools..."
	@echo "Installing mysqldef..."
	go install github.com/sqldef/sqldef/cmd/mysqldef@latest
	brew install mysql
	@echo "✅ All tools installed"
	@echo "ℹ️  proto-to-ddl will be built automatically via 'cargo run --release'"

check-tools: ## Check if required tools are installed
	@echo "🔍 Checking required tools..."
	@test -d infra/tidb/proto-to-ddl || (echo "❌ proto-to-ddl directory not found" && exit 1)
	@command -v mysqldef >/dev/null 2>&1 || (echo "❌ mysqldef not found. Run 'make install-tools'" && exit 1)
	@echo "✅ All required tools are available"

setup: ## Create necessary directories
	@echo "📁 Setting up directories..."
	@mkdir -p $(SQL_DIR)
	@mkdir -p $(MIGR_DIR)
	@mkdir -p $(ENTITY_DIR)
	@echo "✅ Directories created"

# Build RPC with CHAIN parameter (defaults to devnet)
CHAIN ?= devnet

build-rpc: ## Build ARM64 RPC for Graviton and create tar archive, upload to S3 (use CHAIN=testnet/mainnet)
	@echo "🐳 Building RPC for chain: $(CHAIN)..."
	@echo "📦 Creating deployment archive for Amazon Linux 2023 ARM64..."
	@mkdir -p build
	@# Validate chain parameter
	@if [ "$(CHAIN)" != "devnet" ] && [ "$(CHAIN)" != "testnet" ] && [ "$(CHAIN)" != "mainnet" ]; then \
		echo "❌ ERROR: CHAIN must be devnet, testnet, or mainnet. Got: $(CHAIN)"; \
		exit 1; \
	fi
	@# Check for AWS credentials file for the specific chain
	@if [ ! -f "docker/rpc/.env.$(CHAIN)" ]; then \
		echo "❌ ERROR: AWS credentials file docker/rpc/.env.$(CHAIN) not found"; \
		echo "Please create the file with AWS credentials for $(CHAIN) environment"; \
		exit 1; \
	fi
	@echo "🔨 Building Docker image for ARM64 (Graviton), compiling RPC..."
	@DOCKER_BUILDKIT=1 docker build \
		--platform linux/arm64 \
		--build-arg CHAIN=$(CHAIN) \
		--secret id=aws,src=docker/rpc/.env.$(CHAIN) \
		-f docker/rpc/Dockerfile \
		-t rpc-builder:al2023-arm64-$(CHAIN) \
		--progress=plain \
		.
	@echo "🧹 Cleaning up Docker image..."
	@docker rmi rpc-builder:al2023-arm64-$(CHAIN) 2>/dev/null || true
	@echo "✅ RPC deployment archive built successfully for ARM64 ($(CHAIN))"
	@echo "📦 Archive uploaded to: s3://silvana-images-$(CHAIN)/rpc.tar.gz"
	@echo "🚀 To deploy, run: cd infra/pulumi-rpc && pulumi up -s $(CHAIN)"

build-arm: ## Build coordinator for Ubuntu Linux ARM64 (aarch64) using Docker
	@echo "🐳 Building Silvana for Ubuntu Linux ARM64..."
	@mkdir -p docker/coordinator/release/arm
	@echo "🔨 Building Docker image for ARM64, compiling coordinator..."
	@DOCKER_BUILDKIT=1 docker build \
		--platform linux/arm64 \
		-f docker/coordinator/Dockerfile \
		-t coordinator-builder:arm64 \
		--progress=plain \
		.
	@echo "📦 Extracting binary from Docker image..."
	@docker create --name coordinator-extract coordinator-builder:arm64
	@docker cp coordinator-extract:/output/silvana docker/coordinator/release/arm/silvana
	@docker rm coordinator-extract
	@echo "🧹 Cleaning up Docker image..."
	@docker rmi coordinator-builder:arm64 2>/dev/null || true
	@echo "✅ Silvana built successfully for ARM64"
	@echo "📦 Binary location: docker/coordinator/release/arm/silvana"

build-x86: ## Build coordinator for Ubuntu Linux x86_64 (amd64) using Docker
	@echo "🐳 Building Silvana for Ubuntu Linux x86_64..."
	@mkdir -p docker/coordinator/release/x86
	@echo "🔨 Building Docker image for x86_64, compiling coordinator..."
	@DOCKER_BUILDKIT=1 docker build \
		--platform linux/amd64 \
		-f docker/coordinator/Dockerfile \
		-t coordinator-builder:amd64 \
		--progress=plain \
		.
	@echo "📦 Extracting binary from Docker image..."
	@docker create --name coordinator-extract coordinator-builder:amd64
	@docker cp coordinator-extract:/output/silvana docker/coordinator/release/x86/silvana
	@docker rm coordinator-extract
	@echo "🧹 Cleaning up Docker image..."
	@docker rmi coordinator-builder:amd64 2>/dev/null || true
	@echo "✅ Silvana built successfully for x86_64"
	@echo "📦 Binary location: docker/coordinator/release/x86/silvana"

build-mac: ## Build coordinator for macOS Apple Silicon (M1/M2/M3) natively
	@echo "🍎 Building Silvana for macOS Apple Silicon..."
	@mkdir -p docker/coordinator/release/mac
	@echo "🔨 Building with cargo for Apple Silicon..."
	@cargo build --release --bin silvana
	@cp target/release/silvana docker/coordinator/release/mac/silvana
	@echo "✅ Silvana built successfully for macOS Apple Silicon"
	@echo "📦 Binary location: docker/coordinator/release/mac/silvana"

deploy-ethereum: check-ethereum-url check-ethereum-private-key ## Deploy Ethereum coordination contracts to configured network
	@echo "🚀 Deploying Ethereum Coordination Layer..."
	@echo ""
	@ETH_URL="$(call load_ethereum_url)"; \
	echo "📡 Network: $$ETH_URL"; \
	echo "📝 Creating deployment log..."; \
	mkdir -p build; \
	LOG_FILE="build/ethereum-deploy-$$(date +%Y%m%d-%H%M%S).log"; \
	VERIFY_FLAG=""; \
	ETHERSCAN_KEY="$$(grep "^ETHERSCAN_API_KEY=" .env 2>/dev/null | cut -d'=' -f2- || echo '')"; \
	if [ -n "$$ETHERSCAN_KEY" ] && [ "$$ETHERSCAN_KEY" != "none" ]; then \
		echo "🔍 ETHERSCAN_API_KEY found - contract verification enabled"; \
		VERIFY_FLAG="--verify"; \
	else \
		echo "⚠️  No ETHERSCAN_API_KEY in .env - contract verification disabled"; \
	fi; \
	echo ""; \
	export ETHERSCAN_API_KEY="$$ETHERSCAN_KEY"; \
	export POLYGONSCAN_API_KEY="$$ETHERSCAN_KEY"; \
	export BASESCAN_API_KEY="$$ETHERSCAN_KEY"; \
	cd solidity/coordination && \
	if forge script script/Deploy.s.sol:DeployCoordination \
		--rpc-url "$$ETH_URL" \
		--private-key "$$(grep "^ETHEREUM_PRIVATE_KEY=" ../../.env | cut -d'=' -f2- | sed 's/^["'"'"']//;s/["'"'"']$$//')" \
		--broadcast \
		$$VERIFY_FLAG \
		-vvv \
		2>&1 | tee "../../$$LOG_FILE"; then \
		if grep -q "Error:" "../../$$LOG_FILE" || grep -q "error code" "../../$$LOG_FILE"; then \
			echo ""; \
			echo "❌ Deployment failed! Check log for errors."; \
			echo "📄 Full log: $$LOG_FILE"; \
			exit 1; \
		else \
			echo ""; \
			echo "✅ Deployment complete!"; \
			echo "📄 Full log saved to: $$LOG_FILE"; \
			echo ""; \
			echo "📋 Deployment Summary:"; \
			grep "deployed at:" "../../$$LOG_FILE" || echo "⚠️  Could not extract contract addresses from log"; \
		fi; \
	else \
		echo ""; \
		echo "❌ Deployment command failed!"; \
		echo "📄 Full log: $$LOG_FILE"; \
		exit 1; \
	fi

deploy-ethereum-dry: check-ethereum-url ## Simulate Ethereum contract deployment (no broadcast)
	@echo "🔍 Simulating Ethereum Coordination Layer deployment..."
	@echo ""
	@ETH_URL="$(call load_ethereum_url)"; \
	echo "📡 Network: $$ETH_URL"; \
	echo "⚠️  DRY RUN - No transactions will be broadcast"; \
	echo ""; \
	VERIFY_FLAG=""; \
	if echo "$$ETH_URL" | grep -qE "(localhost|127\.0\.0\.1)"; then \
		echo "🏠 Local network detected - verification disabled"; \
	else \
		ETHERSCAN_KEY="$$(grep "^ETHERSCAN_API_KEY=" .env 2>/dev/null | cut -d'=' -f2- || echo '')"; \
		if [ -n "$$ETHERSCAN_KEY" ] && [ "$$ETHERSCAN_KEY" != "none" ]; then \
			echo "🔍 Etherscan verification will be enabled on broadcast"; \
			VERIFY_FLAG="--verify"; \
			export ETHERSCAN_API_KEY="$$ETHERSCAN_KEY"; \
			export POLYGONSCAN_API_KEY="$$ETHERSCAN_KEY"; \
			export BASESCAN_API_KEY="$$ETHERSCAN_KEY"; \
		else \
			echo "ℹ️  No ETHERSCAN_API_KEY - verification disabled"; \
		fi; \
	fi; \
	cd solidity/coordination && \
	forge script script/Deploy.s.sol:DeployCoordination \
		--rpc-url "$$ETH_URL" \
		$$VERIFY_FLAG \
		-vvv
	@echo ""
	@echo "✅ Simulation complete!"

deploy-ethereum-local: ## Deploy to local Anvil testnet (no verification)
	@echo "🏠 Deploying Ethereum Coordination Layer to local Anvil..."
	@echo ""
	@if ! nc -z 127.0.0.1 8545 2>/dev/null; then \
		echo "❌ ERROR: Anvil not running on port 8545"; \
		echo ""; \
		echo "Please start Anvil first:"; \
		echo "  anvil"; \
		echo ""; \
		exit 1; \
	fi
	@PRIVATE_KEY="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"; \
	ADMIN="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"; \
	COORD1="0x70997970C51812dc3A010C7d01b50e0d17dc79C8"; \
	COORD2="0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"; \
	OPERATOR="0x90F79bf6EB2c4f870365E785982E1f101E93b906"; \
	echo "📡 Network: http://127.0.0.1:8545"; \
	echo "🔑 Using private key: $${PRIVATE_KEY:0:10}..."; \
	echo ""; \
	echo "📦 Deploying contracts..."; \
	mkdir -p build; \
	DEPLOY_FILE="build/ethereum-local-$$(date +%Y%m%d-%H%M%S).json"; \
	cd solidity/coordination && \
	echo "  1/6 JobManager..."; \
	JOB_OUTPUT=$$(forge create src/JobManager.sol:JobManager --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy --broadcast 2>&1); \
	JOB_ADDR=$$(echo "$$JOB_OUTPUT" | grep "Deployed to:" | awk '{print $$3}'); \
	echo "  2/6 AppInstanceManager..."; \
	APP_OUTPUT=$$(forge create src/AppInstanceManager.sol:AppInstanceManager --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy --broadcast 2>&1); \
	APP_ADDR=$$(echo "$$APP_OUTPUT" | grep "Deployed to:" | awk '{print $$3}'); \
	echo "  3/6 ProofManager..."; \
	PROOF_OUTPUT=$$(forge create src/ProofManager.sol:ProofManager --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy --broadcast 2>&1); \
	PROOF_ADDR=$$(echo "$$PROOF_OUTPUT" | grep "Deployed to:" | awk '{print $$3}'); \
	echo "  4/6 SettlementManager..."; \
	SETTLE_OUTPUT=$$(forge create src/SettlementManager.sol:SettlementManager --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy --broadcast 2>&1); \
	SETTLE_ADDR=$$(echo "$$SETTLE_OUTPUT" | grep "Deployed to:" | awk '{print $$3}'); \
	echo "  5/6 StorageManager..."; \
	STORAGE_OUTPUT=$$(forge create src/StorageManager.sol:StorageManager --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy --broadcast 2>&1); \
	STORAGE_ADDR=$$(echo "$$STORAGE_OUTPUT" | grep "Deployed to:" | awk '{print $$3}'); \
	echo "  6/6 SilvanaCoordination..."; \
	COORD_OUTPUT=$$(forge create src/SilvanaCoordination.sol:SilvanaCoordination --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy --broadcast 2>&1); \
	COORD_ADDR=$$(echo "$$COORD_OUTPUT" | grep "Deployed to:" | awk '{print $$3}'); \
	echo ""; \
	echo "💰 Funding local accounts..."; \
	cast send 0xD99b88cd8c784D9efd42646b4C1E27358D2179E6 --value 100ether --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send 0xbc356b91e24e0f3809fd1E455fc974995eF124dF --value 100ether --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send 0x78a7ddbc8b7a5afee8870d8a40ae5631b959a39e --value 100ether --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	echo ""; \
	echo "⚙️  Initializing contracts..."; \
	cast send "$$COORD_ADDR" "initialize(address,address,address,address,address,address)" "$$ADMIN" "$$JOB_ADDR" "$$APP_ADDR" "$$PROOF_ADDR" "$$SETTLE_ADDR" "$$STORAGE_ADDR" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$APP_ADDR" "setProofManager(address)" "$$PROOF_ADDR" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	echo ""; \
	echo "🔐 Granting roles..."; \
	COORD_ROLE=$$(cast call "$$JOB_ADDR" "COORDINATOR_ROLE()(bytes32)" --rpc-url localhost 2>/dev/null); \
	OPERATOR_ROLE=$$(cast call "$$JOB_ADDR" "OPERATOR_ROLE()(bytes32)" --rpc-url localhost 2>/dev/null); \
	PAUSER_ROLE=$$(cast call "$$COORD_ADDR" "PAUSER_ROLE()(bytes32)" --rpc-url localhost 2>/dev/null); \
	cast send "$$JOB_ADDR" "grantRole(bytes32,address)" "$$COORD_ROLE" "$$COORD_ADDR" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$APP_ADDR" "grantRole(bytes32,address)" "$$COORD_ROLE" "$$COORD_ADDR" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$PROOF_ADDR" "grantRole(bytes32,address)" "$$COORD_ROLE" "$$COORD_ADDR" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$SETTLE_ADDR" "grantRole(bytes32,address)" "$$COORD_ROLE" "$$COORD_ADDR" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$STORAGE_ADDR" "grantRole(bytes32,address)" "$$COORD_ROLE" "$$COORD_ADDR" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$PROOF_ADDR" "grantRole(bytes32,address)" "$$COORD_ROLE" "$$APP_ADDR" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$COORD_ADDR" "grantRole(bytes32,address)" "$$COORD_ROLE" "$$COORD1" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$COORD_ADDR" "grantRole(bytes32,address)" "$$COORD_ROLE" "$$COORD2" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$COORD_ADDR" "grantRole(bytes32,address)" "$$OPERATOR_ROLE" "$$OPERATOR" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$COORD_ADDR" "grantRole(bytes32,address)" "$$PAUSER_ROLE" "$$OPERATOR" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$JOB_ADDR" "grantRole(bytes32,address)" "$$COORD_ROLE" "$$COORD1" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$JOB_ADDR" "grantRole(bytes32,address)" "$$COORD_ROLE" "$$COORD2" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$JOB_ADDR" "grantRole(bytes32,address)" "$$OPERATOR_ROLE" "$$OPERATOR" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$APP_ADDR" "grantRole(bytes32,address)" "$$COORD_ROLE" "$$COORD1" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$APP_ADDR" "grantRole(bytes32,address)" "$$COORD_ROLE" "$$COORD2" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$APP_ADDR" "grantRole(bytes32,address)" "$$OPERATOR_ROLE" "$$OPERATOR" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$PROOF_ADDR" "grantRole(bytes32,address)" "$$COORD_ROLE" "$$COORD1" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$PROOF_ADDR" "grantRole(bytes32,address)" "$$COORD_ROLE" "$$COORD2" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$PROOF_ADDR" "grantRole(bytes32,address)" "$$OPERATOR_ROLE" "$$OPERATOR" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$SETTLE_ADDR" "grantRole(bytes32,address)" "$$COORD_ROLE" "$$COORD1" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$SETTLE_ADDR" "grantRole(bytes32,address)" "$$COORD_ROLE" "$$COORD2" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$SETTLE_ADDR" "grantRole(bytes32,address)" "$$OPERATOR_ROLE" "$$OPERATOR" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$STORAGE_ADDR" "grantRole(bytes32,address)" "$$COORD_ROLE" "$$COORD1" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$STORAGE_ADDR" "grantRole(bytes32,address)" "$$COORD_ROLE" "$$COORD2" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cast send "$$STORAGE_ADDR" "grantRole(bytes32,address)" "$$OPERATOR_ROLE" "$$OPERATOR" --rpc-url localhost --private-key "$$PRIVATE_KEY" --legacy > /dev/null 2>&1; \
	cd ../..; \
	printf '{\n  "timestamp": "%s",\n  "network": "anvil-local",\n  "chainId": 31337,\n  "admin": "%s",\n  "coordinator1": "%s",\n  "coordinator2": "%s",\n  "operator": "%s",\n  "contracts": {\n    "SilvanaCoordination": "%s",\n    "JobManager": "%s",\n    "AppInstanceManager": "%s",\n    "ProofManager": "%s",\n    "SettlementManager": "%s",\n    "StorageManager": "%s"\n  }\n}\n' "$$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$$ADMIN" "$$COORD1" "$$COORD2" "$$OPERATOR" "$$COORD_ADDR" "$$JOB_ADDR" "$$APP_ADDR" "$$PROOF_ADDR" "$$SETTLE_ADDR" "$$STORAGE_ADDR" > "$$DEPLOY_FILE"; \
	echo ""; \
	echo "✅ Local deployment complete!"; \
	echo ""; \
	echo "📋 Deployment Summary:"; \
	echo ""; \
	echo "  Admin:          $$ADMIN"; \
	echo "  Coordinator 1:  $$COORD1"; \
	echo "  Coordinator 2:  $$COORD2"; \
	echo "  Operator:       $$OPERATOR"; \
	echo ""; \
	echo "  Contracts:"; \
	echo "    SilvanaCoordination: $$COORD_ADDR"; \
	echo "    JobManager:          $$JOB_ADDR"; \
	echo "    AppInstanceManager:  $$APP_ADDR"; \
	echo "    ProofManager:        $$PROOF_ADDR"; \
	echo "    SettlementManager:   $$SETTLE_ADDR"; \
	echo "    StorageManager:      $$STORAGE_ADDR"; \
	echo ""; \
	echo "📄 Deployment info saved to: $$DEPLOY_FILE"; \
	echo ""; \
	echo "⚙️  Update coordinator.toml with:"; \
	echo ""; \
	echo "[[ethereum]]"; \
	echo "layer_id = \"ethereum-anvil\""; \
	echo "rpc_url = \"http://localhost:8545\""; \
	echo "ws_url = \"ws://localhost:8545\""; \
	echo "contract_address = \"$$COORD_ADDR\""; \
	echo "job_manager_address = \"$$JOB_ADDR\""; \
	echo "private_key_env = \"ETHEREUM_PRIVATE_KEY\""; \
	echo "operation_mode = \"direct\""; \
	echo "chain_id = 31337"; \
	echo ""; \
	echo "Or run these commands:"; \
	echo "  sed -i '' 's/contract_address = \".*\"/contract_address = \"$$COORD_ADDR\"/' coordinator.toml"; \
	echo "  sed -i '' 's/job_manager_address = \".*\"/job_manager_address = \"$$JOB_ADDR\"/' coordinator.toml"; \
	echo ""

test-ethereum-coordination: ## Run Ethereum coordination layer integration tests
	@echo "🧪 Running Ethereum coordination integration tests..."
	@echo ""
	@if ! nc -z 127.0.0.1 8545 2>/dev/null; then \
		echo "❌ ERROR: Anvil not running on port 8545"; \
		echo ""; \
		echo "Please start Anvil first:"; \
		echo "  anvil"; \
		echo ""; \
		exit 1; \
	fi
	@if [ ! -d build ] || [ -z "$$(ls -A build/ethereum-local-*.json 2>/dev/null)" ]; then \
		echo "⚠️  No deployment found. Running deployment first..."; \
		echo ""; \
		$(MAKE) deploy-ethereum-local; \
		echo ""; \
	fi
	@echo "Running tests (sequential to avoid nonce conflicts)..."
	@cargo test --package silvana-coordination-ethereum --test integration_test -- --test-threads=1
	@echo ""
	@echo "✅ Integration tests complete!"

build-all: build-arm build-x86 build-mac ## Build coordinator for all platforms (Linux ARM64, x86_64, macOS Silicon)
	@echo "✅ Built Silvana for all platforms"

release-archives: build-all ## Build and create release archives for all platforms
	@echo "📦 Creating release archives..."
	@mkdir -p docker/coordinator/release/github
	@echo "📦 Creating ARM64 Linux archive..."
	@cd docker/coordinator/release/arm && tar -czf ../github/silvana-arm64-linux.tar.gz silvana
	@echo "📦 Creating x86_64 Linux archive..."
	@cd docker/coordinator/release/x86 && tar -czf ../github/silvana-x86_64-linux.tar.gz silvana
	@echo "📦 Creating macOS Apple Silicon archive..."
	@cd docker/coordinator/release/mac && tar -czf ../github/silvana-macos-silicon.tar.gz silvana
	@echo "📦 Calculating checksums..."
	@cd docker/coordinator/release/github && shasum -a 256 *.tar.gz > checksums.txt
	@echo "✅ Release archives created in 'docker/coordinator/release/github/' directory:"
	@ls -lh docker/coordinator/release/github/
	@echo ""
	@echo "📝 To create a GitHub release manually:"
	@echo "  1. Go to: https://github.com/SilvanaOne/silvana/releases/new"
	@echo "  2. Upload the files from 'docker/coordinator/release/github/'"
	@echo "  3. Include the checksums.txt for verification"

# Usage: make github-release VERSION=v1.0.0
github-release: release-archives ## Create a GitHub release using gh CLI (all platforms)
	@if [ -z "$(VERSION)" ]; then \
		echo "❌ Error: VERSION is required"; \
		echo "Usage: make github-release VERSION=v1.0.0"; \
		exit 1; \
	fi
	@echo "🚀 Creating GitHub release $(VERSION)"
	@if ! command -v gh >/dev/null 2>&1; then \
		echo "❌ Error: GitHub CLI (gh) is not installed"; \
		echo "Install from: https://cli.github.com/"; \
		exit 1; \
	fi
	@echo "🔄 Fetching latest main branch..."
	@git fetch origin main
	@echo "📌 Creating git tag $(VERSION) from origin/main..."
	@git tag -a $(VERSION) -m "Release $(VERSION)" origin/main 2>/dev/null || echo "Tag already exists"
	@git push origin $(VERSION) 2>/dev/null || echo "Tag already pushed"
	@echo "📦 Creating GitHub release..."
	@gh release create $(VERSION) \
		--title "Silvana $(VERSION)" \
		--generate-notes \
		docker/coordinator/release/github/silvana-arm64-linux.tar.gz \
		docker/coordinator/release/github/silvana-x86_64-linux.tar.gz \
		docker/coordinator/release/github/silvana-macos-silicon.tar.gz \
		docker/coordinator/release/github/checksums.txt
	@echo "✅ Release $(VERSION) created successfully!"

regen: check-database-url check-tools setup proto2entities apply-ddl ## Complete regeneration: proto → DDL+entities → DB
	@echo "🎉 Regeneration complete!"
	@echo ""
	@echo "📝 Summary:"
	@echo "  - DDL generated from proto files"
	@echo "  - Database schema updated"
	@echo "  - Sea-ORM entities regenerated"
	@echo ""
	@echo "🚀 Your application is ready to use the updated schema!"

proto2sql: check-database-url ## Generate DDL from proto files and apply to database
	@echo "🔄 Generating DDL from proto files..."
	@mkdir -p $(SQL_DIR)
	cargo run --manifest-path infra/tidb/proto-to-ddl/Cargo.toml --release -- generate \
		--proto-file $(PROTO_FILE) \
		--output $(SQL_DIR)/rpc.sql
	@echo "✅ DDL generated in $(SQL_DIR)/rpc.sql"
	@echo ""
	@echo "📊 Applying schema changes to database..."
	@DB_URL="$(call load_database_url)"; \
	export DB_USER=$$(echo "$$DB_URL" | sed 's|mysql://||' | sed 's|:.*||'); \
	export DB_PASS=$$(echo "$$DB_URL" | sed 's|mysql://[^:]*:||' | sed 's|@.*||'); \
	export DB_HOST=$$(echo "$$DB_URL" | sed 's|.*@||' | sed 's|:.*||'); \
	export DB_PORT=$$(echo "$$DB_URL" | sed 's|.*:||' | sed 's|/.*||'); \
	export DB_NAME=$$(echo "$$DB_URL" | sed 's|.*/||'); \
	mysqldef --user=$$DB_USER --password=$$DB_PASS --host=$$DB_HOST --port=$$DB_PORT $$DB_NAME \
		--file $(SQL_DIR)/rpc.sql \
		--dry-run > $(MIGR_DIR)/$$(date +%s)_proto_diff.sql
	@echo "🔍 Migration diff saved to $(MIGR_DIR)/"
	@echo "📊 Applying changes to database..."
	@DB_URL="$(call load_database_url)"; \
	export DB_USER=$$(echo "$$DB_URL" | sed 's|mysql://||' | sed 's|:.*||'); \
	export DB_PASS=$$(echo "$$DB_URL" | sed 's|mysql://[^:]*:||' | sed 's|@.*||'); \
	export DB_HOST=$$(echo "$$DB_URL" | sed 's|.*@||' | sed 's|:.*||'); \
	export DB_PORT=$$(echo "$$DB_URL" | sed 's|.*:||' | sed 's|/.*||'); \
	export DB_NAME=$$(echo "$$DB_URL" | sed 's|.*/||'); \
	mysqldef --user=$$DB_USER --password=$$DB_PASS --host=$$DB_HOST --port=$$DB_PORT $$DB_NAME \
		--file $(SQL_DIR)/rpc.sql
	@echo "✅ Database schema updated"

proto2entities: ## Generate both DDL and entities from proto files (combined)
	@echo "🔄 Generating DDL and entities from proto files..."
	@mkdir -p $(SQL_DIR)
	@rm -rf $(ENTITY_DIR)/*
	cargo run --manifest-path infra/tidb/proto-to-ddl/Cargo.toml --release -- generate \
		--proto-file $(PROTO_FILE) \
		--output $(SQL_DIR)/rpc.sql \
		--entities \
		--entity-dir $(ENTITY_DIR)
	@echo "✅ Generated DDL in $(SQL_DIR)/rpc.sql"
	@echo "✅ Generated entities in $(ENTITY_DIR)/"

apply-ddl: check-database-url ## Apply generated DDL to database
	@echo "📊 Applying schema changes to database..."
	@DB_URL=$$(grep "^DATABASE_URL=" .env | cut -d'=' -f2- | sed 's/^["'"'"']//;s/["'"'"']$$//'); \
	export DB_USER=$$(echo "$$DB_URL" | sed 's|mysql://||' | sed 's|:.*||'); \
	export DB_PASS=$$(echo "$$DB_URL" | sed 's|mysql://[^:]*:||' | sed 's|@.*||'); \
	export DB_HOST=$$(echo "$$DB_URL" | sed 's|.*@||' | sed 's|:.*||'); \
	export DB_PORT=$$(echo "$$DB_URL" | sed 's|.*:||' | sed 's|/.*||'); \
	export DB_NAME=$$(echo "$$DB_URL" | sed 's|.*/||'); \
	mysqldef --user=$$DB_USER --password=$$DB_PASS --host=$$DB_HOST --port=$$DB_PORT $$DB_NAME \
		--file $(SQL_DIR)/rpc.sql \
		--dry-run > $(MIGR_DIR)/$$(date +%s)_proto_diff.sql
	@echo "🔍 Migration diff saved to $(MIGR_DIR)/"
	@echo "📊 Applying changes to database..."
	@DB_URL=$$(grep "^DATABASE_URL=" .env | cut -d'=' -f2- | sed 's/^["'"'"']//;s/["'"'"']$$//'); \
	export DB_USER=$$(echo "$$DB_URL" | sed 's|mysql://||' | sed 's|:.*||'); \
	export DB_PASS=$$(echo "$$DB_URL" | sed 's|mysql://[^:]*:||' | sed 's|@.*||'); \
	export DB_HOST=$$(echo "$$DB_URL" | sed 's|.*@||' | sed 's|:.*||'); \
	export DB_PORT=$$(echo "$$DB_URL" | sed 's|.*:||' | sed 's|/.*||'); \
	export DB_NAME=$$(echo "$$DB_URL" | sed 's|.*/||'); \
	mysqldef --user=$$DB_USER --password=$$DB_PASS --host=$$DB_HOST --port=$$DB_PORT $$DB_NAME \
		--file $(SQL_DIR)/rpc.sql
	@echo "✅ Database schema updated"

apply-ddl-state: check-state-database-url ## Apply state.sql schema to STATE_DATABASE_URL from .env
	@echo "📊 Applying state schema to database..."
	@STATE_DB_URL=$$(grep "^STATE_DATABASE_URL=" .env | cut -d'=' -f2- | sed 's/^["'"'"']//;s/["'"'"']$$//'); \
	export STATE_DB_USER=$$(echo "$$STATE_DB_URL" | sed 's|mysql://||' | sed 's|\.root:.*||').root; \
	export STATE_DB_PASS=$$(echo "$$STATE_DB_URL" | sed 's|.*\.root:||' | sed 's|@.*||'); \
	export STATE_DB_HOST=$$(echo "$$STATE_DB_URL" | sed 's|.*@||' | sed 's|:.*||'); \
	export STATE_DB_PORT=$$(echo "$$STATE_DB_URL" | sed 's|.*:||' | sed 's|/.*||'); \
	export STATE_DB_NAME=$$(echo "$$STATE_DB_URL" | sed 's|.*/||'); \
	echo "🔍 Connecting to: $$STATE_DB_HOST:$$STATE_DB_PORT/$$STATE_DB_NAME as $$STATE_DB_USER"; \
	mysqldef --user=$$STATE_DB_USER --password=$$STATE_DB_PASS --host=$$STATE_DB_HOST --port=$$STATE_DB_PORT $$STATE_DB_NAME \
		--file state/state.sql \
		--dry-run > $(MIGR_DIR)/state_$$(date +%s)_diff.sql
	@echo "🔍 Migration diff saved to $(MIGR_DIR)/state_*_diff.sql"
	@echo "📊 Applying changes to state database..."
	@STATE_DB_URL=$$(grep "^STATE_DATABASE_URL=" .env | cut -d'=' -f2- | sed 's/^["'"'"']//;s/["'"'"']$$//'); \
	export STATE_DB_USER=$$(echo "$$STATE_DB_URL" | sed 's|mysql://||' | sed 's|\.root:.*||').root; \
	export STATE_DB_PASS=$$(echo "$$STATE_DB_URL" | sed 's|.*\.root:||' | sed 's|@.*||'); \
	export STATE_DB_HOST=$$(echo "$$STATE_DB_URL" | sed 's|.*@||' | sed 's|:.*||'); \
	export STATE_DB_PORT=$$(echo "$$STATE_DB_URL" | sed 's|.*:||' | sed 's|/.*||'); \
	export STATE_DB_NAME=$$(echo "$$STATE_DB_URL" | sed 's|.*/||'); \
	mysqldef --user=$$STATE_DB_USER --password=$$STATE_DB_PASS --host=$$STATE_DB_HOST --port=$$STATE_DB_PORT $$STATE_DB_NAME \
		--file state/state.sql
	@echo "✅ State database schema updated"

entities: ## Generate Sea-ORM entities from proto file
	@echo "🔄 Generating Sea-ORM entities from proto file..."
	@rm -rf $(ENTITY_DIR)/*
	cargo run --manifest-path infra/tidb/proto-to-ddl/Cargo.toml --release -- generate \
		--proto-file $(PROTO_FILE) \
		--output /dev/null \
		--entities \
		--entity-dir $(ENTITY_DIR)
	@echo "✅ Entities generated in $(ENTITY_DIR)/"

clean-dev: ## Drop all tables for fast development iteration
	@echo "⚠️  Development cleanup: Dropping all tables..."
	@echo "🗑️  This will completely wipe the database!"
	@read -p "Are you sure? (y/N): " confirm && [ "$$confirm" = "y" ] || exit 1
	@echo "🔄 Dropping all tables..."
	cargo run --manifest-path infra/tidb/drop_all_tables/Cargo.toml --release
	@echo "🗑️  Removing generated directories..."
	@rm -rf $(MIGR_DIR)
	@rm -rf $(SQL_DIR)
	@rm -rf $(ENTITY_DIR)
	@echo "✅ Generated directories removed"
	@echo "✅ All tables dropped"
	@echo ""
	@echo "💡 Run 'make regen' to recreate schema from proto files"

clean-dev-state: check-state-database-url ## Drop all tables in state database for fast development iteration
	@echo "⚠️  State database cleanup: Dropping all tables..."
	@echo "🗑️  This will completely wipe the state database!"
	@read -p "Are you sure? (y/N): " confirm && [ "$$confirm" = "y" ] || exit 1
	@echo "🔄 Dropping all state tables..."
	cargo run --manifest-path infra/tidb/drop_state_tables/Cargo.toml --release
	@echo "✅ All state tables dropped"
	@echo ""
	@echo "💡 Run 'make apply-ddl-state' to recreate state schema from state/state.sql"

# Development targets
dev-reset: clean-dev regen ## Full development reset: drop all tables + regenerate from proto

# Secret management targets
store-secret: ## Store a secret via gRPC (requires: DEVELOPER, AGENT, NAME, SECRET, optional: ENDPOINT, APP, APP_INSTANCE)
	@echo "📦 Storing secret via gRPC..."
	@if [ -z "$(DEVELOPER)" ]; then echo "❌ ERROR: DEVELOPER is required. Usage: make store-secret DEVELOPER=alice AGENT=my-agent NAME=api-key SECRET=sk-123"; exit 1; fi
	@if [ -z "$(AGENT)" ]; then echo "❌ ERROR: AGENT is required. Usage: make store-secret DEVELOPER=alice AGENT=my-agent NAME=api-key SECRET=sk-123"; exit 1; fi
	@if [ -z "$(NAME)" ]; then echo "❌ ERROR: NAME is required. Usage: make store-secret DEVELOPER=alice AGENT=my-agent NAME=api-key SECRET=sk-123"; exit 1; fi
	@if [ -z "$(SECRET)" ]; then echo "❌ ERROR: SECRET is required. Usage: make store-secret DEVELOPER=alice AGENT=my-agent NAME=api-key SECRET=sk-123"; exit 1; fi
	@COMMAND="cargo x store-secret --developer $(DEVELOPER) --agent $(AGENT) --name $(NAME) --secret '$(SECRET)'"; \
	if [ -n "$(ENDPOINT)" ]; then COMMAND="$$COMMAND --endpoint $(ENDPOINT)"; fi; \
	if [ -n "$(APP)" ]; then COMMAND="$$COMMAND --app $(APP)"; fi; \
	if [ -n "$(APP_INSTANCE)" ]; then COMMAND="$$COMMAND --app-instance $(APP_INSTANCE)"; fi; \
	eval $$COMMAND

retrieve-secret: ## Retrieve a secret via gRPC (requires: DEVELOPER, AGENT, NAME, optional: ENDPOINT, APP, APP_INSTANCE)
	@echo "🔍 Retrieving secret via gRPC..."
	@if [ -z "$(DEVELOPER)" ]; then echo "❌ ERROR: DEVELOPER is required. Usage: make retrieve-secret DEVELOPER=alice AGENT=my-agent NAME=api-key"; exit 1; fi
	@if [ -z "$(AGENT)" ]; then echo "❌ ERROR: AGENT is required. Usage: make retrieve-secret DEVELOPER=alice AGENT=my-agent NAME=api-key"; exit 1; fi
	@if [ -z "$(NAME)" ]; then echo "❌ ERROR: NAME is required. Usage: make retrieve-secret DEVELOPER=alice AGENT=my-agent NAME=api-key"; exit 1; fi
	@COMMAND="cargo x retrieve-secret --developer $(DEVELOPER) --agent $(AGENT) --name $(NAME)"; \
	if [ -n "$(ENDPOINT)" ]; then COMMAND="$$COMMAND --endpoint $(ENDPOINT)"; fi; \
	if [ -n "$(APP)" ]; then COMMAND="$$COMMAND --app $(APP)"; fi; \
	if [ -n "$(APP_INSTANCE)" ]; then COMMAND="$$COMMAND --app-instance $(APP_INSTANCE)"; fi; \
	eval $$COMMAND

write-config: ## Write configuration to RPC server (requires: CHAIN=[devnet|testnet|mainnet], optional: ENDPOINT)
	@echo "📤 Writing configuration to RPC server..."
	@if [ -z "$(CHAIN)" ]; then echo "❌ ERROR: CHAIN is required. Usage: make write-config CHAIN=devnet"; exit 1; fi
	@if [ "$(CHAIN)" != "devnet" ] && [ "$(CHAIN)" != "testnet" ] && [ "$(CHAIN)" != "mainnet" ]; then \
		echo "❌ ERROR: CHAIN must be devnet, testnet, or mainnet. Got: $(CHAIN)"; exit 1; \
	fi
	@COMMAND="cargo x write-config --chain $(CHAIN)"; \
	if [ ! -z "$(ENDPOINT)" ]; then COMMAND="$$COMMAND --endpoint $(ENDPOINT)"; fi; \
	echo "🔧 Running: $$COMMAND"; \
	eval $$COMMAND

read-config: ## Read configuration from RPC server (requires: CHAIN=[devnet|testnet|mainnet], optional: ENDPOINT)
	@echo "📥 Reading configuration from RPC server..."
	@if [ -z "$(CHAIN)" ]; then echo "❌ ERROR: CHAIN is required. Usage: make read-config CHAIN=devnet"; exit 1; fi
	@if [ "$(CHAIN)" != "devnet" ] && [ "$(CHAIN)" != "testnet" ] && [ "$(CHAIN)" != "mainnet" ]; then \
		echo "❌ ERROR: CHAIN must be devnet, testnet, or mainnet. Got: $(CHAIN)"; exit 1; \
	fi
	@COMMAND="cargo x read-config --chain $(CHAIN)"; \
	if [ ! -z "$(ENDPOINT)" ]; then COMMAND="$$COMMAND --endpoint $(ENDPOINT)"; fi; \
	echo "🔧 Running: $$COMMAND"; \
	eval $$COMMAND

# Utility targets
show-tables: check-database-url ## Show all tables in the RPC database
	@echo "📋 Tables in database:"
	@DB_URL="$(call load_database_url)"; \
	cargo run --manifest-path infra/tidb/proto-to-ddl/Cargo.toml --release -- \
		list-tables --database-url "$$DB_URL"

show-state-tables: check-state-database-url ## Show all tables in the state database
	@echo "📋 Tables in state database:"
	@STATE_DB_URL=$$(grep "^STATE_DATABASE_URL=" .env | cut -d'=' -f2- | sed 's/^["'"'"']//;s/["'"'"']$$//'); \
	export STATE_DB_USER=$$(echo "$$STATE_DB_URL" | sed 's|mysql://||' | sed 's|\.root:.*||').root; \
	export STATE_DB_PASS=$$(echo "$$STATE_DB_URL" | sed 's|.*\.root:||' | sed 's|@.*||'); \
	export STATE_DB_HOST=$$(echo "$$STATE_DB_URL" | sed 's|.*@||' | sed 's|:.*||'); \
	export STATE_DB_PORT=$$(echo "$$STATE_DB_URL" | sed 's|.*:||' | sed 's|/.*||'); \
	export STATE_DB_NAME=$$(echo "$$STATE_DB_URL" | sed 's|.*/||'); \
	mysqldef --user=$$STATE_DB_USER --password=$$STATE_DB_PASS --host=$$STATE_DB_HOST --port=$$STATE_DB_PORT $$STATE_DB_NAME \
		--export 2>/dev/null | grep "^CREATE TABLE" | sed 's/CREATE TABLE //' | sed 's/ (.*$$//' | sed 's/`//g' || echo "❌ Could not retrieve tables"

show-schema: check-database-url ## Show schema for all tables in RPC database  
	@echo "📊 Database schema:"
	@DB_URL="$(call load_database_url)"; \
	export DB_USER=$$(echo "$$DB_URL" | sed 's|mysql://||' | sed 's|:.*||'); \
	export DB_PASS=$$(echo "$$DB_URL" | sed 's|mysql://[^:]*:||' | sed 's|@.*||'); \
	export DB_HOST=$$(echo "$$DB_URL" | sed 's|.*@||' | sed 's|:.*||'); \
	export DB_PORT=$$(echo "$$DB_URL" | sed 's|.*:||' | sed 's|/.*||'); \
	export DB_NAME=$$(echo "$$DB_URL" | sed 's|.*/||'); \
	for table in $$(mysql --user=$$DB_USER --password=$$DB_PASS --host=$$DB_HOST --port=$$DB_PORT $$DB_NAME \
		--execute="SHOW TABLES;" --batch --skip-column-names 2>/dev/null); do \
		echo ""; \
		echo "🔍 Table: $$table"; \
		echo "--------------------------------------------------"; \
		mysql --user=$$DB_USER --password=$$DB_PASS --host=$$DB_HOST --port=$$DB_PORT $$DB_NAME \
			--execute="DESCRIBE $$table;" 2>/dev/null || echo "❌ Could not describe table $$table"; \
	done || echo "❌ Could not retrieve schema"

show-state-schema: check-state-database-url ## Show schema for all tables in state database
	@echo "📊 State database schema:"
	@STATE_DB_URL=$$(grep "^STATE_DATABASE_URL=" .env | cut -d'=' -f2- | sed 's/^["'"'"']//;s/["'"'"']$$//'); \
	export STATE_DB_USER=$$(echo "$$STATE_DB_URL" | sed 's|mysql://||' | sed 's|\.root:.*||').root; \
	export STATE_DB_PASS=$$(echo "$$STATE_DB_URL" | sed 's|.*\.root:||' | sed 's|@.*||'); \
	export STATE_DB_HOST=$$(echo "$$STATE_DB_URL" | sed 's|.*@||' | sed 's|:.*||'); \
	export STATE_DB_PORT=$$(echo "$$STATE_DB_URL" | sed 's|.*:||' | sed 's|/.*||'); \
	export STATE_DB_NAME=$$(echo "$$STATE_DB_URL" | sed 's|.*/||'); \
	mysqldef --user=$$STATE_DB_USER --password=$$STATE_DB_PASS --host=$$STATE_DB_HOST --port=$$STATE_DB_PORT $$STATE_DB_NAME \
		--export 2>/dev/null || echo "❌ Could not retrieve schema"

validate-schema: check-database-url check-tools ## Validate that database schema matches proto definitions
	@echo "🔍 Validating schema consistency..."
	@DB_URL="$(call load_database_url)"; \
	DB_URL=$$(echo "$$DB_URL" | xargs); \
	cargo run --manifest-path infra/tidb/proto-to-ddl/Cargo.toml --release -- validate \
		--proto-file $(PROTO_FILE) \
		--database-url "$$DB_URL"


# Analysis targets
analyze: ## Analyze Sui proof events and compare merge algorithms
	@echo "📊 Running Sui proof event analysis..."
	@python3 scripts/sui_proof_analysis_complete.py

# Example targets
example-archive: ## Prepare archive with example project (examples/add)
	@echo "📦 Packing examples/add folder to S3..."
	@cargo x example-archive

run-state: check-state-database-url ## Run state service locally (reads STATE_DATABASE_URL and STATE_S3_BUCKET from .env)
	@echo "🚀 Starting state service..."
	@if ! grep -q "^STATE_S3_BUCKET=" .env; then \
		echo "⚠️  Warning: STATE_S3_BUCKET not set in .env"; \
		echo "   Large objects (>1MB) will be stored in database instead of S3"; \
		echo ""; \
	fi
	@echo "📝 The service will load configuration from .env file"
	@echo "🔗 gRPC endpoint: 0.0.0.0:50052"
	@echo "🔍 gRPC reflection: enabled"
	@echo ""
	cargo run --release --features binary -p state -- --enable-reflection