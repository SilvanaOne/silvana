#!/usr/bin/env python3
"""
Fetch transactions with ProofCalculationDeletedEvent for a specific object
and count the number of proofs in each block.
"""

import requests
import json
from typing import List, Dict, Optional

# Configuration
GRAPHQL_URL = "https://sui-devnet.mystenlabs.com/graphql"
OBJECT_ID = "0xf829a46b06494edb8d9b31d54be75db5e4af956d1f40d6ea81efd350bc50d861"
EVENT_TYPE = "0x99faf01a34e3ff096befedc4ba05cf83949c07d259b08c31af56294eba9302c4::prover::ProofCalculationDeletedEvent"
MAX_EVENTS = 1000  # Maximum events to fetch

def fetch_events_directly() -> List[Dict]:
    """
    Fetch events directly by event type.

    Returns:
        List of events with transaction details
    """
    events_list = []
    has_next_page = True
    cursor = None
    total_fetched = 0

    while has_next_page and total_fetched < MAX_EVENTS:
        # Build the GraphQL query to fetch events directly
        if cursor:
            query = """
            query GetEvents($eventType: String!, $cursor: String!, $first: Int!) {
                events(
                    filter: {
                        eventType: $eventType
                    }
                    first: $first
                    after: $cursor
                ) {
                    pageInfo {
                        hasNextPage
                        endCursor
                    }
                    nodes {
                        sendingModule {
                            package {
                                address
                            }
                            name
                        }
                        contents {
                            json
                            type {
                                repr
                            }
                        }
                        timestamp
                        transactionBlock {
                            digest
                            sender {
                                address
                            }
                        }
                    }
                }
            }
            """
            variables = {
                "eventType": EVENT_TYPE,
                "cursor": cursor,
                "first": 10  # Fetch 10 events at a time
            }
        else:
            query = """
            query GetEvents($eventType: String!, $first: Int!) {
                events(
                    filter: {
                        eventType: $eventType
                    }
                    first: $first
                ) {
                    pageInfo {
                        hasNextPage
                        endCursor
                    }
                    nodes {
                        sendingModule {
                            package {
                                address
                            }
                            name
                        }
                        contents {
                            json
                            type {
                                repr
                            }
                        }
                        timestamp
                        transactionBlock {
                            digest
                            sender {
                                address
                            }
                        }
                    }
                }
            }
            """
            variables = {
                "eventType": EVENT_TYPE,
                "first": 10
            }

        # Make the GraphQL request
        response = requests.post(
            GRAPHQL_URL,
            json={"query": query, "variables": variables},
            headers={"Content-Type": "application/json"},
            timeout=30
        )

        if response.status_code != 200:
            print(f"Error: HTTP {response.status_code}")
            print(response.text)
            break

        data = response.json()

        if "errors" in data:
            print("GraphQL errors:", data["errors"])
            # Try alternative query structure
            return fetch_events_alternative()

        events_connection = data.get("data", {}).get("events", {})
        nodes = events_connection.get("nodes", [])

        # Process events
        for event in nodes:
            contents = event.get("contents", {})
            event_json = contents.get("json")

            if event_json:
                try:
                    parsed_data = json.loads(event_json) if isinstance(event_json, str) else event_json
                    tx_block = event.get("transactionBlock", {})

                    # Check if this event is related to our object
                    if parsed_data.get("app_instance_id") == OBJECT_ID:
                        events_list.append({
                            "digest": tx_block.get("digest", "Unknown"),
                            "sender": tx_block.get("sender", {}).get("address", "Unknown"),
                            "timestamp": event.get("timestamp"),
                            "block_number": parsed_data.get("block_number"),
                            "app_instance_id": parsed_data.get("app_instance_id"),
                            "start_sequence": parsed_data.get("start_sequence"),
                            "end_sequence": parsed_data.get("end_sequence"),
                            "is_finished": parsed_data.get("is_finished"),
                            "proofs": parsed_data.get("proofs", {})
                        })
                except (json.JSONDecodeError, AttributeError) as e:
                    print(f"Failed to parse event data: {e}")

        total_fetched += len(nodes)
        print(f"Fetched {total_fetched} events...")

        # Check for next page
        page_info = events_connection.get("pageInfo", {})
        has_next_page = page_info.get("hasNextPage", False)
        cursor = page_info.get("endCursor")

        if not nodes:
            break

    return events_list

def fetch_events_alternative() -> List[Dict]:
    """
    Alternative approach: Query transactions that involve the object and filter for events.
    This is more targeted than fetching all transactions.
    """
    print("\nTrying alternative approach: Fetching transactions with specific event type...")

    events_list = []
    has_next_page = True
    cursor = None
    total_fetched = 0

    # First, get transactions that touch our object
    while has_next_page and total_fetched < 100:  # Limit to 100 transactions for now
        if cursor:
            query = """
            query GetTransactions($objectId: SuiAddress!, $cursor: String!, $first: Int!) {
                transactionBlocks(
                    filter: {
                        inputObject: $objectId
                    }
                    first: $first
                    after: $cursor
                ) {
                    pageInfo {
                        hasNextPage
                        endCursor
                    }
                    nodes {
                        digest
                        sender {
                            address
                        }
                        effects {
                            timestamp
                            events {
                                nodes {
                                    contents {
                                        json
                                        type {
                                            repr
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            """
            variables = {
                "objectId": OBJECT_ID,
                "cursor": cursor,
                "first": 10
            }
        else:
            query = """
            query GetTransactions($objectId: SuiAddress!, $first: Int!) {
                transactionBlocks(
                    filter: {
                        inputObject: $objectId
                    }
                    first: $first
                ) {
                    pageInfo {
                        hasNextPage
                        endCursor
                    }
                    nodes {
                        digest
                        sender {
                            address
                        }
                        effects {
                            timestamp
                            events {
                                nodes {
                                    contents {
                                        json
                                        type {
                                            repr
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            """
            variables = {
                "objectId": OBJECT_ID,
                "first": 10
            }

        response = requests.post(
            GRAPHQL_URL,
            json={"query": query, "variables": variables},
            headers={"Content-Type": "application/json"},
            timeout=30
        )

        if response.status_code != 200:
            print(f"Error: HTTP {response.status_code}")
            break

        data = response.json()

        if "errors" in data:
            print("GraphQL errors:", data["errors"])
            break

        transaction_blocks = data.get("data", {}).get("transactionBlocks", {})
        nodes = transaction_blocks.get("nodes", [])

        # Process transactions
        for tx in nodes:
            digest = tx.get("digest")
            sender = tx.get("sender", {}).get("address", "Unknown")
            effects = tx.get("effects", {})
            timestamp = effects.get("timestamp")
            events = effects.get("events", {}).get("nodes", []) if effects else []

            # Check events in this transaction
            for event in events:
                contents = event.get("contents", {})
                event_type = contents.get("type", {}).get("repr", "") if contents else ""

                # Check if this is our ProofCalculationDeletedEvent
                if event_type == EVENT_TYPE:
                    event_json = contents.get("json")
                    if event_json:
                        try:
                            parsed_data = json.loads(event_json) if isinstance(event_json, str) else event_json
                            events_list.append({
                                "digest": digest,
                                "sender": sender,
                                "timestamp": timestamp,
                                "block_number": parsed_data.get("block_number"),
                                "app_instance_id": parsed_data.get("app_instance_id"),
                                "start_sequence": parsed_data.get("start_sequence"),
                                "end_sequence": parsed_data.get("end_sequence"),
                                "is_finished": parsed_data.get("is_finished"),
                                "proofs": parsed_data.get("proofs", {})
                            })
                            print(f"Found ProofCalculationDeletedEvent in tx {digest}")
                        except (json.JSONDecodeError, AttributeError) as e:
                            print(f"Failed to parse event data: {e}")

        total_fetched += len(nodes)

        # Check for next page
        page_info = transaction_blocks.get("pageInfo", {})
        has_next_page = page_info.get("hasNextPage", False)
        cursor = page_info.get("endCursor")

        if not nodes:
            break

    return events_list

def count_proofs_in_event(proofs_data: Dict) -> int:
    """
    Count the number of proofs in the event data.

    Args:
        proofs_data: The proofs field from the event

    Returns:
        Number of proofs
    """
    if not proofs_data or not isinstance(proofs_data, dict):
        return 0

    contents = proofs_data.get("contents", [])
    return len(contents) if isinstance(contents, list) else 0

def count_used_and_unused_proofs(event: Dict) -> tuple:
    """
    Count used and unused proofs by tracking the merkle tree structure.

    Args:
        event: Event data containing proofs

    Returns:
        Tuple of (total_proofs, used_proofs, unused_proofs)
    """
    proofs_data = event.get("proofs", {})
    if not proofs_data or not isinstance(proofs_data, dict):
        return 0, 0, 0

    contents = proofs_data.get("contents", [])
    if not contents or not isinstance(contents, list):
        return 0, 0, 0

    # Build proof index for quick lookup
    proof_index = {}
    for proof in contents:
        if isinstance(proof, dict) and "key" in proof:
            key = proof["key"]
            # Convert key to tuple for use as dictionary key
            key_tuple = tuple(key) if isinstance(key, list) else (key,)
            proof_index[key_tuple] = proof

    # Track used proofs
    used_proofs = set()

    # Find the block proof (covers all sequences from start to end)
    start_seq = int(event.get("start_sequence", "0"))
    end_seq = int(event.get("end_sequence", "0"))

    # Create list of all sequences in the block
    all_sequences = [str(i) for i in range(start_seq, end_seq + 1)]
    block_proof_key = tuple(all_sequences)

    # Find the block proof or the largest proof that might be the block proof
    if block_proof_key in proof_index:
        # Start recursive marking from the block proof
        mark_proof_as_used(block_proof_key, proof_index, used_proofs)
    else:
        # If exact match not found, find the proof with the most sequences
        # This handles cases where the proof structure might be different
        largest_proof_key = None
        largest_size = 0
        for key_tuple in proof_index.keys():
            if len(key_tuple) > largest_size:
                largest_size = len(key_tuple)
                largest_proof_key = key_tuple

        if largest_proof_key:
            mark_proof_as_used(largest_proof_key, proof_index, used_proofs)

    total_proofs = len(contents)
    used_count = len(used_proofs)
    unused_count = total_proofs - used_count

    return total_proofs, used_count, unused_count

def mark_proof_as_used(key_tuple: tuple, proof_index: Dict, used_proofs: set):
    """
    Recursively mark a proof and its dependencies as used.

    Args:
        key_tuple: The key of the proof to mark as used
        proof_index: Dictionary mapping keys to proofs
        used_proofs: Set of used proof keys
    """
    # If already marked, skip
    if key_tuple in used_proofs:
        return

    # Mark this proof as used
    used_proofs.add(key_tuple)

    # Get the proof
    proof = proof_index.get(key_tuple)
    if not proof:
        return

    # Process dependencies (sequence1 and sequence2)
    value = proof.get("value", {})
    if isinstance(value, dict):
        seq1 = value.get("sequence1")
        seq2 = value.get("sequence2")

        # Recursively mark sequence1 proof as used
        if seq1:
            seq1_tuple = tuple(seq1) if isinstance(seq1, list) else (seq1,)
            if seq1_tuple in proof_index:
                mark_proof_as_used(seq1_tuple, proof_index, used_proofs)

        # Recursively mark sequence2 proof as used
        if seq2:
            seq2_tuple = tuple(seq2) if isinstance(seq2, list) else (seq2,)
            if seq2_tuple in proof_index:
                mark_proof_as_used(seq2_tuple, proof_index, used_proofs)

def main():
    print(f"Fetching ProofCalculationDeletedEvent for object: {OBJECT_ID}")
    print(f"Event type: {EVENT_TYPE}")
    print(f"GraphQL endpoint: {GRAPHQL_URL}\n")

    # Fetch events
    print("Fetching events...")
    events = fetch_events_directly()

    if not events:
        print("\nNo ProofCalculationDeletedEvent found.")
        return

    print(f"\nTotal ProofCalculationDeletedEvent found: {len(events)}")
    print("\n" + "=" * 100)
    print("PROOF CALCULATION DELETED EVENTS")
    print("=" * 100)

    # Sort by block number
    sorted_events = sorted(events, key=lambda x: int(x.get("block_number", "0")))

    total_proofs = 0
    total_used = 0
    total_unused = 0

    for event in sorted_events:
        proof_count = count_proofs_in_event(event.get("proofs", {}))
        total_proofs += proof_count

        # Calculate used and unused proofs
        total, used, unused = count_used_and_unused_proofs(event)
        total_used += used
        total_unused += unused

        # Calculate percentage
        unused_percent = (unused / total * 100) if total > 0 else 0

        print(f"\nBlock #{event.get('block_number', 'Unknown')}")
        print(f"  Transaction: {event.get('digest', 'Unknown')}")
        print(f"  Sender: {event.get('sender', 'Unknown')}")
        print(f"  App Instance: {event.get('app_instance_id', 'Unknown')}")
        print(f"  Sequence Range: {event.get('start_sequence', '?')} - {event.get('end_sequence', '?')}")
        print(f"  Is Finished: {event.get('is_finished', False)}")
        print(f"  Timestamp: {event.get('timestamp', 'Unknown')}")
        print(f"  Total Proofs: {proof_count}")
        print(f"  Used Proofs: {used}")
        print(f"  Unused Proofs: {unused} ({unused_percent:.1f}%)")

        # Show proof keys if available
        if event.get("proofs") and isinstance(event["proofs"], dict):
            contents = event["proofs"].get("contents", [])
            if contents and isinstance(contents, list):
                proof_keys = []
                for proof in contents[:5]:  # Show first 5 proof keys
                    if isinstance(proof, dict) and "key" in proof:
                        proof_keys.append(str(proof["key"]))
                if proof_keys:
                    print(f"  Proof Keys (first 5): {', '.join(proof_keys)}")
                    if len(contents) > 5:
                        print(f"  ... and {len(contents) - 5} more")

    # Summary statistics
    print("\n" + "=" * 100)
    print("SUMMARY")
    print("=" * 100)
    print(f"Total ProofCalculationDeletedEvent: {len(events)}")
    print(f"Total number of proofs across all events: {total_proofs}")
    print(f"Total used proofs: {total_used}")
    print(f"Total unused proofs: {total_unused}")
    if total_proofs > 0:
        print(f"Overall unused percentage: {(total_unused / total_proofs * 100):.1f}%")

    if events:
        avg_proofs = total_proofs / len(events)
        print(f"Average proofs per event: {avg_proofs:.2f}")

        # Find min and max
        proof_counts = [count_proofs_in_event(e.get("proofs", {})) for e in events]
        print(f"Minimum proofs in an event: {min(proof_counts)}")
        print(f"Maximum proofs in an event: {max(proof_counts)}")

        # Group by block with unused proof stats
        blocks = {}
        for event in events:
            block_num = event.get("block_number", "Unknown")
            if block_num not in blocks:
                blocks[block_num] = {"events": 0, "total": 0, "used": 0, "unused": 0}

            total, used, unused = count_used_and_unused_proofs(event)
            blocks[block_num]["events"] += 1
            blocks[block_num]["total"] += total
            blocks[block_num]["used"] += used
            blocks[block_num]["unused"] += unused

        print(f"\nBlocks with events: {len(blocks)}")
        for block_num in sorted(blocks.keys(), key=lambda x: int(x) if x != "Unknown" else -1):
            block_data = blocks[block_num]
            unused_pct = (block_data["unused"] / block_data["total"] * 100) if block_data["total"] > 0 else 0
            print(f"  Block #{block_num}: {block_data['events']} event(s), "
                  f"{block_data['total']} total proofs, "
                  f"{block_data['used']} used, "
                  f"{block_data['unused']} unused ({unused_pct:.1f}%)")

if __name__ == "__main__":
    main()