# XTask Commands

This directory contains automation tasks for the Silvana project.

## Available Commands

### `cargo xtask docker-build`
Build Docker images for every binary.

### `cargo xtask db-reset`
Wipe & re-apply TiDB migrations on the local dev container.

### `cargo xtask store-secret`
Store a secret via gRPC to the secrets storage service.

**Required parameters:**
- `--developer`: Developer identifier
- `--agent`: Agent identifier  
- `--name`: Secret name/key
- `--secret`: Secret value to store

**Optional parameters:**
- `--endpoint`: RPC endpoint (default: http://localhost:50051)
- `--app`: App identifier
- `--app-instance`: App instance identifier

**Examples:**
```bash
# Store a basic secret
cargo xtask store-secret \
  --developer "alice" \
  --agent "my-agent" \
  --name "api-key" \
  --secret "sk-1234567890"

# Store a secret with app context
cargo xtask store-secret \
  --developer "bob" \
  --agent "trading-bot" \
  --app "my-trading-app" \
  --name "database-password" \
  --secret "super-secure-password"

# Store a secret with full context (most specific)
cargo xtask store-secret \
  --developer "charlie" \
  --agent "data-processor" \
  --app "analytics" \
  --app-instance "prod-instance-123" \
  --name "redis-url" \
  --secret "redis://user:pass@host:6379/0"

# Store to a different RPC endpoint
cargo xtask store-secret \
  --endpoint "https://rpc.silvana.ai:443" \
  --developer "alice" \
  --agent "my-agent" \
  --name "prod-api-key" \
  --secret "sk-prod-9876543210"
```

### `cargo xtask retrieve-secret`
Retrieve a secret via gRPC from the secrets storage service.

**Required parameters:**
- `--developer`: Developer identifier
- `--agent`: Agent identifier  
- `--name`: Secret name/key

**Optional parameters:**
- `--endpoint`: RPC endpoint (default: http://localhost:50051)
- `--app`: App identifier
- `--app-instance`: App instance identifier

**Examples:**
```bash
# Retrieve a basic secret
cargo xtask retrieve-secret \
  --developer "alice" \
  --agent "my-agent" \
  --name "api-key"

# Retrieve a secret with app context
cargo xtask retrieve-secret \
  --developer "bob" \
  --agent "trading-bot" \
  --app "my-trading-app" \
  --name "database-password"

# Retrieve a secret with full context
cargo xtask retrieve-secret \
  --developer "charlie" \
  --agent "data-processor" \
  --app "analytics" \
  --app-instance "prod-instance-123" \
  --name "redis-url"
```

## Secret Scoping

Secrets are scoped by the following hierarchy (from most specific to least specific):
1. `developer + agent + app + app_instance + name`
2. `developer + agent + app + name`
3. `developer + agent + name`

When storing secrets, the most specific scope that matches your parameters will be used.
When retrieving secrets, the system will try the most specific scope first and fall back to less specific scopes.

## Makefile Integration

The secret storage commands are also available through the project's Makefile for easier integration with build processes and CI/CD:

### `make store-secret`
Store a secret using Makefile variables.

**Required variables:**
- `DEVELOPER`: Developer identifier
- `AGENT`: Agent identifier
- `NAME`: Secret name/key
- `SECRET`: Secret value to store

**Optional variables:**
- `ENDPOINT`: RPC endpoint (default: http://localhost:50051)
- `APP`: App identifier
- `APP_INSTANCE`: App instance identifier

**Examples:**
```bash
# Store a basic secret
make store-secret DEVELOPER=alice AGENT=my-agent NAME=api-key SECRET=sk-123

# Store with app context
make store-secret DEVELOPER=bob AGENT=trading-bot APP=my-app NAME=db-pass SECRET=secret123

# Store with full context and custom endpoint
make store-secret \
  DEVELOPER=charlie \
  AGENT=processor \
  APP=analytics \
  APP_INSTANCE=prod-123 \
  NAME=redis-url \
  SECRET="redis://user:pass@host:6379/0" \
  ENDPOINT=https://rpc.silvana.ai:443
```

### `make retrieve-secret`
Retrieve a secret using Makefile variables.

**Required variables:**
- `DEVELOPER`: Developer identifier
- `AGENT`: Agent identifier
- `NAME`: Secret name/key

**Optional variables:**
- `ENDPOINT`: RPC endpoint (default: http://localhost:50051)
- `APP`: App identifier
- `APP_INSTANCE`: App instance identifier

**Examples:**
```bash
# Retrieve a basic secret
make retrieve-secret DEVELOPER=alice AGENT=my-agent NAME=api-key

# Retrieve with app context
make retrieve-secret DEVELOPER=bob AGENT=trading-bot APP=my-app NAME=db-pass
```

## Notes

- The gRPC endpoint defaults to `http://localhost:50051` but can be overridden
- Signature validation is not yet implemented (placeholder signatures are used)
- All secret values are stored as strings and encrypted at rest using AWS KMS
- The secrets are stored in DynamoDB with envelope encryption
- Both direct `cargo xtask` commands and `make` commands are available for flexibility