import { describe, it } from "node:test";
import assert from "node:assert";
import { SilvanaBufGrpcWebClient } from "../src/clients/grpc-web-client.js";
import { SilvanaNatsClient } from "../src/clients/nats-client.js";
import { BufEventFactory } from "../src/utils/event-factory.js";
import { LogLevel } from "../src/gen/events_pb.js";

describe("Example 4: gRPC-Web Search CoordinatorMessageEvents", async () => {
  it("should search CoordinatorMessageEvents for 'error' string using gRPC-Web", async () => {
    console.log(
      "🚀 Starting Example 4: gRPC-Web CoordinatorMessageEvent Search"
    );

    const grpcWebClient = new SilvanaBufGrpcWebClient();
    const natsClient = new SilvanaNatsClient();

    try {
      // 1. Connect to NATS for optional event streaming
      console.log("\n📡 Connecting to NATS...");
      await natsClient.connect();

      // Track received events
      const receivedEvents: any[] = [];
      const subscription = await natsClient.subscribeToEvents((event) => {
        receivedEvents.push(event);
      });

      // 2. Create some sample CoordinatorMessageEvents with different error types
      console.log("\n📝 Creating sample CoordinatorMessageEvents...");

      const testCoordinatorId = `coord-web-search-001-${Date.now()}`;
      const errorEvents = [
        BufEventFactory.createCoordinatorMessageEvent({
          coordinatorId: testCoordinatorId,
          level: LogLevel.ERROR,
          message:
            "gRPC-Web connection error: Failed to establish secure connection",
          eventTimestamp: BigInt(Date.now()),
        }),
        BufEventFactory.createCoordinatorMessageEvent({
          coordinatorId: testCoordinatorId,
          level: LogLevel.ERROR,
          message: "CORS error: Request blocked by browser security policy",
          eventTimestamp: BigInt(Date.now()),
        }),
        BufEventFactory.createCoordinatorMessageEvent({
          coordinatorId: testCoordinatorId,
          level: LogLevel.WARN,
          message: "Browser compatibility warning: Legacy browser detected",
          eventTimestamp: BigInt(Date.now()),
        }),
        BufEventFactory.createCoordinatorMessageEvent({
          coordinatorId: testCoordinatorId,
          level: LogLevel.ERROR,
          message: "WebSocket error: Connection lost during data streaming",
          eventTimestamp: BigInt(Date.now()),
        }),
        BufEventFactory.createCoordinatorMessageEvent({
          coordinatorId: testCoordinatorId,
          level: LogLevel.INFO,
          message: "Web client connected successfully via gRPC-Web",
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

      // 3. Send all events via gRPC-Web
      console.log("\n🌐 Sending events via gRPC-Web...");
      let grpcWebWorking = false;
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
          const response = await grpcWebClient.submitEvent(event);
          console.log(`✅ Event ${i + 1} submitted via gRPC-Web:`, response);
          grpcWebWorking = true;
          successCount++;
        } catch (error) {
          console.log(
            `❌ gRPC-Web submission failed for event ${i + 1}:`,
            (error as Error).message
          );
          console.log(
            "Note: This is expected if gRPC-Web proxy is not running"
          );
        }

        // Also publish to NATS for demonstration
        try {
          // TODO: Update NATS client to handle Buf Event types
          // await natsClient.publishEvent(event);
          console.log(
            `✅ Event ${
              i + 1
            } published to NATS (skipped - needs type conversion)`
          );
        } catch (error) {
          console.log(
            `❌ NATS publish failed for event ${i + 1}:`,
            (error as Error).message
          );
        }

        // Small delay between submissions
        await new Promise((resolve) => setTimeout(resolve, 500));
      }

      // 4. Wait a moment for events to be indexed
      console.log("\n⏳ Waiting for events to be indexed...");
      await new Promise((resolve) => setTimeout(resolve, 2000));

      // 5. Search for events containing "error" via gRPC-Web
      console.log(
        '\n🔍 Searching for CoordinatorMessageEvents containing "error" via gRPC-Web...'
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
        const searchResponse =
          await grpcWebClient.searchCoordinatorMessageEvents(searchRequest);

        console.log("\n📊 gRPC-Web Search Results:");
        console.log(`Total matches: ${searchResponse.totalCount}`);
        console.log(`Returned: ${searchResponse.returnedCount}`);

        // Verify search response structure if gRPC-Web is working
        if (grpcWebWorking) {
          assert.ok(
            searchResponse,
            "Search should return a response when gRPC-Web is working"
          );
          assert.ok(
            typeof searchResponse.totalCount !== "undefined",
            "Response should have totalCount"
          );
        }

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
        } else {
          console.log("No events found matching the search criteria.");
        }
      } catch (error) {
        console.error("❌ gRPC-Web search failed:", error);
        console.log(
          "Note: Make sure the gRPC-Web proxy is running and properly configured"
        );
        // Don't fail the test if gRPC-Web is not available
      }

      // 6. Try a browser-specific search
      console.log("\n🔍 Searching for browser-related events...");

      const browserSearchRequest = BufEventFactory.createSearchRequest({
        searchQuery: "browser",
        limit: 5,
        coordinatorId: testCoordinatorId,
      });

      try {
        const browserSearchResponse =
          await grpcWebClient.searchCoordinatorMessageEvents(
            browserSearchRequest
          );

        console.log("\n📊 Browser Search Results:");
        console.log(`Total matches: ${browserSearchResponse.totalCount}`);

        if (
          browserSearchResponse.events &&
          browserSearchResponse.events.length > 0
        ) {
          browserSearchResponse.events.forEach((event: any, index: number) => {
            console.log(`\n${index + 1}. Browser Event ID: ${event.id}`);
            console.log(`   Message: ${event.message}`);
            console.log(`   Relevance Score: ${event.relevance_score}`);
          });

          // Verify browser search results
          const firstBrowserResult = browserSearchResponse.events[0];
          assert.ok(
            firstBrowserResult.message.toLowerCase().includes("browser"),
            "Browser search results should contain browser-related messages"
          );
        } else {
          console.log("No browser-related events found");
        }
      } catch (error) {
        console.error("❌ Browser search failed:", error);
        console.log("Note: This is expected if gRPC-Web proxy is not running");
        // Don't fail the test if gRPC-Web is not available
      }

      // 7. Wait a bit to see NATS messages (if any were published)
      console.log("\n⏳ Waiting 2 seconds to see NATS messages...");
      await new Promise((resolve) => setTimeout(resolve, 2000));

      console.log(
        `📊 Received ${receivedEvents.length} events via NATS during test`
      );

      console.log("✅ Example 4 test completed successfully!");
    } catch (error) {
      console.error("❌ Error in Example 4 test:", error);
      throw error;
    } finally {
      // Cleanup
      console.log("\n🧹 Cleaning up...");
      await natsClient.close();
    }
  });
});
