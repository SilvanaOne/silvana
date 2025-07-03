#!/bin/bash

# Silvana RPC Server Setup Script
# This script sets up NATS JetStream, Nginx with TLS, and gRPC proxy
# Called from user-data.sh after basic system preparation

set -e  # Exit on any error

# Set up logging
exec > >(tee -a /var/log/start-script.log)
exec 2>&1
echo "Starting Silvana RPC setup script at $(date)"

# Configuration
DOMAIN_NAME="rpc-dev.silvana.dev"
EMAIL="dev@silvana.one"
NATS_VERSION="2.11.6"
NATS_CLI_VERSION="0.2.3"

echo "🚀 Initializing Silvana RPC server setup..."

# -------------------------
# Fetch Environment Variables from Parameter Store
# -------------------------
echo "Configuring AWS CLI for ec2-user..."
sudo -u ec2-user mkdir -p /home/ec2-user/.aws

# Set default region
cat <<EOF | sudo -u ec2-user tee /home/ec2-user/.aws/config
[default]
region = eu-central-1
output = json
EOF

# Set proper permissions
sudo chown -R ec2-user:ec2-user /home/ec2-user/.aws
sudo chmod 600 /home/ec2-user/.aws/config

# Verify AWS access is working
echo "Verifying AWS access..."
if sudo -u ec2-user aws sts get-caller-identity >/dev/null 2>&1; then
    echo "✅ AWS access verified successfully"
else
    echo "❌ AWS access verification failed"
    echo "Checking instance metadata and IAM role..."
    curl -s http://169.254.169.254/latest/meta-data/iam/security-credentials/ || echo "No IAM role attached"
    exit 1
fi

echo "Fetching .env from Parameter Store..."
sudo -u ec2-user aws ssm get-parameter \
     --name "/silvana-rpc/dev/env" \
     --with-decryption \
     --query Parameter.Value \
     --output text > /home/ec2-user/rpc/.env

# Lock down permissions
sudo chown ec2-user:ec2-user /home/ec2-user/rpc/.env
sudo chmod 600 /home/ec2-user/rpc/.env
echo "✅ Environment variables fetched, updated, and secured"

# -------------------------
# Prepare NATS JetStream Server
# -------------------------
echo "Preparing NATS JetStream server (will install after TLS certificates)..."

# Create nats user
sudo useradd -r -s /bin/false nats 2>/dev/null || echo "nats user already exists"

# Download and install NATS server for ARM64 (Graviton)
echo "Downloading NATS server v${NATS_VERSION}..."
wget -q "https://github.com/nats-io/nats-server/releases/download/v${NATS_VERSION}/nats-server-v${NATS_VERSION}-linux-arm64.tar.gz" -O /tmp/nats-server.tar.gz
cd /tmp
tar -xzf nats-server.tar.gz
sudo mv "nats-server-v${NATS_VERSION}-linux-arm64/nats-server" /usr/local/bin/
sudo chmod +x /usr/local/bin/nats-server
rm -rf /tmp/nats-server*

# Create NATS directories
sudo mkdir -p /etc/nats /var/lib/nats/jetstream /var/log/nats
sudo chown -R nats:nats /var/lib/nats /var/log/nats

echo "✅ NATS server binaries prepared"

# -------------------------
# Nginx and SSL Certificate Setup
# -------------------------
echo "Setting up Nginx and SSL certificates..."

# Install certbot
echo "Installing nginx and certbot..."
sudo dnf install -y certbot python3-certbot-nginx nginx

# Verify nginx is installed and create directories
if ! command -v sudo nginx >/dev/null 2>&1; then
    echo "ERROR: Nginx installation failed"
    exit 1
fi

sudo mkdir -p /etc/nginx/conf.d /var/log/nginx /var/cache/nginx

# Start and enable Nginx service
echo "Starting nginx service..."
sudo systemctl start nginx && sudo systemctl enable nginx
sleep 2

# Create nginx user if needed
if ! id nginx >/dev/null 2>&1; then
    echo "Creating nginx user..."
    sudo useradd -r -s /bin/false nginx
fi

# Prepare webroot for ACME challenges
sudo mkdir -p /var/www/letsencrypt/.well-known/acme-challenge
sudo chown -R nginx:nginx /var/www/letsencrypt

# Create initial Nginx configuration for HTTP and ACME challenges
echo "Creating nginx configuration..."
cat <<EOF | sudo tee /etc/nginx/conf.d/rpc-silvana.conf
server {
    listen 80;
    server_name ${DOMAIN_NAME};

    location ^~ /.well-known/acme-challenge/ {
        alias /var/www/letsencrypt/.well-known/acme-challenge/;
    }

    location / {
        return 301 https://\$host\$request_uri;
    }
}
EOF

# Test and reload nginx
echo "Testing nginx configuration..."
if sudo nginx -t; then
    echo "✅ Initial nginx configuration is valid"
    sudo systemctl reload nginx
else
    echo "❌ Initial nginx configuration failed"
    exit 1
fi

# Check for existing certificates in S3 first
echo "Checking for existing SSL certificates in S3..."
if sudo -u ec2-user aws s3 cp s3://silvana-tee-images/rpc-cert.tar.gz /tmp/rpc-cert.tar.gz 2>/dev/null; then
    echo "✅ Found existing certificates in S3, extracting..."
    cd /tmp
    sudo tar -xzf rpc-cert.tar.gz -C /
    
    # Verify certificates were extracted successfully
    if sudo test -f "/etc/letsencrypt/live/${DOMAIN_NAME}/fullchain.pem"; then
        echo "✅ Certificates restored from S3 successfully"
        cert_from_s3=true
    else
        echo "⚠️  Certificate extraction failed, will obtain new certificates"
        cert_from_s3=false
    fi
else
    echo "📋 No existing certificates found in S3, will obtain new ones"
    cert_from_s3=false
fi

# Obtain SSL certificates if not restored from S3
if [ "$cert_from_s3" = false ]; then
    echo "Obtaining new SSL certificates..."
    sudo certbot certonly --webroot -w /var/www/letsencrypt --non-interactive --agree-tos -m "$EMAIL" -d "$DOMAIN_NAME"
    
    # Upload new certificates to S3 for future use
    if sudo test -f "/etc/letsencrypt/live/${DOMAIN_NAME}/fullchain.pem"; then
        echo "📤 Uploading new certificates to S3..."
        cd /tmp
        sudo tar -czf rpc-cert.tar.gz -C / etc/letsencrypt/live/${DOMAIN_NAME} etc/letsencrypt/archive/${DOMAIN_NAME} etc/letsencrypt/renewal/${DOMAIN_NAME}.conf
        sudo -u ec2-user aws s3 cp rpc-cert.tar.gz s3://silvana-tee-images/rpc-cert.tar.gz
        echo "✅ Certificates uploaded to S3 for future deployments"
        sudo rm -f /tmp/rpc-cert.tar.gz
    else
        echo "❌ Certificate generation failed"
        exit 1
    fi
fi

# The gRPC server handles TLS directly on port 443
echo "✅ nginx configured for HTTP only (gRPC uses direct TLS)"

# Test and reload nginx with final configuration
echo "Testing final nginx configuration..."
if sudo nginx -t; then
    echo "✅ Final nginx configuration is valid"
    sudo systemctl reload nginx
    
    # Verify nginx is listening on port 80 (HTTP only)
    sleep 2
    if sudo netstat -tlnp | grep -q ":80.*nginx"; then
        echo "✅ nginx is listening on port 80 (HTTP/redirect only)"
    else
        echo "❌ nginx is NOT listening on port 80"
        echo "Checking nginx error logs..."
        sudo tail -n 10 /var/log/nginx/error.log
        exit 1
    fi
else
    echo "❌ Final nginx configuration failed"
    sudo nginx -t  # Show the error details
    exit 1
fi

# Copy certificates to RPC project directory for easy access
echo "Copying SSL certificates to RPC project directory..."
if sudo test -f "/etc/letsencrypt/live/${DOMAIN_NAME}/fullchain.pem"; then
    # Create certificates directory in RPC project
    sudo mkdir -p /home/ec2-user/rpc/certs
    
    # Copy certificates to RPC project directory with proper ownership
    sudo cp "/etc/letsencrypt/live/${DOMAIN_NAME}/fullchain.pem" /home/ec2-user/rpc/certs/
    sudo cp "/etc/letsencrypt/live/${DOMAIN_NAME}/privkey.pem" /home/ec2-user/rpc/certs/
    
    # Set proper ownership and permissions
    sudo chown ec2-user:ec2-user /home/ec2-user/rpc/certs/*
    sudo chmod 600 /home/ec2-user/rpc/certs/*
    
    # Add TLS certificate paths to .env file
    echo "" | sudo -u ec2-user tee -a /home/ec2-user/rpc/.env
    echo "# TLS Certificate Configuration" | sudo -u ec2-user tee -a /home/ec2-user/rpc/.env
    echo "TLS_CERT_PATH=/home/ec2-user/rpc/certs/fullchain.pem" | sudo -u ec2-user tee -a /home/ec2-user/rpc/.env
    echo "TLS_KEY_PATH=/home/ec2-user/rpc/certs/privkey.pem" | sudo -u ec2-user tee -a /home/ec2-user/rpc/.env
    echo "SERVER_ADDRESS=0.0.0.0:443" | sudo -u ec2-user tee -a /home/ec2-user/rpc/.env
    
    echo "✅ SSL certificates copied to RPC project directory"
    echo "   📁 Location: /home/ec2-user/rpc/certs/"
    echo "✅ TLS certificate paths added to .env file"
else
    echo "❌ SSL certificates not found - gRPC server will run without TLS"
    echo "SERVER_ADDRESS=0.0.0.0:50051" | sudo -u ec2-user tee -a /home/ec2-user/rpc/.env
fi

# -------------------------
# Setup SSL Certificate Auto-Renewal
# -------------------------
echo "Setting up automatic SSL renewal..."

cat <<'EOF' | sudo tee /etc/systemd/system/certbot-renew.service
[Unit]
Description=Renew Let\'s Encrypt certificates

[Service]
Type=oneshot
ExecStart=/usr/bin/certbot renew --quiet --deploy-hook "/usr/bin/systemctl reload nginx && /usr/local/bin/upload-renewed-certs.sh && /usr/local/bin/update-rpc-certs.sh"
EOF

cat <<'EOF' | sudo tee /etc/systemd/system/certbot-renew.timer
[Unit]
Description=Run certbot-renew twice daily

[Timer]
OnCalendar=*-*-* 00,12:00:00
RandomizedDelaySec=1h
Persistent=true

[Install]
WantedBy=timers.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now certbot-renew.timer

# Create script to upload renewed certificates to S3
echo "Creating certificate upload script..."
cat <<UPLOAD_SCRIPT | sudo tee /usr/local/bin/upload-renewed-certs.sh
#!/bin/bash
# Script to upload renewed certificates to S3
DOMAIN_NAME="${DOMAIN_NAME}"

echo "\$(date): Uploading renewed certificates to S3..."
cd /tmp
tar -czf rpc-cert-renewed.tar.gz -C / etc/letsencrypt/live/\${DOMAIN_NAME} etc/letsencrypt/archive/\${DOMAIN_NAME} etc/letsencrypt/renewal/\${DOMAIN_NAME}.conf

if sudo -u ec2-user aws s3 cp rpc-cert-renewed.tar.gz s3://silvana-tee-images/rpc-cert.tar.gz; then
    echo "\$(date): ✅ Renewed certificates uploaded to S3 successfully"
    rm -f rpc-cert-renewed.tar.gz
else
    echo "\$(date): ❌ Failed to upload renewed certificates to S3"
fi
UPLOAD_SCRIPT

sudo chmod +x /usr/local/bin/upload-renewed-certs.sh

# Create script to update RPC certificates when they get renewed
echo "Creating RPC certificate update script..."
cat <<UPDATE_RPC_SCRIPT | sudo tee /usr/local/bin/update-rpc-certs.sh
#!/bin/bash
# Script to update RPC project certificates after renewal
DOMAIN_NAME="${DOMAIN_NAME}"
RPC_CERTS_DIR="/home/ec2-user/rpc/certs"

echo "\$(date): Updating RPC project certificates..."

if [ -f "/etc/letsencrypt/live/\${DOMAIN_NAME}/fullchain.pem" ]; then
    # Copy renewed certificates to RPC project
    cp "/etc/letsencrypt/live/\${DOMAIN_NAME}/fullchain.pem" "\${RPC_CERTS_DIR}/"
    cp "/etc/letsencrypt/live/\${DOMAIN_NAME}/privkey.pem" "\${RPC_CERTS_DIR}/"
    
    # Set proper ownership and permissions
    chown ec2-user:ec2-user "\${RPC_CERTS_DIR}"/*
    chmod 600 "\${RPC_CERTS_DIR}"/*
    
    echo "\$(date): ✅ RPC project certificates updated successfully"
else
    echo "\$(date): ❌ Failed to find renewed certificates"
fi
UPDATE_RPC_SCRIPT

sudo chmod +x /usr/local/bin/update-rpc-certs.sh

# -------------------------
# Install and Configure NATS JetStream with TLS
# -------------------------
echo "Installing and configuring NATS JetStream server with TLS..."

# Install NATS CLI tool (ARM64 for Graviton)
echo "Installing NATS CLI tool..."
wget -q "https://github.com/nats-io/natscli/releases/download/v${NATS_CLI_VERSION}/nats-${NATS_CLI_VERSION}-arm64.rpm" -O /tmp/nats-cli.rpm
if sudo dnf install -y /tmp/nats-cli.rpm; then
    echo "✅ NATS CLI v${NATS_CLI_VERSION} installed successfully"
    nats --version 2>/dev/null || echo "📋 NATS CLI ready for use"
else
    echo "⚠️  NATS CLI installation failed, continuing without CLI"
fi
rm -f /tmp/nats-cli.rpm

# Setup certificate permissions for NATS
echo "Setting up certificate access for NATS..."
sudo groupadd ssl-cert 2>/dev/null || true
sudo usermod -a -G ssl-cert nats

# Create renewal hooks directory
sudo mkdir -p /etc/letsencrypt/renewal-hooks/deploy

# Create certificate permission script for renewals
cat <<CERT_SCRIPT | sudo tee /etc/letsencrypt/renewal-hooks/deploy/nats-cert-permissions.sh
#!/bin/bash
chgrp ssl-cert /etc/letsencrypt/live/${DOMAIN_NAME}/fullchain.pem
chgrp ssl-cert /etc/letsencrypt/live/${DOMAIN_NAME}/privkey.pem
chmod 640 /etc/letsencrypt/live/${DOMAIN_NAME}/fullchain.pem
chmod 640 /etc/letsencrypt/live/${DOMAIN_NAME}/privkey.pem
systemctl reload-or-restart nats-server
systemctl restart silvana-rpc 2>/dev/null || echo "RPC service not yet available"
CERT_SCRIPT

sudo chmod +x /etc/letsencrypt/renewal-hooks/deploy/nats-cert-permissions.sh

# Configure and start NATS
if sudo test -f "/etc/letsencrypt/live/${DOMAIN_NAME}/fullchain.pem"; then
    echo "Setting certificate permissions for NATS..."
    sudo chgrp ssl-cert /etc/letsencrypt/live/${DOMAIN_NAME}/fullchain.pem
    sudo chgrp ssl-cert /etc/letsencrypt/live/${DOMAIN_NAME}/privkey.pem
    sudo chmod 640 /etc/letsencrypt/live/${DOMAIN_NAME}/fullchain.pem
    sudo chmod 640 /etc/letsencrypt/live/${DOMAIN_NAME}/privkey.pem

    echo "Creating NATS configuration with TLS..."
    cat <<EOF | sudo tee /etc/nats/nats-server.conf
# NATS Server Configuration with JetStream and TLS
host: 0.0.0.0
port: 4222

tls {
    cert_file: "/etc/letsencrypt/live/${DOMAIN_NAME}/fullchain.pem"
    key_file: "/etc/letsencrypt/live/${DOMAIN_NAME}/privkey.pem"
    timeout: 5
}

http_port: 8222

websocket {
    host: 0.0.0.0
    port: 8080
    compression: true
    tls {
        cert_file: "/etc/letsencrypt/live/${DOMAIN_NAME}/fullchain.pem"
        key_file: "/etc/letsencrypt/live/${DOMAIN_NAME}/privkey.pem"
    }
}

jetstream {
    store_dir: "/var/lib/nats/jetstream"
    max_memory_store: 100MB
    max_file_store: 1GB
    sync_interval: 1s
}

log_file: "/var/log/nats/nats-server.log"
log_size_limit: 100MB
max_traced_msg_len: 32768
max_payload: 1MB
max_pending: 256MB
max_connections: 64K
write_deadline: "10s"
EOF

    nats_config="TLS enabled"
else
    echo "⚠️  SSL certificates not found, configuring NATS without TLS"
    cat <<EOF | sudo tee /etc/nats/nats-server.conf
# NATS Server Configuration with JetStream (No TLS)
host: 0.0.0.0
port: 4222
http_port: 8222

websocket {
    host: 0.0.0.0
    port: 8080
    compression: true
}

jetstream {
    store_dir: "/var/lib/nats/jetstream"
    max_memory_store: 100MB
    max_file_store: 1GB
    sync_interval: 1s
}

log_file: "/var/log/nats/nats-server.log"
log_size_limit: 100MB
max_traced_msg_len: 32768
max_payload: 1MB
max_pending: 256MB
max_connections: 64K
write_deadline: "10s"
EOF

    nats_config="No TLS (certificates not available)"
fi

# Create NATS systemd service
echo "Creating NATS systemd service..."
cat <<EOF | sudo tee /etc/systemd/system/nats-server.service
[Unit]
Description=NATS JetStream Server
Documentation=https://docs.nats.io/
After=network.target
Wants=network.target

[Service]
Type=simple
User=nats
Group=nats
ExecStart=/usr/local/bin/nats-server -c /etc/nats/nats-server.conf
ExecReload=/bin/kill -s HUP \$MAINPID
KillMode=process
Restart=always
RestartSec=5s
LimitNOFILE=1000000
LimitNPROC=1000000

NoNewPrivileges=true
PrivateTmp=true
ProtectHome=true
ProtectSystem=strict
ReadWritePaths=/var/lib/nats /var/log/nats

[Install]
WantedBy=multi-user.target
EOF

# Start NATS server
echo "Starting NATS JetStream server..."
sudo systemctl daemon-reload
sudo systemctl enable nats-server
sudo systemctl start nats-server

sleep 5

# Verify NATS status
if sudo systemctl is-active --quiet nats-server; then
    echo "✅ NATS JetStream server started successfully (${nats_config})"
    if [ "$nats_config" = "TLS enabled" ]; then
        echo "🔒 NATS (TLS): nats://${DOMAIN_NAME}:4222"
        echo "🔒 NATS-WS (TLS): wss://${DOMAIN_NAME}:8080/ws"
    else
        echo "🔓 NATS: nats://${DOMAIN_NAME}:4222"
        echo "🔓 NATS-WS: ws://${DOMAIN_NAME}:8080/ws"
    fi
    echo "📊 NATS monitoring: http://${DOMAIN_NAME}:8222"
else
    echo "⚠️  NATS server failed to start, checking logs..."
    sudo journalctl -u nats-server -n 10 --no-pager
fi

# -------------------------
# Setup RPC Server Service
# -------------------------
echo "Setting up Silvana RPC server service..."

# Note: .env file is already fetched from Parameter Store and placed at /home/ec2-user/rpc/.env

# Create Silvana RPC systemd service
echo "Creating Silvana RPC systemd service..."
cat <<EOF | sudo tee /etc/systemd/system/silvana-rpc.service
[Unit]
Description=Silvana RPC Server
Documentation=https://github.com/SilvanaOne/zk-tests/tree/main/packages/avs/rpc
After=network.target nats-server.service
Wants=network.target
Requires=nats-server.service

[Service]
Type=simple
User=ec2-user
Group=ec2-user
WorkingDirectory=/home/ec2-user/rpc
EnvironmentFile=/home/ec2-user/rpc/.env

# Use the pre-built RPC server binary
ExecStart=/home/ec2-user/rpc/rpc

# Restart configuration
Restart=always
RestartSec=10s
StartLimitInterval=300s
StartLimitBurst=5

# Resource limits
LimitNOFILE=65536
LimitNPROC=4096

# Security settings
NoNewPrivileges=false
PrivateTmp=true
ProtectHome=false
ProtectSystem=strict
ReadWritePaths=/home/ec2-user/rpc/logs /var/log

# Capabilities for binding to privileged ports
AmbientCapabilities=CAP_NET_BIND_SERVICE
CapabilityBoundingSet=CAP_NET_BIND_SERVICE

# Logging
StandardOutput=journal
StandardError=journal
SyslogIdentifier=silvana-rpc

# Graceful shutdown
KillMode=mixed
KillSignal=SIGTERM
TimeoutStopSec=30s

[Install]
WantedBy=multi-user.target
EOF

# Note: RPC binary is pre-built and available at /home/ec2-user/rpc/rpc

# Create RPC management script
echo "Creating RPC management script..."
cat <<'MANAGEMENT_SCRIPT' | sudo tee /usr/local/bin/rpc-service.sh
#!/bin/bash
# Management script for Silvana RPC service

SCRIPT_NAME="$(basename "$0")"
SERVICE_NAME="silvana-rpc"

usage() {
    echo "Usage: $SCRIPT_NAME {start|stop|restart|status|logs}"
    echo ""
    echo "Commands:"
    echo "  start     - Start the RPC service"
    echo "  stop      - Stop the RPC service"
    echo "  restart   - Restart the RPC service"
    echo "  status    - Show service status"
    echo "  logs      - Show recent logs (follow with -f)"
    exit 1
}

case "${1:-}" in
    start)
        echo "Starting Silvana RPC service..."
        systemctl start "$SERVICE_NAME"
        systemctl status "$SERVICE_NAME" --no-pager
        ;;
    stop)
        echo "Stopping Silvana RPC service..."
        systemctl stop "$SERVICE_NAME"
        ;;
    restart)
        echo "Restarting Silvana RPC service..."
        systemctl restart "$SERVICE_NAME"
        systemctl status "$SERVICE_NAME" --no-pager
        ;;
    status)
        systemctl status "$SERVICE_NAME" --no-pager
        ;;
    logs)
        if [ "${2:-}" = "-f" ]; then
            journalctl -u "$SERVICE_NAME" -f
        else
            journalctl -u "$SERVICE_NAME" -n 50 --no-pager
        fi
        ;;
    *)
        usage
        ;;
esac
MANAGEMENT_SCRIPT

sudo chmod +x /usr/local/bin/rpc-service.sh

# Verify RPC binary exists and set proper permissions
echo "Verifying RPC server binary..."
if [ -f "/home/ec2-user/rpc/rpc" ]; then
    echo "✅ RPC server binary found at /home/ec2-user/rpc/rpc"
    # Ensure proper ownership and executable permissions
    sudo chown ec2-user:ec2-user /home/ec2-user/rpc/rpc
    sudo chmod +x /home/ec2-user/rpc/rpc
    # Set capability to bind to privileged ports (443)
    sudo setcap 'cap_net_bind_service=+ep' /home/ec2-user/rpc/rpc
    echo "✅ RPC server permissions and capabilities set"
else
    echo "❌ RPC server binary not found at /home/ec2-user/rpc/rpc"
    echo "Expected binary location: /home/ec2-user/rpc/rpc"
    ls -la /home/ec2-user/rpc/ || echo "Directory listing failed"
    exit 1
fi

# Create required directories for RPC service
echo "Creating RPC service directories..."
sudo mkdir -p /home/ec2-user/rpc/logs
sudo chown ec2-user:ec2-user /home/ec2-user/rpc/logs
sudo chmod 755 /home/ec2-user/rpc/logs
echo "✅ RPC logs directory created and configured"

# Enable and start the RPC service
echo "Enabling and starting Silvana RPC service..."
sudo systemctl daemon-reload
sudo systemctl enable silvana-rpc

# Wait a moment for NATS to be fully ready
echo "Waiting for NATS server to be fully ready..."
sleep 10

# Start the RPC service
if sudo systemctl start silvana-rpc; then
    echo "✅ Silvana RPC service started successfully"
    
    # Check service status
    sleep 5
    if sudo systemctl is-active --quiet silvana-rpc; then
        echo "✅ Silvana RPC service is running and healthy"
    else
        echo "⚠️  Silvana RPC service may have issues, checking logs..."
        sudo journalctl -u silvana-rpc -n 10 --no-pager
    fi
else
    echo "❌ Failed to start Silvana RPC service"
    echo "📋 Service logs:"
    sudo journalctl -u silvana-rpc -n 20 --no-pager
fi

# -------------------------
# Summary
# -------------------------
echo ""
echo "🎉 Silvana RPC server setup completed at $(date)"
echo ""
echo "📋 Services Status:"
echo "   • Nginx: $(sudo systemctl is-active nginx)"
echo "   • NATS JetStream: $(sudo systemctl is-active nats-server)"
echo "   • Silvana RPC: $(sudo systemctl is-active silvana-rpc)"
echo "   • SSL Auto-renewal: $(sudo systemctl is-active certbot-renew.timer)"
echo ""
echo "🌐 Endpoints:"
echo "   • gRPC + gRPC-Web (Direct TLS): https://${DOMAIN_NAME}:443"
echo "   • HTTP Redirect: http://${DOMAIN_NAME}:80"
if [ "$nats_config" = "TLS enabled" ]; then
echo "   • NATS (TLS): nats://${DOMAIN_NAME}:4222"
echo "   • NATS-WS (TLS): wss://${DOMAIN_NAME}:8080/ws"
else
echo "   • NATS: nats://${DOMAIN_NAME}:4222"
echo "   • NATS-WS: ws://${DOMAIN_NAME}:8080/ws"
fi
echo "   • NATS Monitoring: http://${DOMAIN_NAME}:8222"
echo "   • Prometheus Metrics: http://${DOMAIN_NAME}:9090/metrics"
echo ""
echo "🔧 Management Commands:"
echo "   • RPC service control: sudo rpc-service.sh {start|stop|restart|status|logs}"
echo "   • Check RPC status: sudo systemctl status silvana-rpc"
echo "   • View RPC logs: sudo journalctl -u silvana-rpc -f"
echo "   • Check NATS status: sudo systemctl status nats-server"
echo "   • Check Nginx status: sudo systemctl status nginx"
echo "   • View NATS logs: sudo journalctl -u nats-server -f"
echo "   • NATS CLI: nats --help"
echo ""

echo "RPC server started! 🚀" 


