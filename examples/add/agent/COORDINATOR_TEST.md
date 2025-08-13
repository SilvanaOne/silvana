# Coordinator Test Environment

This directory contains test scripts to create a test environment for the Silvana Coordinator.

## Prerequisites

1. Ensure you have the coordinator built:
   ```bash
   cd /path/to/silvana
   cargo build --bin coordinator
   ```

2. Install dependencies:
   ```bash
   npm install
   ```

3. Ensure you have a valid `.env` file with Sui credentials.

## Testing Workflow

### Step 1: Deploy a Test App

First, create a new app instance on Sui that will generate events:

```bash
npm run deploy
```

This will:
- Create a new app on Sui devnet
- Save the app ID and package ID to `.env.app`
- Display the configuration needed for the coordinator

### Step 2: Configure the Coordinator

After running deploy, copy the `COORDINATION_PACKAGE_ID` from `.env.app` to your coordinator's `.env`:

```bash
# In silvana/crates/coordinator/.env
COORDINATION_PACKAGE_ID=<package_id_from_env_app>
COORDINATION_MODULE=main
```

### Step 3: Start the Coordinator

In a separate terminal, start the coordinator:

```bash
cd /path/to/silvana
cargo run --bin coordinator
```

The coordinator will:
- Connect to Sui via gRPC
- Monitor for events from the coordination package
- Process any events by launching Docker containers

### Step 4: Generate Test Events

In another terminal, run the event generator:

```bash
npm run send
```

This will:
- Send random "add" and "multiply" actions every 20 seconds
- Generate events that the coordinator will detect
- Display the current state after each action
- Run continuously until stopped (Ctrl+C)

## Event Flow

1. **send.test.ts** → Sends transactions to Sui contract
2. **Sui Contract** → Emits events (ValueAddedEvent, ValueMultipliedEvent)
3. **Coordinator** → Monitors events via gRPC
4. **Coordinator** → Processes events and manages Docker containers

## Files

- `test/deploy.test.ts` - Creates a new app and saves configuration
- `test/send.test.ts` - Generates random events continuously
- `.env.app` - Generated configuration file (created by deploy.test.ts)

## Monitoring

Watch the coordinator logs to see:
- Event detection
- Docker container launches
- Processing results

## Troubleshooting

1. **No events detected**: Ensure the COORDINATION_PACKAGE_ID matches the deployed package
2. **Connection errors**: Check that Sui RPC URL is accessible
3. **Docker errors**: Ensure Docker daemon is running

## Example Output

### Deploy Output:
```
🚀 Creating new app for coordinator testing...
📦 Package ID: 0x86c3d88659cef230aa594906747b42d13dce0daf20cbaaef8e8f76d6db7aee13
✅ App created successfully!
📦 App ID: 0x123...abc
💾 Configuration saved to .env.app
```

### Send Output:
```
🚀 Action #1
   Type: add
   Index: 3
   Value: 42
✅ Action completed in 1234ms
   Old sum: 0
   New sum: 42
⏱️  Waiting 20 seconds before next action...
```

### Coordinator Output:
```
🚀 Starting Silvana Coordinator
📦 Monitoring package: 0x86c3d88659cef230aa594906747b42d13dce0daf20cbaaef8e8f76d6db7aee13
✅ Coordinator initialized, starting event monitoring...
Processing event: type=ValueAddedEvent, checkpoint=12345, timestamp=1234567890
```