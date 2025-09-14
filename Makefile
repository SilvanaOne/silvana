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

# mysqldef supports DATABASE_URL directly, no parsing needed

.PHONY: help install-tools regen proto2sql entities clean-dev setup check-tools check-database-url validate-schema check-schema show-tables show-schema apply-ddl proto2entities dev-reset build store-secret retrieve-secret write-config read-config

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
	@grep -E '^(build-rpc|build-arm|build-x86|build-mac|build-all|release-archives|github-release):.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'
	@echo ""
	@echo "🗃️  DATABASE MANAGEMENT:"
	@grep -E '^(regen|proto2sql|proto2entities|apply-ddl|entities):.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'
	@echo ""
	@echo "🔧 DEVELOPMENT & DEBUGGING:"
	@grep -E '^(clean-dev|dev-reset|show-tables|show-schema|validate-schema|check-schema):.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'
	@echo ""
	@echo "🔐 SECRET MANAGEMENT & CONFIG:"
	@grep -E '^(store-secret|retrieve-secret|write-config|read-config):.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'
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
	@echo ""
	@echo "  Build and Release:"
	@echo "    make build-all                    # Build for all platforms"
	@echo "    make github-release VERSION=v1.0.0 # Create GitHub release"
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

build-rpc: ## Build ARM64 RPC for Graviton and create tar archive, upload to S3
	@echo "🐳 Building RPC and creating deployment archive for Amazon Linux 2023 ARM64..."
	@mkdir -p build
	@echo "🔨 Building Docker image for ARM64 (Graviton), compiling RPC, and creating tar archive..."
	@DOCKER_BUILDKIT=1 docker build \
		--platform linux/arm64 \
		--secret id=aws,src=docker/rpc/.env \
		-f docker/rpc/Dockerfile \
		-t rpc-builder:al2023-arm64 \
		--progress=plain \
		.
	@echo "🧹 Cleaning up Docker image..."
	@docker rmi rpc-builder:al2023-arm64 2>/dev/null || true
	@echo "✅ RPC deployment archive built successfully for ARM64"
	@echo "📦 Archive: rpc.tar.gz (contains rpc folder + ARM64 RPC executable)"
	@echo "🔄 Touching user-data.sh to trigger Pulumi redeploy..."
	@touch infra/pulumi-rpc/user-data.sh

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
show-tables: check-database-url ## Show all tables in the database
	@echo "📋 Tables in database:"
	@DB_URL="$(call load_database_url)"; \
	export DB_USER=$$(echo "$$DB_URL" | sed 's|mysql://||' | sed 's|:.*||'); \
	export DB_PASS=$$(echo "$$DB_URL" | sed 's|mysql://[^:]*:||' | sed 's|@.*||'); \
	export DB_HOST=$$(echo "$$DB_URL" | sed 's|.*@||' | sed 's|:.*||'); \
	export DB_PORT=$$(echo "$$DB_URL" | sed 's|.*:||' | sed 's|/.*||'); \
	export DB_NAME=$$(echo "$$DB_URL" | sed 's|.*/||'); \
	mysql --user=$$DB_USER --password=$$DB_PASS --host=$$DB_HOST --port=$$DB_PORT $$DB_NAME \
		--execute="SHOW TABLES;" 2>/dev/null || echo "❌ Could not connect to database"

show-schema: check-database-url ## Show schema for all tables  
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

validate-schema: check-database-url check-tools ## Validate that database schema matches proto definitions
	@echo "🔍 Validating schema consistency..."
	@DB_URL="$(call load_database_url)"; \
	cargo run --manifest-path infra/tidb/proto-to-ddl/Cargo.toml --release -- validate \
		--proto-file $(PROTO_FILE) \
		--database-url "$$DB_URL"


# Example targets
example-archive: ## Prepare archive with example project (examples/add)
	@echo "📦 Packing examples/add folder to S3..."
	@cargo x example-archive 