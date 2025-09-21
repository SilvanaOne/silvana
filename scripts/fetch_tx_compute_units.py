#!/usr/bin/env python3
"""
Fetch transactions containing a specific object and sort by compute units.
"""

import requests
import json
from typing import List, Dict, Optional, Tuple

# Configuration
# Using the standard Sui devnet GraphQL endpoint
GRAPHQL_URL = "https://sui-devnet.mystenlabs.com/graphql"
OBJECT_ID = "0xf829a46b06494edb8d9b31d54be75db5e4af956d1f40d6ea81efd350bc50d861"
MAX_TRANSACTIONS = 1_000_000_000  # For testing, fetch only 100 transactions

def fetch_transactions(object_id: str, limit: int = 100) -> List[Dict]:
    """
    Fetch transactions that use the specified object as input.

    Args:
        object_id: The Sui object ID to search for
        limit: Maximum number of transactions to fetch

    Returns:
        List of transaction data with compute units
    """
    transactions = []
    has_next_page = True
    cursor = None
    total_fetched = 0

    while has_next_page and total_fetched < limit:
        # Determine how many to fetch in this batch (max 10 per GraphQL limit)
        batch_size = min(10, limit - total_fetched)

        # Build the GraphQL query
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
                            gasEffects {
                                gasSummary {
                                    computationCost
                                    storageCost
                                    storageRebate
                                    nonRefundableStorageFee
                                }
                            }
                        }
                    }
                }
            }
            """
            variables = {
                "objectId": object_id,
                "cursor": cursor,
                "first": batch_size
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
                            gasEffects {
                                gasSummary {
                                    computationCost
                                    storageCost
                                    storageRebate
                                    nonRefundableStorageFee
                                }
                            }
                        }
                    }
                }
            }
            """
            variables = {
                "objectId": object_id,
                "first": batch_size
            }

        # Make the GraphQL request
        response = requests.post(
            GRAPHQL_URL,
            json={"query": query, "variables": variables},
            headers={"Content-Type": "application/json"}
        )

        if response.status_code != 200:
            print(f"Error: HTTP {response.status_code}")
            print(response.text)
            break

        data = response.json()

        if "errors" in data:
            print("GraphQL errors:", data["errors"])
            break

        transaction_blocks = data.get("data", {}).get("transactionBlocks", {})
        nodes = transaction_blocks.get("nodes", [])

        # Process transactions
        for tx in nodes:
            gas_summary = tx.get("effects", {}).get("gasEffects", {}).get("gasSummary", {})
            if gas_summary and tx.get("digest"):
                transactions.append({
                    "digest": tx["digest"],
                    "sender": tx.get("sender", {}).get("address", "Unknown"),
                    "computationCost": int(gas_summary.get("computationCost", 0)),
                    "storageCost": int(gas_summary.get("storageCost", 0)),
                    "storageRebate": int(gas_summary.get("storageRebate", 0)),
                    "nonRefundableStorageFee": int(gas_summary.get("nonRefundableStorageFee", 0))
                })

        total_fetched += len(nodes)

        # Print progress every 1000 transactions
        if total_fetched % 1000 == 0 or total_fetched == len(nodes):
            print(f"Fetched {total_fetched} transactions...")

        # Check for next page
        page_info = transaction_blocks.get("pageInfo", {})
        has_next_page = page_info.get("hasNextPage", False)
        cursor = page_info.get("endCursor")

        if not nodes:
            break

    return transactions

def mist_to_sui(mist: int) -> float:
    """Convert MIST to SUI (1 SUI = 1,000,000,000 MIST)"""
    return mist / 1_000_000_000

def calculate_net_gas(tx: Dict) -> int:
    """Calculate net gas cost for a transaction"""
    return tx["computationCost"] + tx["storageCost"] - tx["storageRebate"]

def main():
    print(f"Fetching transactions for object: {OBJECT_ID}")
    print(f"GraphQL endpoint: {GRAPHQL_URL}")
    print(f"Maximum transactions to fetch: {MAX_TRANSACTIONS}\n")

    # Fetch transactions
    transactions = fetch_transactions(OBJECT_ID, MAX_TRANSACTIONS)

    if not transactions:
        print("No transactions found.")
        return

    print(f"\nTotal transactions fetched: {len(transactions)}")

    # Sort by computation cost (descending)
    sorted_txs = sorted(transactions, key=lambda x: x["computationCost"], reverse=True)

    # Display top 10 most expensive transactions
    print("\n" + "=" * 80)
    print("TOP 10 MOST EXPENSIVE TRANSACTIONS BY COMPUTE UNITS")
    print("=" * 80)

    for i, tx in enumerate(sorted_txs[:10], 1):
        net_gas = calculate_net_gas(tx)
        print(f"\n{i}. Transaction: {tx['digest']}")
        print(f"   Sender: {tx['sender']}")
        print(f"   Computation Cost: {tx['computationCost']:,} MIST ({mist_to_sui(tx['computationCost']):.9f} SUI)")
        print(f"   Storage Cost: {tx['storageCost']:,} MIST ({mist_to_sui(tx['storageCost']):.9f} SUI)")
        print(f"   Storage Rebate: {tx['storageRebate']:,} MIST ({mist_to_sui(tx['storageRebate']):.9f} SUI)")
        print(f"   Non-refundable Fee: {tx['nonRefundableStorageFee']:,} MIST ({mist_to_sui(tx['nonRefundableStorageFee']):.9f} SUI)")
        print(f"   Net Gas Cost: {net_gas:,} MIST ({mist_to_sui(net_gas):.9f} SUI)")

    # Calculate and display statistics
    print("\n" + "=" * 80)
    print("STATISTICS")
    print("=" * 80)

    total_compute = sum(tx["computationCost"] for tx in transactions)
    avg_compute = total_compute // len(transactions) if transactions else 0

    print(f"Total transactions analyzed: {len(transactions)}")
    print(f"Total computation cost: {total_compute:,} MIST ({mist_to_sui(total_compute):.9f} SUI)")
    print(f"Average computation cost: {avg_compute:,} MIST ({mist_to_sui(avg_compute):.9f} SUI)")

    # Find min and max
    if transactions:
        min_tx = min(transactions, key=lambda x: x["computationCost"])
        max_tx = max(transactions, key=lambda x: x["computationCost"])
        print(f"Minimum computation cost: {min_tx['computationCost']:,} MIST")
        print(f"Maximum computation cost: {max_tx['computationCost']:,} MIST")

if __name__ == "__main__":
    main()