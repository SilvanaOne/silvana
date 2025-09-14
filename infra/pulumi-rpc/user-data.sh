#!/bin/bash

# AWS EC2 User Data Script for Silvana RPC Server
# This script performs initial system setup and then calls start.sh from the S3 bucket



# Set up logging
exec > >(tee /var/log/user-data.log)
exec 2>&1
echo "Starting user-data script execution at $(date)"

# Update the instance first
echo "Updating the system..."
sudo dnf update -y

# Install required packages (using dnf consistently for Amazon Linux 2023)
# Note: Skip curl since curl-minimal provides the functionality and conflicts with curl package
echo "Installing required packages..."
sudo dnf install -y awscli nano gcc libcap --skip-broken

echo "Downloading RPC app and scripts"
if aws s3 cp s3://silvana-tee-images/rpc.tar.gz /home/ec2-user/rpc.tar.gz 2>/dev/null; then
    echo "✅ Found existing rpc app in S3, extracting..."
    sudo tar -xzf /home/ec2-user/rpc.tar.gz -C /home/ec2-user/
    sudo chown -R ec2-user:ec2-user /home/ec2-user/rpc
    sudo setcap 'cap_net_bind_service=+ep' /home/ec2-user/rpc/rpc
else
    echo "📋 No existing rpc app found in S3"
    exit 1
fi

# Run the main setup script from the cloned repository
echo "Running Silvana RPC setup script..."
if [ -f "/home/ec2-user/rpc/start.sh" ]; then
    sudo bash /home/ec2-user/rpc/start.sh
    setup_exit_code=$?
    if [ $setup_exit_code -eq 0 ]; then
        echo "✅ Silvana RPC setup completed successfully"
    else
        echo "❌ Silvana RPC setup failed with exit code: $setup_exit_code"
        echo "Check /var/log/start-script.log for detailed error information"
        exit 1
    fi
else
    echo "ERROR: start.sh script not found in rpc folder"
    echo "Expected location: /home/ec2-user/rpc/start.sh"
    ls -la /home/ec2-user/rpc/ || echo "Directory listing failed"
    exit 1
fi

echo "User-data script completed successfully at $(date)"

