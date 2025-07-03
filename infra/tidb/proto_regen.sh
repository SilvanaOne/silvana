#!/bin/bash

# Silvana RPC - Proto-driven Schema Regeneration Script
# ====================================================
# This script implements the workflow where proto files are the single source of truth
# Usage: ./proto_regen.sh [command]

set -e  # Exit on any error

# Configuration
PROTO_FILES="proto/events.proto"
SQL_DIR="tidb/sql"
MIGR_DIR="tidb/migration/sql"
ENTITY_DIR="src/entity"

# Database configuration
check_database_url() {
    if [ -z "$DATABASE_URL" ]; then
        log_error "DATABASE_URL environment variable is not set"
        echo ""
        echo "Please set DATABASE_URL with the format:"
        echo "  export DATABASE_URL=\"mysql://user:pass@tcp(host:port)/database\""
        echo ""
        echo "Examples:"
        echo "  export DATABASE_URL=\"mysql://root:@tcp(localhost:4000)/silvana_rpc\""
        echo "  export DATABASE_URL=\"mysql://user:pass@tcp(myhost.com:3306)/mydb\""
        echo ""
        exit 1
    fi
}

# Parse DATABASE_URL to extract components for mysqldef
# Format: mysql://user:pass@tcp(host:port)/dbname
parse_database_url() {
    # Extract user
    DB_USER=$(echo "$DATABASE_URL" | sed -n 's|mysql://\([^:]*\):.*|\1|p')
    
    # Extract password (handle empty password)
    DB_PASS=$(echo "$DATABASE_URL" | sed -n 's|mysql://[^:]*:\([^@]*\)@.*|\1|p')
    
    # Extract host
    DB_HOST=$(echo "$DATABASE_URL" | sed -n 's|.*@tcp(\([^:]*\):.*|\1|p')
    
    # Extract port
    DB_PORT=$(echo "$DATABASE_URL" | sed -n 's|.*@tcp([^:]*:\([0-9]*\)).*|\1|p')
    
    # Extract database name
    DB_NAME=$(echo "$DATABASE_URL" | sed -n 's|.*/\([^?]*\).*|\1|p')
}

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Helper functions
log_info() {
    echo -e "${BLUE}ℹ️  $1${NC}"
}

log_success() {
    echo -e "${GREEN}✅ $1${NC}"
}

log_warning() {
    echo -e "${YELLOW}⚠️  $1${NC}"
}

log_error() {
    echo -e "${RED}❌ $1${NC}"
}

# Check if required tools are installed
check_tools() {
    log_info "Checking required tools..."
    
    local missing_tools=""
    
    if ! test -f "tidb/proto-to-ddl/target/release/proto-to-ddl"; then
        missing_tools="${missing_tools}proto-to-ddl "
    fi
    
    if ! command -v mysqldef &> /dev/null; then
        missing_tools="${missing_tools}mysqldef "
    fi
    
    if [ -n "$missing_tools" ]; then
        log_error "Missing required tools: $missing_tools"
        log_info "Run './proto_regen.sh install-tools' to install them"
        exit 1
    fi
    
    log_success "All required tools are available"
}

# Install required tools
install_tools() {
    log_info "Installing required tools..."
    
    log_info "Building proto-to-ddl Rust tool..."
    cd tidb/proto-to-ddl && cargo build --release && cd ../..
    
    log_info "Installing mysqldef..."
    go install github.com/sqldef/sqldef/cmd/mysqldef@latest
    
    log_success "All tools installed"
}

# Setup directories
setup_dirs() {
    log_info "Setting up directories..."
    mkdir -p "$SQL_DIR"
    mkdir -p "$MIGR_DIR"
    mkdir -p "$ENTITY_DIR"
    log_success "Directories created"
}

# Generate DDL from proto and apply to database
proto_to_sql() {
    # Check that DATABASE_URL is set
    check_database_url
    
    log_info "Generating DDL from proto files..."
    ./tidb/proto-to-ddl/target/release/proto-to-ddl generate \
        --proto-file $PROTO_FILES \
        --output "$SQL_DIR/events.sql"
    log_success "DDL generated in $SQL_DIR/events.sql"
    
    echo ""
    log_info "Applying schema changes to database..."
    
    # Generate migration diff first
    TIMESTAMP=$(date +%s)
    MIGRATION_FILE="$MIGR_DIR/${TIMESTAMP}_proto_diff.sql"
    
    # Parse DATABASE_URL for mysqldef
    parse_database_url
    
    mysqldef --user="$DB_USER" --password="$DB_PASS" --host="$DB_HOST" --port="$DB_PORT" "$DB_NAME" \
        --file "$SQL_DIR/events.sql" \
        --dry-run > "$MIGRATION_FILE"
    
    log_info "Migration diff saved to $MIGRATION_FILE"
    
    # Apply changes to database
    log_info "Applying changes to database..."
    mysqldef --user="$DB_USER" --password="$DB_PASS" --host="$DB_HOST" --port="$DB_PORT" "$DB_NAME" \
        --file "$SQL_DIR/events.sql"
    
    log_success "Database schema updated"
}

# Generate Sea-ORM entities from proto files
generate_entities() {
    log_info "Generating Sea-ORM entities from proto files..."
    
    # Remove existing entities
    rm -rf "$ENTITY_DIR"/*
    
    # Generate new entities using proto-to-ddl
    ./tidb/proto-to-ddl/target/release/proto-to-ddl generate \
        --proto-file $PROTO_FILES \
        --output /dev/null \
        --entities \
        --entity-dir "$ENTITY_DIR"
    
    log_success "Entities generated in $ENTITY_DIR/"
}

# Drop all tables for development iteration
clean_dev() {
    # Check that DATABASE_URL is set
    check_database_url
    
    log_warning "Development cleanup: Dropping all tables..."
    log_warning "This will completely wipe the database!"
    
    read -p "Are you sure? (y/N): " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        log_info "Operation cancelled"
        exit 0
    fi
    
    log_info "Dropping all tables..."
    DATABASE_URL="$DATABASE_URL" cargo run --manifest-path tidb/drop_all_tables/Cargo.toml
    log_success "All tables dropped"
    
    echo ""
    log_info "💡 Run './proto_regen.sh regen' to recreate schema from proto files"
}

# Show all tables in the database
show_tables() {
    # Check that DATABASE_URL is set
    check_database_url
    
    # Parse DATABASE_URL to get connection details
    parse_database_url
    
    mysql -h "$DB_HOST" -P "$DB_PORT" -u "$DB_USER" -p"$DB_PASS" "$DB_NAME" -e "SHOW TABLES;"
}

# Show schema for all tables
show_schema() {
    # Check that DATABASE_URL is set
    check_database_url
    
    # Parse DATABASE_URL to get connection details
    parse_database_url
    
    FIRST_TABLE=$(mysql -h "$DB_HOST" -P "$DB_PORT" -u "$DB_USER" -p"$DB_PASS" "$DB_NAME" -e 'SHOW TABLES;' -s -N | head -1)
    if [ -n "$FIRST_TABLE" ]; then
        mysql -h "$DB_HOST" -P "$DB_PORT" -u "$DB_USER" -p"$DB_PASS" "$DB_NAME" -e "SHOW CREATE TABLE \`$FIRST_TABLE\`;"
    else
        echo "No tables found"
    fi
}

# Generate both DDL and entities from proto files (combined)
proto2entities() {
    log_info "Generating DDL and entities from proto files..."
    
    # Remove existing entities
    rm -rf "$ENTITY_DIR"/*
    
    # Generate both DDL and entities using proto-to-ddl
    ./tidb/proto-to-ddl/target/release/proto-to-ddl generate \
        --proto-file $PROTO_FILES \
        --output "$SQL_DIR/events.sql" \
        --entities \
        --entity-dir "$ENTITY_DIR"
    
    log_success "Generated DDL in $SQL_DIR/events.sql"
    log_success "Generated entities in $ENTITY_DIR/"
}

# Apply generated DDL to database
apply_ddl() {
    # Check that DATABASE_URL is set
    check_database_url
    
    log_info "Applying schema changes to database..."
    
    # Generate migration diff first
    TIMESTAMP=$(date +%s)
    MIGRATION_FILE="$MIGR_DIR/${TIMESTAMP}_proto_diff.sql"
    
    # Parse DATABASE_URL for mysqldef
    parse_database_url
    
    mysqldef --user="$DB_USER" --password="$DB_PASS" --host="$DB_HOST" --port="$DB_PORT" "$DB_NAME" \
        --file "$SQL_DIR/events.sql" \
        --dry-run > "$MIGRATION_FILE"
    
    log_info "Migration diff saved to $MIGRATION_FILE"
    
    # Apply changes to database
    log_info "Applying changes to database..."
    mysqldef --user="$DB_USER" --password="$DB_PASS" --host="$DB_HOST" --port="$DB_PORT" "$DB_NAME" \
        --file "$SQL_DIR/events.sql"
    
    log_success "Database schema updated"
}

# Complete regeneration workflow
regen() {
    echo "🚀 Silvana RPC Schema Regeneration"
    echo "=================================="
    echo ""
    
    # Check that DATABASE_URL is set first
    check_database_url
    echo "Database URL: $DATABASE_URL"
    echo ""
    
    check_tools
    setup_dirs
    proto2entities
    apply_ddl
    
    echo ""
    log_success "🎉 Regeneration complete!"
    echo ""
    echo "📝 Summary:"
    echo "  - DDL generated from proto files"
    echo "  - Database schema updated"
    echo "  - Sea-ORM entities regenerated"
    echo ""
    echo "🚀 Your application is ready to use the updated schema!"
}

# Show help
show_help() {
    echo "Silvana RPC Proto-driven Schema Management"
    echo "========================================="
    echo ""
    echo "Usage: $0 [command]"
    echo ""
    echo "Commands:"
    echo "  install-tools   Install required tools (proto-to-ddl, mysqldef)"
    echo "  check-tools     Check if required tools are installed"
    echo "  setup           Create necessary directories"
    echo "  proto2sql       Generate DDL from proto files and apply to database"
    echo "  entities        Generate Sea-ORM entities from proto files"
    echo "  proto2entities  Generate both DDL and entities from proto files"
    echo "  apply-ddl       Apply generated DDL to database"
    echo "  regen           Complete regeneration: proto → DDL+entities → DB"
    echo "  clean-dev       Drop all tables for fast development iteration"
    echo "  dev-reset       Full development reset: drop all tables + regenerate from proto"
    echo "  show-tables     Show all tables in the database"
    echo "  show-schema     Show schema for all tables"
    echo "  help            Show this help message"
    echo ""
    echo "Environment Variables:"
    echo "  DATABASE_URL    Database connection URL (REQUIRED)"
    echo "                  Format: mysql://user:pass@tcp(host:port)/dbname"
    echo ""
    echo "Examples:"
    echo "  $0 regen                                                           # Full regeneration with default DB"
    echo "  $0 clean-dev                                                       # Drop all tables"
    echo "  DATABASE_URL=mysql://user:pass@tcp(myhost.com:3306)/mydb $0 regen  # Use custom database"
    echo ""
    if [ -n "$DATABASE_URL" ]; then
        echo "Current DATABASE_URL: $DATABASE_URL"
    else
        echo "❌ DATABASE_URL is not set"
    fi
}

# Main command handling
case "${1:-help}" in
    "install-tools")
        install_tools
        ;;
    "check-tools")
        check_tools
        ;;
    "setup")
        setup_dirs
        ;;
    "proto2sql")
        check_database_url
        check_tools
        setup_dirs
        proto_to_sql
        ;;
    "entities")
        check_tools
        setup_dirs
        generate_entities
        ;;
    "proto2entities")
        check_tools
        setup_dirs
        proto2entities
        ;;
    "apply-ddl")
        check_database_url
        check_tools
        apply_ddl
        ;;
    "regen")
        regen
        ;;
    "clean-dev")
        clean_dev
        ;;
    "dev-reset")
        clean_dev
        regen
        ;;
    "show-tables")
        show_tables
        ;;
    "show-schema")
        show_schema
        ;;
    "help"|"--help"|"-h")
        show_help
        ;;
    *)
        log_error "Unknown command: $1"
        echo ""
        show_help
        exit 1
        ;;
esac 