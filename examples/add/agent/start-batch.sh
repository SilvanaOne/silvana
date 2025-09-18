#!/bin/bash

# Script to pull Docker image, start container, and run batch test
# For Ubuntu systems without Node.js/npm installed

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Container name
CONTAINER_NAME="add-agent-batch"

echo -e "${GREEN}Starting batch test in Docker container...${NC}"

# Check if .env and .env.app files exist
if [ ! -f ".env" ]; then
    echo -e "${RED}Error: .env file not found${NC}"
    exit 1
fi

if [ ! -f ".env.app" ]; then
    echo -e "${RED}Error: .env.app file not found${NC}"
    exit 1
fi

# Source the .env file to get DOCKER_IMAGE
set -a
source .env
set +a

if [ -z "$DOCKER_IMAGE" ]; then
    echo -e "${RED}Error: DOCKER_IMAGE not set in .env${NC}"
    exit 1
fi

echo -e "${YELLOW}Using Docker image: $DOCKER_IMAGE${NC}"

# Check if container already exists
if docker ps -a --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
    echo -e "${YELLOW}Container $CONTAINER_NAME already exists, removing it...${NC}"
    docker rm -f $CONTAINER_NAME
fi

# Pull the latest image
echo -e "${GREEN}Pulling Docker image...${NC}"
docker pull $DOCKER_IMAGE

# Prepare environment variables from both .env files
ENV_VARS=""

# Function to parse env file and add to ENV_VARS
parse_env_file() {
    local file=$1
    while IFS= read -r line || [ -n "$line" ]; do
        # Skip comments and empty lines
        if [[ ! "$line" =~ ^[[:space:]]*# ]] && [[ ! -z "$line" ]]; then
            # Remove any trailing comments
            line=$(echo "$line" | sed 's/#.*//')
            # Trim whitespace
            line=$(echo "$line" | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
            if [[ ! -z "$line" ]]; then
                ENV_VARS="$ENV_VARS -e $line"
            fi
        fi
    done < "$file"
}

echo -e "${GREEN}Loading environment variables...${NC}"
parse_env_file ".env"
parse_env_file ".env.app"

# Function to cleanup on exit
cleanup() {
    echo ""
    echo "----------------------------------------"
    echo -e "${YELLOW}Caught interrupt signal, stopping container...${NC}"
    if docker ps --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
        docker stop $CONTAINER_NAME
        echo -e "${GREEN}Container stopped${NC}"
        docker rm $CONTAINER_NAME 2>/dev/null || true
        echo -e "${GREEN}Container removed${NC}"
    fi
    exit 0
}

# Trap Ctrl+C (SIGINT) and call cleanup
trap cleanup INT

# Start the container in detached mode with npm run batch
echo -e "${GREEN}Starting container with batch test...${NC}"

# Also mount the env files as volumes so they're available inside the container
docker run -d \
    --name $CONTAINER_NAME \
    $ENV_VARS \
    -v "$(pwd)/.env:/app/.env:ro" \
    -v "$(pwd)/.env.app:/app/.env.app:ro" \
    --workdir /app \
    $DOCKER_IMAGE \
    npm run batch

# Check if container started successfully
if ! docker ps --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
    echo -e "${RED}Error: Container failed to start${NC}"
    docker logs $CONTAINER_NAME 2>&1
    exit 1
fi

echo -e "${GREEN}Container started successfully${NC}"
echo -e "${YELLOW}Batch test is running... (press Ctrl+C to stop)${NC}"
echo "----------------------------------------"

# Follow the logs
docker logs -f $CONTAINER_NAME 2>&1

# This will only be reached if docker logs fails (container stops on its own)
echo "----------------------------------------"
echo -e "${GREEN}Container stopped${NC}"