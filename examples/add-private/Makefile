.PHONY: help setup add run build push clean

help:
	@echo "Add Private Agent - Makefile Commands"
	@echo ""
	@echo "Usage:"
	@echo "  setup          - Generate Ed25519 keypair and create app instance"
	@echo "  add            - Submit add(1, 1) job to Private coordination layer"
	@echo "  run            - Run agent locally to process jobs"
	@echo "  build          - Build Docker image"
	@echo "  push           - Push Docker image to registry"
	@echo "  clean          - Clean build artifacts"
	@echo ""

setup:
	@echo "🔐 Running setup (generate keys + create app instance)..."
	cd agent && cargo run -- setup

add:
	@echo "➕ Submitting add job..."
	cd agent && cargo run -- add

run:
	@echo "🤖 Running agent..."
	cd agent && cargo run -- agent

build:
	@echo "🏗️  Building Docker image..."
	@if [ -z "$(DOCKER_IMAGE)" ]; then \
		echo "❌ Error: DOCKER_IMAGE not set in .env"; \
		exit 1; \
	fi
	cd agent && docker build -t $(DOCKER_IMAGE) .
	@echo "✅ Docker image built: $(DOCKER_IMAGE)"

push:
	@echo "📤 Pushing Docker image..."
	@if [ -z "$(DOCKER_IMAGE)" ]; then \
		echo "❌ Error: DOCKER_IMAGE not set in .env"; \
		exit 1; \
	fi
	docker push $(DOCKER_IMAGE)
	@echo "✅ Docker image pushed: $(DOCKER_IMAGE)"

clean:
	@echo "🧹 Cleaning build artifacts..."
	cd agent && cargo clean
	@echo "✅ Clean complete"
