#!/usr/bin/env python3
"""
Test script to understand proof structure for unused proof detection.
"""

import requests
import json

# Configuration
GRAPHQL_URL = "https://sui-devnet.mystenlabs.com/graphql"
EVENT_TYPE = "0x99faf01a34e3ff096befedc4ba05cf83949c07d259b08c31af56294eba9302c4::prover::ProofCalculationDeletedEvent"

# Fetch just one event to analyze
query = """
query GetEvents($eventType: String!, $first: Int!) {
    events(
        filter: {
            eventType: $eventType
        }
        first: $first
    ) {
        nodes {
            contents {
                json
            }
        }
    }
}
"""

variables = {
    "eventType": EVENT_TYPE,
    "first": 1
}

response = requests.post(
    GRAPHQL_URL,
    json={"query": query, "variables": variables},
    headers={"Content-Type": "application/json"},
    timeout=30
)

data = response.json()
events = data.get("data", {}).get("events", {}).get("nodes", [])

if events:
    event_json = events[0]["contents"]["json"]
    event_data = json.loads(event_json) if isinstance(event_json, str) else event_json
    
    print(f"Block #{event_data.get('block_number')}")
    print(f"Sequences: {event_data.get('start_sequence')} - {event_data.get('end_sequence')}")
    
    proofs = event_data.get("proofs", {}).get("contents", [])
    print(f"\nTotal proofs: {len(proofs)}")
    
    # Analyze proof structure
    leaf_proofs = []
    merge_proofs = []
    
    for proof in proofs:
        key = proof.get("key", [])
        value = proof.get("value", {})
        seq1 = value.get("sequence1")
        seq2 = value.get("sequence2")
        
        if seq1 is None and seq2 is None:
            leaf_proofs.append(key)
        else:
            merge_proofs.append({
                "key": key,
                "seq1": seq1,
                "seq2": seq2
            })
    
    print(f"\nLeaf proofs (no children): {len(leaf_proofs)}")
    print(f"Merge proofs (has children): {len(merge_proofs)}")
    
    # Find the block proof
    start = int(event_data.get('start_sequence'))
    end = int(event_data.get('end_sequence'))
    expected_sequences = end - start + 1
    
    block_proof_found = False
    for proof in proofs:
        key = proof.get("key", [])
        if len(key) == expected_sequences:
            block_proof_found = True
            print(f"\nBlock proof found with {len(key)} sequences")
            break
    
    if not block_proof_found:
        # Find the largest proof
        largest_key_len = 0
        for proof in proofs:
            key = proof.get("key", [])
            if len(key) > largest_key_len:
                largest_key_len = len(key)
        print(f"\nNo exact block proof found. Largest proof has {largest_key_len} sequences (expected {expected_sequences})")
    
    # Show some example proofs
    print("\nExample proofs:")
    for i, proof in enumerate(proofs[:5]):
        key = proof.get("key", [])
        value = proof.get("value", {})
        seq1 = value.get("sequence1")
        seq2 = value.get("sequence2")
        print(f"  Proof {i+1}: key={key}, seq1={seq1}, seq2={seq2}")
