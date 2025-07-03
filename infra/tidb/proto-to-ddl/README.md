# proto-to-ddl

A Rust tool that generates MySQL DDL from Protocol Buffer files, designed specifically for the Silvana RPC proto-first workflow.

## Features

- **Proto-first design**: Keeps protobuf files as the single source of truth
- **Event message focus**: Automatically extracts `*Event` messages as database tables
- **Smart type mapping**: Converts protobuf types to appropriate MySQL/Rust types
- **Automatic indexing**: Creates indexes for common patterns (IDs, timestamps, hashes)
- **JSON support**: Maps `repeated` fields to JSON columns
- **Snake case conversion**: Converts CamelCase to snake_case for table/column names
- **Sea-ORM entity generation**: Generates type-safe Rust entities directly from proto definitions
- **Optional/repeated field handling**: Proper Rust Option<T> and JSON types

## Usage

```bash
# Build the tool
cargo build --release

# Generate DDL from proto file
./target/release/proto-to-ddl \
    --proto-file ../proto/events.proto \
    --output ../sql/events.sql

# Generate both DDL and Sea-ORM entities
./target/release/proto-to-ddl \
    --proto-file ../proto/events.proto \
    --output ../sql/events.sql \
    --entities

# With custom entity directory
./target/release/proto-to-ddl \
    --proto-file ../proto/events.proto \
    --output ../sql/events.sql \
    --entities \
    --entity-dir src/entities

# With database prefix
./target/release/proto-to-ddl \
    --proto-file ../proto/events.proto \
    --output ../sql/events.sql \
    --database mydb \
    --entities
```

## Type Mappings

| Proto Type                    | MySQL Type        | Notes                    |
| ----------------------------- | ----------------- | ------------------------ |
| `string`                      | `VARCHAR(255)`    | Default string type      |
| `bytes`                       | `BLOB`            | Binary data              |
| `int32`, `sint32`, `sfixed32` | `INT`             | 32-bit signed integers   |
| `int64`, `sint64`, `sfixed64` | `BIGINT`          | 64-bit signed integers   |
| `uint32`, `fixed32`           | `INT UNSIGNED`    | 32-bit unsigned integers |
| `uint64`, `fixed64`           | `BIGINT UNSIGNED` | 64-bit unsigned integers |
| `float`                       | `FLOAT`           | Single precision         |
| `double`                      | `DOUBLE`          | Double precision         |
| `bool`                        | `BOOLEAN`         | Boolean values           |
| `repeated T`                  | `JSON`            | Arrays stored as JSON    |

## Rust Type Mappings (Entity Generation)

| Proto Type                    | Rust Type (Required) | Rust Type (Optional) | Rust Type (Repeated)        |
| ----------------------------- | -------------------- | -------------------- | --------------------------- |
| `string`                      | `String`             | `Option<String>`     | `Option<serde_json::Value>` |
| `bytes`                       | `Vec<u8>`            | `Option<Vec<u8>>`    | `Option<serde_json::Value>` |
| `int32`, `sint32`, `sfixed32` | `i32`                | `Option<i32>`        | `Option<serde_json::Value>` |
| `int64`, `sint64`, `sfixed64` | `i64`                | `Option<i64>`        | `Option<serde_json::Value>` |
| `uint32`, `fixed32`           | `u32`                | `Option<u32>`        | `Option<serde_json::Value>` |
| `uint64`, `fixed64`           | `i64`                | `Option<i64>`        | `Option<serde_json::Value>` |
| `float`                       | `f32`                | `Option<f32>`        | `Option<serde_json::Value>` |
| `double`                      | `f64`                | `Option<f64>`        | `Option<serde_json::Value>` |
| `bool`                        | `bool`               | `Option<bool>`       | `Option<serde_json::Value>` |

## Generated Schema Features

- **Auto-increment ID**: Every table gets a `BIGINT AUTO_INCREMENT PRIMARY KEY`
- **Metadata columns**: `created_at` and `updated_at` timestamps
- **Smart indexing**: Automatic indexes on:
  - Fields containing "id" (except primary key)
  - Fields containing "timestamp"
  - Fields containing "hash"
  - `created_at` column

## Message Filtering

The tool focuses on event messages:

- Includes: Messages ending with "Event" (e.g., `CoordinatorStartedEvent`)
- Excludes: Union types (`CoordinatorEvent`, `AgentEvent`, `Event`)
- Excludes: Request/Response messages
- Excludes: Utility messages like `Timestamp`

## Example

Given this proto message:

```protobuf
message CoordinatorStartedEvent {
  string coordinator_id = 1;
  string ethereum_address = 2;
  uint64 timestamp = 3;
  repeated string tags = 4;
}
```

Generates this DDL:

```sql
CREATE TABLE IF NOT EXISTS coordinator_started_event (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    coordinator_id VARCHAR(255) NOT NULL,
    ethereum_address VARCHAR(255) NOT NULL,
    timestamp BIGINT UNSIGNED NOT NULL,
    tags JSON NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    INDEX idx_created_at (created_at),
    INDEX idx_coordinator_id (coordinator_id),
    INDEX idx_timestamp (timestamp)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
```

## Dependencies

- `clap`: Command line argument parsing
- `anyhow`: Error handling
- `regex`: Pattern matching for proto parsing
- `Inflector`: String case conversion
- `serde_json`: JSON handling (future use)

## Testing

```bash
# Run tests
cargo test

# Test specific functionality
cargo test test_proto_type_conversion
cargo test test_field_parsing
```

## Integration

This tool is integrated into the Silvana RPC workflow via:

- `Makefile`: `make proto2sql` target
- `proto_regen.sh`: Shell script automation
- Build process: `make install-tools` builds the tool

The tool is designed to work seamlessly with:

- **mysqldef**: Applies generated DDL to database
- **sea-orm-cli**: Generates Rust entities from updated schema
