# AddApp Quick Start Guide

Quick reference for deploying and using the AddApp example.

## Prerequisites

1. **Anvil running**:

```bash
anvil
```

2. **Coordination contracts deployed**:

```bash
cd ../../../
make deploy-ethereum-local
```

3. **Get the coordination address** from deployment output:

```bash
export SILVANA_COORDINATION_ADDRESS=0x5eb3Bc0a489C5A8288765d2336659EbCA68FCd00
```

## Deploy AddApp

```bash
cd examples/add-ethereum/solidity

# Deploy
forge script script/Deploy.s.sol:DeployAddAppLocal \
  --rpc-url localhost \
  --broadcast \
  --legacy

# Save the AddApp address from output
export ADD_APP_ADDRESS=0x...
```

## Use AddApp

### Add Values

```bash
# Add 50 at index 1
cast send $ADD_APP_ADDRESS "add(uint32,uint256)" 1 50 \
  --rpc-url localhost \
  --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
  --legacy

# Add 30 at index 2
cast send $ADD_APP_ADDRESS "add(uint32,uint256)" 2 30 \
  --rpc-url localhost \
  --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
  --legacy

# Add 15 more at index 1 (will be 50 + 15 = 65)
cast send $ADD_APP_ADDRESS "add(uint32,uint256)" 1 15 \
  --rpc-url localhost \
  --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
  --legacy
```

### Query State

```bash
# Get total sum (should be 95 after above commands)
cast call $ADD_APP_ADDRESS "getSum()(uint256)" --rpc-url localhost

# Get value at index 1 (should be 65)
cast call $ADD_APP_ADDRESS "getValue(uint32)(uint256)" 1 --rpc-url localhost

# Get value at index 2 (should be 30)
cast call $ADD_APP_ADDRESS "getValue(uint32)(uint256)" 2 --rpc-url localhost

# Get current sequence (should be 3)
cast call $ADD_APP_ADDRESS "getSequence()(uint256)" --rpc-url localhost

# Get state commitment
cast call $ADD_APP_ADDRESS "getStateCommitment()(bytes32)" --rpc-url localhost
```

### Check Jobs Created

```bash
# Get app instance ID
APP_INSTANCE=$(cast call $ADD_APP_ADDRESS "appInstanceId()(string)" --rpc-url localhost)
echo "App Instance: $APP_INSTANCE"

# Get job manager address
JOB_MANAGER=$(cast call $SILVANA_COORDINATION_ADDRESS "jobManager()(address)" --rpc-url localhost)
echo "Job Manager: $JOB_MANAGER"

# Note: You can query jobs through the coordination contracts
# See coordination contract documentation for job query methods
```

## Run Tests

```bash
# Make sure Anvil and coordination contracts are running
forge test --rpc-url localhost -vvv

# Run specific test
forge test --match-test testAddSingleValue -vvv

# With gas reporting
forge test --gas-report
```

## Example Session

```bash
# 1. Deploy
forge script script/Deploy.s.sol:DeployAddAppLocal --rpc-url localhost --broadcast --legacy
export ADD_APP_ADDRESS=0x9A9f2CCfdE556A7E9Ff0848998Aa4a0CFD8863AE

# 2. Add some values
cast send $ADD_APP_ADDRESS "add(uint32,uint256)" 1 10 --rpc-url localhost --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 --legacy
cast send $ADD_APP_ADDRESS "add(uint32,uint256)" 2 20 --rpc-url localhost --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 --legacy
cast send $ADD_APP_ADDRESS "add(uint32,uint256)" 3 30 --rpc-url localhost --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 --legacy

# 3. Check sum (should be 60)
cast call $ADD_APP_ADDRESS "getSum()(uint256)" --rpc-url localhost

# 4. Check individual values
cast call $ADD_APP_ADDRESS "getValue(uint32)(uint256)" 1 --rpc-url localhost  # Returns 10
cast call $ADD_APP_ADDRESS "getValue(uint32)(uint256)" 2 --rpc-url localhost  # Returns 20
cast call $ADD_APP_ADDRESS "getValue(uint32)(uint256)" 3 --rpc-url localhost  # Returns 30
```

## Constraints

- **Value must be < 100**: Values >= 100 will revert
- **Index must be > 0**: Index 0 is reserved (for conceptual sum storage)
- **Index must be < 1,000,000**: Maximum index constraint
- **Sum can overflow**: Be careful with many large values (unlikely with max value 99)

## Troubleshooting

### "Coordination is zero address"

Set the environment variable:

```bash
export SILVANA_COORDINATION_ADDRESS=0x5eb3Bc0a489C5A8288765d2336659EbCA68FCd00
```

### "NotInitialized" error

The app instance wasn't created. Redeploy with the deployment script.

### "Failed to create app instance"

Ensure coordination contracts are deployed and accessible.

### Anvil not responding

Restart Anvil and redeploy coordination contracts:

```bash
# Terminal 1
anvil

# Terminal 2
cd /path/to/silvana
make deploy-ethereum-local
```

## Next Steps

- Check the [README.md](README.md) for detailed documentation
- Review the [AddApp.sol](src/AddApp.sol) source code
- Explore the [test suite](test/AddApp.t.sol) for usage examples
- Compare with the [Sui Move version](../../add/move/sources/main.move)
