import { describe, it } from "node:test";
import assert from "node:assert";
import {
  LogLevel,
  AgentMessageEvent,
  SubmitEventRequest,
  Event,
  AgentEvent,
} from "../src/proto/silvana/events/v1/events_pb.js";
import { createClient } from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-node";
import { SilvanaEventsService } from "../src/proto/silvana/events/v1/events_pb.js";

const testServer = process.env.TEST_SERVER;

if (!testServer) {
  throw new Error("TEST_SERVER is not set");
}

const transport = createGrpcWebTransport({
  baseUrl: testServer,
  httpVersion: "2", // or "1.1"
});

const grpc = createClient(SilvanaEventsService, transport);

describe("Example 2: gRPC-Web AgentMessageEvent", async () => {
  it("should send AgentMessageEvent to gRPC-Web, read it from gRPC-Web", async () => {
    console.log("🚀 Starting node gRPC-Web AgentMessageEvent");

    try {
      console.log("📝 Creating AgentMessageEvent...");
      const jobId = `job-${crypto.randomUUID()}`;
      const coordinatorId = `coord-${crypto.randomUUID()}`;
      const agentEvent: AgentMessageEvent = {
        coordinatorId: coordinatorId,
        developer: "silvana",
        agent: "example-agent",
        app: "test-app",
        jobId,
        sequences: [1001n, 1002n, 1003n],
        level: LogLevel.INFO,
        message: "Agent message from Example 1 - gRPC test",
        eventTimestamp: BigInt(Date.now()),
      } as AgentMessageEvent;

      const agentEventWrapper: AgentEvent = {
        event: {
          case: "message",
          value: agentEvent,
        },
      } as AgentEvent;

      const event: Event = {
        eventType: {
          case: "agent",
          value: agentEventWrapper,
        },
      } as Event;

      const submitRequest: SubmitEventRequest = {
        event: event,
      } as SubmitEventRequest;

      console.log("🔗 Sending event via gRPC-Web client...");
      const submitResponse = await grpc.submitEvent(submitRequest);
      console.log("gRPC Submit Response:", submitResponse);
      assert.ok(submitResponse, "gRPC should return a response");
      assert.strictEqual(
        submitResponse.success,
        true,
        "gRPC should return success"
      );

      console.log("🔍 Querying AgentMessageEvents via gRPC-Web client...");
      await new Promise((resolve) => setTimeout(resolve, 5000));
      const queryResponse = await grpc.getAgentMessageEventsBySequence({
        sequence: 1002n, // Note: using bigint literal
        coordinatorId: coordinatorId,
      });

      console.log("gRPC Query Response:", queryResponse);
      assert.ok(queryResponse, "Query should return a response");
    } catch (error: any) {
      console.error("❌ Error in Example 2", error?.message);
      throw error;
    }
  });
});
