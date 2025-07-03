#!/bin/bash

# Silvana RPC Server Dev Setup Script
# This script sets up Rust and Cargo, and clones the zk-tests repository
# Called from user-data.sh after basic system preparation

set -e  # Exit on any error
sudo dnf install -y git make protobuf-compiler protobuf-devel  --skip-broken

# Verify git is installed before proceeding
if ! command -v git >/dev/null 2>&1; then
    echo "Git installation failed, retrying..."
    sudo dnf install -y git-all
fi

echo "Installing Rust and Cargo..."
sudo -u ec2-user -i bash -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y'
sudo -u ec2-user -i bash -c 'source ~/.cargo/env && rustc --version && cargo --version'

echo "Cloning zk-tests repository as ec2-user into /home/ec2-user..."
if command -v git >/dev/null 2>&1; then
    sudo -u ec2-user -i bash -c 'cd /home/ec2-user && git clone --quiet --no-progress --depth 1 https://github.com/SilvanaOne/zk-tests'
else
    echo "ERROR: Git is still not available after installation attempts"
    exit 1
fi