# Silvana RPC - Proto-driven Database Schema Management

This project implements a **proto-first** workflow where protobuf definitions are the single source of truth for database schema. The workflow automatically generates DDL from proto files, applies changes to TiDB, and regenerates Sea-ORM entities.

## 🏗️ Architecture Overview

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   .proto file   │───▶│  DDL Generation │───▶│  TiDB Schema    │───▶│  Sea-ORM        │
│ (single truth)  │    │ (proto-to-ddl)  │    │  (mysqldef)     │    │  Entities       │
└─────────────────┘    └─────────────────┘    └─────────────────┘    └─────────────────┘
```

## 🚀 Quick Start

### Prerequisites

1. **Go** (for installing protoc plugins)
2. **Protocol Buffers compiler** (`protoc`)
3. **Rust** with `cargo`
4. **TiDB** or MySQL-compatible database

### Installation

1. Install required tools:

   ```bash
   # Using Makefile
   make install-tools
   ```

2. Set your database URL:

   ```bash
   export DATABASE_URL="mysql://user:password@tcp(host:port)/database"
   ```

3. Generate schema and entities:

   ```bash
   # Complete regeneration
   make regen
   ```

## 📋 Available Commands

### Makefile Commands

```bash
make help           # Show all available commands
make install-tools  # Install required tools (proto-to-ddl, mysqldef, sea-orm-cli)
make check-tools    # Verify tools are installed
make regen          # Complete regeneration workflow
make proto2sql      # Generate DDL and update database
make entities       # Generate Sea-ORM entities from proto
make proto2entities # Generate both DDL and entities from proto
make validate-schema # Validate that DB schema matches proto entities
make clean-dev      # Drop all tables (independent tool - always works)
make dev-reset      # Drop tables + regenerate from proto
make show-tables    # List all database tables
make show-schema    # Show database schema
```

## 🔄 Workflow Details

### 1. Proto to DDL Generation

The workflow uses our custom `proto-to-ddl` Rust tool to convert protobuf messages into TiDB-compatible DDL:

- Each message becomes a table
- Fields map to columns with appropriate types
- `repeated` fields become JSON columns
- Automatic primary key generation
- Support for indexes via comments

### 2. Schema Migration

`mysqldef` handles incremental schema changes:

- Compares generated DDL with current database state
- Generates minimal `ALTER TABLE` statements
- Saves migration diffs for review
- Applies changes safely without data loss

### 3. Entity Generation

Our custom `proto-to-ddl` tool generates Sea-ORM entities directly from proto definitions:

- One entity per proto Event message
- Type-safe column definitions matching proto fields
- Proper handling of optional/repeated fields
- Maintains proto file as single source of truth

## 🛠️ Development Workflow

### Making Schema Changes

1. **Edit proto file**: Modify `proto/silvana/rpc/v1/rpc.proto`
2. **Regenerate**: Run `make regen`
3. **Review changes**: Check generated entities in `src/entity/`
4. **Test**: Run your application with new schema

### Fast Iteration

For rapid development iteration:

```bash
# Drop everything and start fresh
make dev-reset

# Or step by step
make clean-dev  # Drop all tables
make regen      # Regenerate from proto
```

### Migration Management

Migration diffs are automatically saved in `tidb/migration/sql/` with timestamps:

```bash
tidb/migration/sql/
├── 1673123456_proto_diff.sql
├── 1673123789_proto_diff.sql
└── ...
```

These files can be:

- Reviewed before applying changes
- Committed to version control
- Used for deployment automation

## 🔧 Configuration

### Database Connection

Set the `DATABASE_URL` environment variable:

```bash
# Format: mysql://user:password@tcp(host:port)/database
export DATABASE_URL="mysql://root:@tcp(localhost:4000)/silvana_rpc"

# TiDB Cloud example
export DATABASE_URL="mysql://user:pass@tcp(gateway01.us-west-2.prod.aws.tidbcloud.com:4000)/mydb"

# Local TiDB
export DATABASE_URL="mysql://root:@tcp(127.0.0.1:4000)/test"
```

### Environment Variables

| Variable       | Description                      | Required | Default |
| -------------- | -------------------------------- | -------- | ------- |
| `DATABASE_URL` | Complete database connection URL | **Yes**  | None    |

### Type Mappings

| Proto Type | TiDB Type         | Notes                 |
| ---------- | ----------------- | --------------------- |
| `string`   | `VARCHAR(255)`    | Default string length |
| `uint64`   | `BIGINT UNSIGNED` | Large integers        |
| `uint32`   | `INT UNSIGNED`    | Standard integers     |
| `bytes`    | `BLOB`            | Binary data           |
| `bool`     | `BOOLEAN`         | Boolean values        |
