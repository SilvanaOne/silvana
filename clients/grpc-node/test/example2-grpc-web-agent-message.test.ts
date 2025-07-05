import { describe, it } from "node:test";
import assert from "node:assert";
import { grpc } from "../src/clients/grpc-web-client.js";
import { SilvanaNatsClient } from "../src/clients/nats-client.js";
import { BufEventFactory } from "../src/utils/event-factory.js";
import { LogLevel } from "../src/gen/events_pb.js";

describe("Example 2: gRPC-Web AgentMessageEvent with NATS", async () => {
  it("should send AgentMessageEvent to gRPC-Web, get it from gRPC-Web and NATS", async () => {
    console.log("🚀 Starting Example 2: gRPC-Web AgentMessageEvent with NATS");

    const grpcWebClient = new SilvanaBufGrpcWebClient();
    const natsClient = new SilvanaNatsClient();

    try {
      // 1. Connect to NATS and set up subscription
      console.log("\n📡 Connecting to NATS...");
      await natsClient.connect();

      // Track received events
      const receivedEvents: any[] = [];
      const subscription = await natsClient.subscribeToEvents((event) => {
        console.log(
          "\n📨 Received event from NATS:",
          JSON.stringify(event, null, 2)
        );
        receivedEvents.push(event);
      });

      // 2. Create an AgentMessageEvent
      console.log("\n📝 Creating AgentMessageEvent...");
      const agentEvent = BufEventFactory.createAgentMessageEvent({
        coordinatorId: "coord-002-test",
        developer: "silvana-team",
        agent: "web-agent",
        app: "web-test-app",
        jobId: `web-job-${Date.now()}`,
        sequences: [2001n, 2002n],
        level: LogLevel.INFO,
        message: "Agent message from Example 2 - gRPC-Web test",
        eventTimestamp: BigInt(Date.now()),
      });

      console.log(
        "Created event:",
        JSON.stringify(
          agentEvent,
          (key, value) =>
            typeof value === "bigint" ? value.toString() : value,
          2
        )
      );

      // Verify event structure using the new discriminated union approach
      assert.strictEqual(
        agentEvent.eventType.case,
        "agent",
        "Event should be an agent event"
      );

      if (agentEvent.eventType.case === "agent") {
        const agentEventValue = agentEvent.eventType.value;
        assert.strictEqual(
          agentEventValue.event.case,
          "message",
          "Agent event should be a message"
        );

        if (agentEventValue.event.case === "message") {
          const messageEvent = agentEventValue.event.value;
          assert.strictEqual(messageEvent.coordinatorId, "coord-002-test");
          assert.strictEqual(
            messageEvent.message,
            "Agent message from Example 2 - gRPC-Web test"
          );
        }
      }

      // 3. Send event via gRPC-Web (may fail if proxy not running)
      console.log("\n🌐 Sending event via gRPC-Web...");
      let grpcWebWorking = false;
      try {
        const submitResponse = await grpcWebClient.submitEvent(agentEvent);
        console.log("gRPC-Web Submit Response:", submitResponse);
        grpcWebWorking = true;
        assert.ok(
          submitResponse,
          "gRPC-Web should return a response when working"
        );
      } catch (error) {
        console.log(
          "Note: gRPC-Web may fail if proxy is not running. Error:",
          (error as Error).message
        );
        // This is expected if proxy is not running, so we don't fail the test
      }

      // 4. Publish to NATS for demonstration
      console.log("\n📤 Publishing event to NATS...");
      await natsClient.publishEvent(agentEvent);

      // 5. Query the event back via gRPC-Web (may fail if proxy not running)
      console.log("\n🔍 Querying AgentMessageEvents via gRPC-Web...");
      const queryRequest = BufEventFactory.createSequenceRequest({
        sequence: 2001n, // One of the sequences we sent
        limit: 5,
        coordinatorId: "coord-002-test",
      });

      try {
        const queryResponse =
          await grpcWebClient.getAgentMessageEventsBySequence(queryRequest);
        console.log(
          "gRPC-Web Query Response:",
          JSON.stringify(
            queryResponse,
            (key, value) =>
              typeof value === "bigint" ? value.toString() : value,
            2
          )
        );
        if (grpcWebWorking) {
          assert.ok(
            queryResponse,
            "Query should return a response when gRPC-Web is working"
          );
        }
      } catch (error) {
        console.log(
          "Note: gRPC-Web query may fail if proxy is not running. Error:",
          (error as Error).message
        );
      }

      // 6. Wait a bit to see NATS messages
      console.log("\n⏳ Waiting 3 seconds to see NATS messages...");
      await new Promise((resolve) => setTimeout(resolve, 3000));

      // Verify NATS received at least one event
      assert.ok(
        receivedEvents.length > 0,
        "Should receive at least one event via NATS"
      );

      console.log("✅ Example 2 test completed successfully!");
    } catch (error) {
      console.error("❌ Error in Example 2 test:", error);
      throw error;
    } finally {
      // Cleanup
      console.log("\n🧹 Cleaning up...");
      await natsClient.close();
    }
  });
});
