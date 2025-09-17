#!/bin/bash

# Script to stop and remove the batch test Docker container

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Container name
CONTAINER_NAME="add-agent-batch"

echo -e "${GREEN}Stopping batch test container...${NC}"

# Check if container is running
if docker ps --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
    echo -e "${YELLOW}Container $CONTAINER_NAME is running${NC}"
    echo -e "${GREEN}Stopping container...${NC}"
    docker stop $CONTAINER_NAME
    echo -e "${GREEN}Container stopped${NC}"

    # Remove the container
    echo -e "${GREEN}Removing container...${NC}"
    docker rm $CONTAINER_NAME 2>/dev/null || true
    echo -e "${GREEN}Container removed${NC}"
else
    # Check if container exists but is stopped
    if docker ps -a --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
        echo -e "${YELLOW}Container exists but is not running${NC}"
        echo -e "${GREEN}Removing container...${NC}"
        docker rm $CONTAINER_NAME
        echo -e "${GREEN}Container removed${NC}"
    else
        echo -e "${GREEN}No batch test container found${NC}"
    fi
fi

echo -e "${GREEN}Cleanup complete!${NC}"