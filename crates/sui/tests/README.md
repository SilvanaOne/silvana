# Registry Tests

This directory contains tests for the Silvana registry functions.

## Running the Tests

### Prerequisites

Set the following environment variables (or create a `.env` file in the project root):

```bash
# Required for all tests
export SUI_ADDRESS="your_sui_address"
export SUI_SECRET_KEY="your_sui_private_key"
export SUI_CHAIN="testnet"  # or "mainnet", "devnet"

# Optional - uses coordination package if not set
export SILVANA_REGISTRY_PACKAGE="0x32f8ad21df94c28401912c8ffebcc3bd186f5bf7da0995057a63755005937025"
```

#### Using a .env file

Create a `.env` file in the project root with:

```env
SUI_ADDRESS=0x...your_address...
SUI_SECRET_KEY=suiprivkey1...your_private_key...
SUI_CHAIN=testnet
SILVANA_REGISTRY_PACKAGE=0x32f8ad21df94c28401912c8ffebcc3bd186f5bf7da0995057a63755005937025
```

The tests will automatically load the `.env` file using `dotenvy`.

### Run All Registry Tests

```bash
cargo test --package sui --test registry_test
```

### Run Specific Test

```bash
# Test basic registry operations
cargo test --package sui --test registry_test test_registry_operations

# Test with multiple entities
cargo test --package sui --test registry_test test_registry_with_multiple_entities

# Test with environment package ID
cargo test --package sui --test registry_test test_registry_with_env_package_id
```

### Run with Debug Output

```bash
RUST_LOG=info cargo test --package sui --test registry_test -- --nocapture
```

## Test Structure

The tests follow the same pattern as the TypeScript tests in `silvana-lib`:

1. **Create Registry**: Creates a new test registry with a random name
2. **Developer Operations**: Tests add, update, and remove developer functions
3. **Agent Operations**: Tests add, update, and remove agent functions
4. **App Operations**: Tests add, update, and remove app functions
5. **Cleanup**: Removes all entities in reverse order

Each test uses randomly generated names with prefixes like:
- `test_registry_XXXXXXXX`
- `developer_XXXXXXXX`
- `agent_XXXXXXXX`
- `app_XXXXXXXX`

This ensures tests don't conflict with each other or existing data.

## Test Functions

- `test_registry_operations()`: Complete end-to-end test of all registry functions
- `test_registry_with_multiple_entities()`: Tests multiple developers, agents, and apps
- `test_registry_with_env_package_id()`: Tests using SILVANA_REGISTRY_PACKAGE env var