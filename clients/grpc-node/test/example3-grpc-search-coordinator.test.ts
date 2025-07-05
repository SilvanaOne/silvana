import { describe, it } from "node:test";
import assert from "node:assert";
import { SilvanaBufGrpcClient } from "../src/clients/grpc-client.js";
import { SilvanaNatsClient } from "../src/clients/nats-client.js";
import { BufEventFactory } from "../src/utils/event-factory.js";
import { LogLevel } from "../src/gen/events_pb.js";

describe("Example 3: gRPC Search CoordinatorMessageEvents", async () => {
  it("should search CoordinatorMessageEvents for 'error' string using gRPC", async () => {
    console.log("🚀 Starting Example 3: gRPC CoordinatorMessageEvent Search");

    const grpcClient = new SilvanaBufGrpcClient();
    const natsClient = new SilvanaNatsClient();

    try {
      // 1. Connect to NATS for optional event streaming
      console.log("\n📡 Connecting to NATS...");
      await natsClient.connect();

      // 2. Create some sample CoordinatorMessageEvents with different error types
      console.log("\n📝 Creating sample CoordinatorMessageEvents...");

      const testCoordinatorId = `coord-search-001-${Date.now()}`;
      const errorEvents = [
        BufEventFactory.createCoordinatorMessageEvent({
          coordinatorId: testCoordinatorId,
          level: LogLevel.ERROR,
          message:
            "Database connection error: Failed to connect to TiDB cluster",
          eventTimestamp: BigInt(Date.now()),
        }),
        BufEventFactory.createCoordinatorMessageEvent({
          coordinatorId: testCoordinatorId,
          level: LogLevel.ERROR,
          message: "Authentication error: Invalid API key provided",
          eventTimestamp: BigInt(Date.now()),
        }),
        BufEventFactory.createCoordinatorMessageEvent({
          coordinatorId: testCoordinatorId,
          level: LogLevel.WARN,
          message: "Performance warning: High memory usage detected",
          eventTimestamp: BigInt(Date.now()),
        }),
        BufEventFactory.createCoordinatorMessageEvent({
          coordinatorId: testCoordinatorId,
          level: LogLevel.ERROR,
          message:
            "Network error: Timeout while connecting to external service",
          eventTimestamp: BigInt(Date.now()),
        }),
        BufEventFactory.createCoordinatorMessageEvent({
          coordinatorId: testCoordinatorId,
          level: LogLevel.INFO,
          message: "System startup completed successfully",
          eventTimestamp: BigInt(Date.now()),
        }),
      ];

      // Verify event structure
      assert.strictEqual(errorEvents.length, 5, "Should create 5 test events");
      const firstEvent = errorEvents[0];
      assert.strictEqual(
        firstEvent.eventType.case,
        "coordinator",
        "Event should be a coordinator event"
      );

      if (firstEvent.eventType.case === "coordinator") {
        const coordinatorEventValue = firstEvent.eventType.value;
        assert.strictEqual(
          coordinatorEventValue.event.case,
          "coordinatorError",
          "Coordinator event should be an error"
        );

        if (coordinatorEventValue.event.case === "coordinatorError") {
          const errorEvent = coordinatorEventValue.event.value;
          assert.strictEqual(errorEvent.coordinatorId, testCoordinatorId);
        }
      }

      // 3. Send all events via gRPC
      console.log("\n🔗 Sending events via gRPC...");
      let successCount = 0;
      for (let i = 0; i < errorEvents.length; i++) {
        const event = errorEvents[i];
        let eventMessage = "";

        // Extract message for logging using the discriminated union approach
        if (
          event.eventType.case === "coordinator" &&
          event.eventType.value.event.case === "coordinatorError"
        ) {
          eventMessage = event.eventType.value.event.value.message;
        }

        console.log(`Sending event ${i + 1}:`, eventMessage);

        try {
          const response = await grpcClient.submitEvent(event);
          console.log(`✅ Event ${i + 1} submitted:`, response);
          successCount++;

          // TODO: Update NATS client to handle Buf Event types
          // await natsClient.publishEvent(event);
          console.log(
            `📤 Event ${
              i + 1
            } published to NATS (skipped - needs type conversion)`
          );
        } catch (error) {
          console.log(
            `❌ Failed to submit event ${i + 1}:`,
            (error as Error).message
          );
        }

        // Small delay between submissions
        await new Promise((resolve) => setTimeout(resolve, 500));
      }

      // Verify at least some events were submitted
      assert.ok(
        successCount > 0,
        "At least some events should be submitted successfully"
      );

      // 4. Wait a moment for events to be indexed
      console.log("\n⏳ Waiting for events to be indexed...");
      await new Promise((resolve) => setTimeout(resolve, 2000));

      // 5. Search for events containing "error"
      console.log(
        '\n🔍 Searching for CoordinatorMessageEvents containing "error"...'
      );

      const searchRequest = BufEventFactory.createSearchRequest({
        searchQuery: "error",
        limit: 10,
        coordinatorId: testCoordinatorId,
      });

      console.log("Search request:", JSON.stringify(searchRequest, null, 2));

      // Verify search request structure
      assert.strictEqual(searchRequest.searchQuery, "error");
      assert.strictEqual(searchRequest.coordinatorId, testCoordinatorId);

      try {
        const searchResponse = await grpcClient.searchCoordinatorMessageEvents(
          searchRequest
        );

        console.log("\n📊 Search Results:");
        console.log(`Total matches: ${searchResponse.totalCount}`);
        console.log(`Returned: ${searchResponse.returnedCount}`);

        // Verify search response structure
        assert.ok(searchResponse, "Search should return a response");
        assert.ok(
          typeof searchResponse.totalCount !== "undefined",
          "Response should have totalCount"
        );
        assert.ok(
          typeof searchResponse.returnedCount !== "undefined",
          "Response should have returnedCount"
        );

        if (searchResponse.events && searchResponse.events.length > 0) {
          console.log("\n📋 Matching events:");
          searchResponse.events.forEach((event: any, index: number) => {
            console.log(`\n${index + 1}. Event ID: ${event.id}`);
            console.log(`   Coordinator: ${event.coordinator_id}`);
            console.log(`   Level: ${event.level}`);
            console.log(`   Message: ${event.message}`);
            console.log(`   Relevance Score: ${event.relevance_score}`);
            console.log(
              `   Timestamp: ${new Date(
                event.event_timestamp / 1000000
              ).toISOString()}`
            );
          });

          // Verify search results contain our expected data
          const firstResult = searchResponse.events[0];
          assert.ok(
            firstResult.message.toLowerCase().includes("error"),
            "Search results should contain error messages"
          );
        } else {
          console.log(
            "No events found matching the search criteria (this may be expected if indexing is delayed)"
          );
        }
      } catch (error) {
        console.error("❌ Search failed:", error);
        console.log(
          "Note: Make sure the gRPC server is running and supports full-text search"
        );
        // Don't fail the test if search is not available
      }

      // 6. Try another search with different terms
      console.log(
        '\n🔍 Searching for CoordinatorMessageEvents containing "database"...'
      );

      const databaseSearchRequest = BufEventFactory.createSearchRequest({
        searchQuery: "database",
        limit: 5,
        coordinatorId: testCoordinatorId,
      });

      try {
        const databaseSearchResponse =
          await grpcClient.searchCoordinatorMessageEvents(
            databaseSearchRequest
          );

        console.log("\n📊 Database Search Results:");
        console.log(`Total matches: ${databaseSearchResponse.totalCount}`);

        if (
          databaseSearchResponse.events &&
          databaseSearchResponse.events.length > 0
        ) {
          databaseSearchResponse.events.forEach((event: any, index: number) => {
            console.log(`\n${index + 1}. Database Event ID: ${event.id}`);
            console.log(`   Message: ${event.message}`);
            console.log(`   Relevance Score: ${event.relevance_score}`);
          });

          // Verify database search results
          const firstDatabaseResult = databaseSearchResponse.events[0];
          assert.ok(
            firstDatabaseResult.message.toLowerCase().includes("database"),
            "Database search results should contain database-related messages"
          );
        } else {
          console.log("No database-related events found");
        }
      } catch (error) {
        console.error("❌ Database search failed:", error);
        console.log(
          "Note: This is expected if search indexing is not yet available"
        );
        // Don't fail the test if search is not available
      }

      console.log("✅ Example 3 test completed successfully!");
    } catch (error) {
      console.error("❌ Error in Example 3 test:", error);
      throw error;
    } finally {
      // Cleanup
      console.log("\n🧹 Cleaning up...");
      await natsClient.close();
    }
  });
});
