#!/bin/bash
# Generate Ed25519 keypair and output the public key as hex string

# Generate Ed25519 private key
PRIVKEY=$(openssl genpkey -algorithm ed25519 2>/dev/null)

# Extract public key from private key
PUBKEY=$(echo "$PRIVKEY" | openssl pkey -pubout 2>/dev/null)

# Convert public key to DER format and extract the 32-byte public key
# Ed25519 public key in DER: last 32 bytes are the actual public key
PUBLIC_KEY_HEX=$(echo "$PUBKEY" | openssl pkey -pubin -outform DER 2>/dev/null | tail -c 32 | xxd -p -c 32)

# Output public key as hex string without 0x prefix (no newline)
# Forge's vm.ffi will interpret this as a hex string and return bytes
printf "%s" "${PUBLIC_KEY_HEX}"
