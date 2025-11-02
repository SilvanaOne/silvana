# AddApp - Ethereum Solidity Example

A simple example application demonstrating integration with the Silvana coordination layer on Ethereum.

This is the Solidity equivalent of the [Sui Move add example](../../add/move/).

## Overview

AddApp is a minimal stateful application that:

- Maintains indexed `uint256` values (indexes 1-999,999)
- Tracks a running sum of all values (conceptual index 0)
- Creates coordination layer jobs for each `add` operation
- Tracks state commitments for all state transitions
- Enforces that values must be less than 100

## Architecture

### Contract Structure

```
AddApp
├── State Variables
│   ├── sum (uint256)                    - Total sum of all values
│   ├── values (mapping)                 - Indexed value storage
│   ├── sequence (uint256)               - Current sequence number
│   ├── stateCommitment (bytes32)        - Current state hash
│   └── appInstanceId (string)           - Coordination layer instance ID
│
├── Main Functions
│   ├── add(index, value)                - Add value at index, create job
│   ├── getValue(index)                  - Read value at index
│   └── getSum()                         - Read total sum
│
└── Integration
    └── Creates jobs via ICoordination.jobManager().createJob()
```

### Comparison with Sui Move Version

| Feature            | Sui Move                              | Ethereum Solidity            |
| ------------------ | ------------------------------------- | ---------------------------- |
| **State Model**    | Object-based with shared AppInstance  | Contract-based with mappings |
| **Commitments**    | BLS12-381 group elements              | keccak256 hashes             |
| **Data Encoding**  | BCS (Binary Canonical Serialization)  | ABI encoding                 |
| **Job Creation**   | Via coordination::app_instance module | Via ICoordination interface  |
| **Access Control** | Capability-based (AppInstanceCap)     | Address-based (owner)        |
| **Events**         | Sui event system                      | Solidity events              |

## Prerequisites

1. **Running Anvil instance**:

   ```bash
   anvil
   ```

2. **Deployed Silvana coordination contracts**:

   ```bash
   make deploy-ethereum-local
   ```

3. **Foundry installed**:
   ```bash
   curl -L https://foundry.paradigm.xyz | bash
   foundryup
   ```

## Setup

Set the coordination contract address (from deployment output):

```bash
export SILVANA_COORDINATION_ADDRESS=0x5eb3Bc0a489C5A8288765d2336659EbCA68FCd00
```

## Deployment

### Deploy to Local Anvil

```bash
cd examples/add-ethereum/solidity

# Deploy using the local deployment script
forge script script/Deploy.s.sol:DeployAddAppLocal \
  --rpc-url localhost \
  --broadcast \
  --legacy

# Save the deployed AddApp address
export ADD_APP_ADDRESS=<address_from_output>
```

## Usage

### Add Values

```bash
# Add value 50 at index 1
cast send $ADD_APP_ADDRESS \
  "add(uint32,uint256)" 1 50 \
  --rpc-url localhost \
  --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
  --legacy

# Add value 30 at index 2
cast send $ADD_APP_ADDRESS \
  "add(uint32,uint256)" 2 30 \
  --rpc-url localhost \
  --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
  --legacy

# Add value 15 more at index 1
cast send $ADD_APP_ADDRESS \
  "add(uint32,uint256)" 1 15 \
  --rpc-url localhost \
  --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
  --legacy
```

### Query State

```bash
# Get value at index 1
cast call $ADD_APP_ADDRESS "getValue(uint32)(uint256)" 1 --rpc-url localhost

# Get total sum
cast call $ADD_APP_ADDRESS "getSum()(uint256)" --rpc-url localhost

# Get current sequence
cast call $ADD_APP_ADDRESS "getSequence()(uint256)" --rpc-url localhost

# Get current state commitment
cast call $ADD_APP_ADDRESS "getStateCommitment()(bytes32)" --rpc-url localhost
```

### Check Created Jobs

```bash
# Get the app instance ID
APP_INSTANCE=$(cast call $ADD_APP_ADDRESS "appInstanceId()(string)" --rpc-url localhost)

# Check pending jobs (requires coordination contract)
cast call $SILVANA_COORDINATION_ADDRESS \
  "jobManager()(address)" \
  --rpc-url localhost
```

## Testing

Run the test suite:

```bash
cd examples/add-ethereum/solidity

# Run all tests
forge test --rpc-url localhost -vvv

# Run specific test
forge test --match-test testAddSingleValue -vvv

# Run with gas reporting
forge test --gas-report
```
