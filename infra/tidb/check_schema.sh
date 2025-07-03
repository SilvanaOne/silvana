#!/bin/bash

# Quick schema validation check
# This script provides a fast way to check if your proto-generated entities
# match the actual TiDB database schema

set -e

echo "🔍 Quick Schema Validation Check"
echo "================================"
echo ""

# Check if DATABASE_URL is set
if [ -z "$DATABASE_URL" ]; then
    echo "❌ ERROR: DATABASE_URL environment variable is not set"
    echo "Please set it like: export DATABASE_URL='mysql://user:password@host:port/database'"
    exit 1
fi

echo "📊 Database: $(echo $DATABASE_URL | sed 's|mysql://[^:]*:[^@]*@|mysql://***:***@|')"
echo ""

# Check if proto-to-ddl tool exists
if [ ! -f "infra/tidb/proto-to-ddl/target/release/proto-to-ddl" ]; then
    echo "❌ ERROR: proto-to-ddl tool not found"
    echo "💡 Run 'make install-tools' first"
    exit 1
fi

# Run the validation using proto-to-ddl
if ./infra/tidb/proto-to-ddl/target/release/proto-to-ddl validate \
    --proto-file proto/events.proto \
    --database-url "$DATABASE_URL" >/dev/null 2>&1; then
    echo "🎉 All schemas are valid!"
    echo "✅ Proto definitions match database structure perfectly."
    exit 0
else
    echo ""
    echo "⚠️  Schema validation failed."
    echo "💡 Run 'make validate-schema' for detailed information"
    echo "🔧 Run 'make regen' to fix discrepancies"
    exit 1
fi 