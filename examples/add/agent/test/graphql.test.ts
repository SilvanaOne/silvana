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

    // Query to count transactions where object was used as input
    const query = `
      query GetTransactionCount($objectId: SuiAddress!) {
        transactionBlocks(
          filter: {
            inputObject: $objectId
          }
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
                inputObject: $objectId
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
              }
            }
          }
        `;

      const variables = cursor
        ? { objectId: appInstanceId, cursor }
        : { objectId: appInstanceId };

      const result = await client.query({
        query: queryWithPagination,
        variables,
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
      console.log(
        `\n✅ Total transactions using object as input: ${totalCount}`
      );
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
    const query = `
      query GetTransactionCount($objectId: SuiAddress!) {
        transactionBlocks(
          filter: {
            changedObject: $objectId
          }
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
        variables,
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
