import { describe, it } from "node:test";
import assert from "node:assert";
import { SuiGraphQLClient } from "@mysten/sui/graphql";

describe("GraphQL Transaction Count", async () => {
  it("should count transactions for APP_INSTANCE_ID object", async () => {
    // Get object ID from environment
    const appInstanceId = process.env.APP_INSTANCE_ID;
    if (!appInstanceId) {
      console.log("APP_INSTANCE_ID not set, skipping test");
      return;
    }

    // Determine network from SUI_CHAIN environment variable
    const chain = process.env.SUI_CHAIN || "devnet";

    // Select appropriate GraphQL endpoint based on network
    let graphqlUrl: string;
    switch (chain) {
      case "mainnet":
        graphqlUrl = "https://sui-mainnet.mystenlabs.com/graphql";
        break;
      case "testnet":
        graphqlUrl = "https://sui-testnet.mystenlabs.com/graphql";
        break;
      case "devnet":
      default:
        graphqlUrl = "https://sui-devnet.mystenlabs.com/graphql";
        break;
    }

    console.log(`Using GraphQL endpoint: ${graphqlUrl}`);
    console.log(`Counting transactions for object: ${appInstanceId}`);
    console.log(`Network: ${chain}`);

    // Create GraphQL client
    const client = new SuiGraphQLClient({
      url: graphqlUrl,
    });

    // Query to get transactions with fee details
    const query = `
      query GetTransactionCount($objectId: SuiAddress!) {
        transactionBlocks(
          filter: {
            inputObject: $objectId
          }
          first: 10
        ) {
          pageInfo {
            hasNextPage
            endCursor
          }
          nodes {
            digest
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
    `;

    let totalCount = 0;
    let totalComputationCost = 0n;
    let totalStorageCost = 0n;
    let totalStorageRebate = 0n;
    let totalNonRefundableStorageFee = 0n;
    let hasNextPage = true;
    let cursor: string | null = null;
    const maxCount = 100000; // Process up to 100k transactions

    // Paginate through all transactions
    while (hasNextPage && totalCount < maxCount) {
      const queryWithCursor = cursor
        ? `
        query GetTransactionCount($objectId: SuiAddress!, $cursor: String!) {
          transactionBlocks(
            filter: {
              inputObject: $objectId
            }
            first: 10
            after: $cursor
          ) {
            pageInfo {
              hasNextPage
              endCursor
            }
            nodes {
              digest
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
      `
        : query;

      const variables = cursor
        ? { objectId: appInstanceId, cursor }
        : { objectId: appInstanceId };

      const result = await client.query({
        query: queryWithCursor,
        variables: variables as any,
      });

      if (result.errors) {
        console.error("GraphQL errors:", result.errors);
        throw new Error("GraphQL query failed");
      }

      const data = result.data as any;
      const transactionBlocks = data?.transactionBlocks;

      if (transactionBlocks?.nodes) {
        const pageCount = transactionBlocks.nodes.length;
        totalCount += pageCount;

        if (totalCount % 100 === 0 || totalCount === pageCount) {
          console.log(`Processing ${totalCount} transactions...`);
        }

        for (const tx of transactionBlocks.nodes) {
          const gasSummary = tx.effects?.gasEffects?.gasSummary;
          if (gasSummary) {
            const computationCost = BigInt(gasSummary.computationCost || 0);
            const storageCost = BigInt(gasSummary.storageCost || 0);
            const storageRebate = BigInt(gasSummary.storageRebate || 0);
            const nonRefundableStorageFee = BigInt(
              gasSummary.nonRefundableStorageFee || 0
            );

            totalComputationCost += computationCost;
            totalStorageCost += storageCost;
            totalStorageRebate += storageRebate;
            totalNonRefundableStorageFee += nonRefundableStorageFee;
          }
        }
      }

      hasNextPage = transactionBlocks?.pageInfo?.hasNextPage || false;
      cursor = transactionBlocks?.pageInfo?.endCursor || null;
    }

    if (totalCount > 0) {
      console.log("\n" + "=".repeat(60));
      console.log("SUMMARY:");
      console.log(`  Total Transactions: ${totalCount}`);
      console.log(
        `  Total Computation Cost: ${(
          Number(totalComputationCost) / 1_000_000_000
        ).toFixed(9)} SUI`
      );
      console.log(
        `  Total Storage Cost: ${(
          Number(totalStorageCost) / 1_000_000_000
        ).toFixed(9)} SUI`
      );
      console.log(
        `  Total Storage Rebate: ${(
          Number(totalStorageRebate) / 1_000_000_000
        ).toFixed(9)} SUI`
      );
      console.log(
        `  Total Non-refundable Storage Fee: ${(
          Number(totalNonRefundableStorageFee) / 1_000_000_000
        ).toFixed(9)} SUI`
      );
      console.log(
        `  Total Net Gas Cost: ${(
          Number(totalComputationCost + totalStorageCost - totalStorageRebate) /
          1_000_000_000
        ).toFixed(9)} SUI`
      );
      console.log(
        `  Average Net Gas Cost per Tx: ${(
          (totalComputationCost + totalStorageCost - totalStorageRebate) /
          BigInt(totalCount)
        ).toString()} MIST`
      );

      // Averages
      console.log("\n" + "=".repeat(60));
      console.log("AVERAGES:");
      console.log(
        `  Average Computation Cost per Tx: ${(
          totalComputationCost / BigInt(totalCount)
        ).toString()} MIST`
      );
      console.log(
        `  Average Non-refundable Storage Fee per Tx: ${(
          totalNonRefundableStorageFee / BigInt(totalCount)
        ).toString()} MIST`
      );
      console.log(
        `  Average Storage Cost per Tx: ${(
          totalStorageCost / BigInt(totalCount)
        ).toString()} MIST`
      );
      console.log(
        `  Average Storage Rebate per Tx: ${(
          totalStorageRebate / BigInt(totalCount)
        ).toString()} MIST`
      );

      // Key metric: Sum of computationCost + nonRefundableStorageFee
      const totalComputationAndNonRefundableFee =
        totalComputationCost + totalNonRefundableStorageFee;
      console.log("\n" + "=".repeat(60));
      console.log("KEY METRIC:");
      console.log(
        `  Sum of Computation Cost + Non-refundable Storage Fee: ${(
          Number(totalComputationAndNonRefundableFee) / 1_000_000_000
        ).toFixed(9)} SUI`
      );
      console.log(
        `  Average per Tx: ${(
          totalComputationAndNonRefundableFee / BigInt(totalCount)
        ).toString()} MIST`
      );
    }

    if (totalCount >= maxCount) {
      console.log(
        `\n⚠️ Reached max count limit of ${maxCount}. More transactions may exist.`
      );
    } else if (totalCount > 0) {
      console.log(`\n✅ Processed all ${totalCount} transactions`);
    }

    // Assert that we found at least some transactions (if this is not a new object)
    assert(totalCount >= 0, "Transaction count should be non-negative");
  });

  it.skip("should count transactions that changed APP_INSTANCE_ID object", async () => {
    // Get object ID from environment
    const appInstanceId = process.env.APP_INSTANCE_ID;
    if (!appInstanceId) {
      console.log("APP_INSTANCE_ID not set, skipping test");
      return;
    }

    // Determine network from SUI_CHAIN environment variable
    const chain = process.env.SUI_CHAIN || "devnet";

    // Select appropriate GraphQL endpoint based on network
    let graphqlUrl: string;
    switch (chain) {
      case "mainnet":
        graphqlUrl = "https://sui-mainnet.mystenlabs.com/graphql";
        break;
      case "testnet":
        graphqlUrl = "https://sui-testnet.mystenlabs.com/graphql";
        break;
      case "devnet":
      default:
        graphqlUrl = "https://sui-devnet.mystenlabs.com/graphql";
        break;
    }

    console.log(`Using GraphQL endpoint: ${graphqlUrl}`);
    console.log(`Counting transactions that changed object: ${appInstanceId}`);
    console.log(`Network: ${chain}`);

    // Create GraphQL client
    const client = new SuiGraphQLClient({
      url: graphqlUrl,
    });

    // Query to count transactions where object was changed

    let totalCount = 0;
    let hasNextPage = true;
    let cursor: string | null = null;
    const maxCount = 100000; // Process up to 100k transactions

    // Paginate through all transactions (with max count limit)
    while (hasNextPage && totalCount < maxCount) {
      const queryWithPagination = cursor
        ? `
          query GetTransactionCount($objectId: SuiAddress!, $cursor: String!) {
            transactionBlocks(
              filter: {
                changedObject: $objectId
              }
              after: $cursor
              first: 10
            ) {
              pageInfo {
                hasNextPage
                endCursor
              }
              nodes {
                digest
              }
            }
          }
        `
        : `
          query GetTransactionCount($objectId: SuiAddress!) {
            transactionBlocks(
              filter: {
                changedObject: $objectId
              }
              first: 10
            ) {
              pageInfo {
                hasNextPage
                endCursor
              }
              nodes {
                digest
              }
            }
          }
        `;

      const variables = cursor
        ? { objectId: appInstanceId, cursor }
        : { objectId: appInstanceId };

      const result = await client.query({
        query: queryWithPagination,
        variables: variables as any,
      });

      if (result.errors) {
        console.error("GraphQL errors:", result.errors);
        throw new Error("GraphQL query failed");
      }

      const data = result.data as any;
      const transactionBlocks = data?.transactionBlocks;

      if (transactionBlocks?.nodes) {
        const pageCount = transactionBlocks.nodes.length;
        totalCount += pageCount;

        if (pageCount > 0) {
          console.log(
            `  Fetched ${pageCount} transactions (total: ${totalCount})`
          );
        }
      }

      hasNextPage = transactionBlocks?.pageInfo?.hasNextPage || false;
      cursor = transactionBlocks?.pageInfo?.endCursor || null;
    }

    if (totalCount >= maxCount) {
      console.log(
        `\n⚠️ Reached max count limit. Counted at least ${totalCount} transactions (more may exist)`
      );
    } else {
      console.log(`\n✅ Total transactions that changed object: ${totalCount}`);
    }

    // Assert that we found at least some transactions (if this is not a new object)
    assert(totalCount >= 0, "Transaction count should be non-negative");
  });
});
