import { describe, it } from "node:test";
import assert from "node:assert";
import {
  LogLevel,
  AgentMessageEvent,
  SubmitEventRequest,
  Event,
  AgentEvent,
  EventSchema,
} from "../src/proto/silvana/events/v1/events_pb.js";
import { fromBinary } from "@bufbuild/protobuf";
import { connect } from "@nats-io/transport-node";
import { DeliverPolicy, jetstream, jetstreamManager } from "@nats-io/jetstream";
import { createClient } from "@connectrpc/connect";
import { createGrpcTransport } from "@connectrpc/connect-node";
import { SilvanaEventsService } from "../src/proto/silvana/events/v1/events_pb.js";

const testServer = process.env.TEST_SERVER;

if (!testServer) {
  throw new Error("TEST_SERVER is not set");
}

const transport = createGrpcTransport({
  baseUrl: testServer,
});

const grpc = createClient(SilvanaEventsService, transport);

describe("Example 1: gRPC AgentMessageEvent with NATS", async () => {
  it("should send AgentMessageEvent to gRPC, read it from gRPC and NATS", async () => {
    console.log("🚀 Starting node gRPC AgentMessageEvent with NATS");

    const natsUrl = process.env.NATS_URL;
    if (!natsUrl) {
      throw new Error("NATS_URL is not set");
    }

    console.log("📡 Connecting to NATS...");
    const nc = await connect({
      servers: natsUrl,
      timeout: 1000,
    });

    const stream = "silvana";
    const js = jetstream(nc, { timeout: 2_000 });
    const jsm = await jetstreamManager(nc);
    await jsm.streams.get(stream);
    const consumerName = crypto.randomUUID();
    const consumer = await jsm.consumers.add(stream, {
      name: consumerName,
      deliver_policy: DeliverPolicy.StartTime,
      opt_start_time: new Date().toISOString(),
    });

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

      console.log("🔗 Sending event via gRPC client...");
      const submitResponse = await grpc.submitEvent(submitRequest);
      console.log("gRPC Submit Response:", submitResponse);
      assert.ok(submitResponse, "gRPC should return a response");
      assert.strictEqual(
        submitResponse.success,
        true,
        "gRPC should return success"
      );

      console.log("🔍 Querying AgentMessageEvents via gRPC client...");
      await new Promise((resolve) => setTimeout(resolve, 5000));
      const queryResponse = await grpc.getAgentMessageEventsBySequence({
        sequence: 1002n, // Note: using bigint literal
        coordinatorId: coordinatorId,
      });

      console.log("gRPC Query Response:", queryResponse);
      assert.ok(queryResponse, "Query should return a response");

      console.log("\n⏳ Looking for NATS messages...");
      let receivedMessage = false;
      const start = Date.now();

      while (!receivedMessage && Date.now() - start < 10_000) {
        console.log("waiting for messages");
        const c = await js.consumers.get(stream, consumer.config);
        const messages = await c.consume();
        try {
          for await (const m of messages) {
            console.log(m.seq);
            m.ack();
            const payload = m.data;
            try {
              const event = fromBinary(EventSchema, payload);
              if (
                event.eventType.case === "agent" &&
                event.eventType.value.event.case === "message"
              ) {
                const agentEvent = event.eventType.value.event.value;
                if (agentEvent.coordinatorId === coordinatorId) {
                  receivedMessage = true;
                  console.log("received message", agentEvent);
                  break;
                }
              }
            } catch (err) {
              console.log(`decode failed: ${err.message}`);
            }
          }
        } catch (err) {
          console.log(`consume failed: ${err.message}`);
        }
      }
    } catch (error: any) {
      console.error("❌ Error in Example 1", error?.message);
      throw error;
    } finally {
      // Cleanup
      console.log("\n🧹 Cleaning up...");
      await nc.drain();
    }
  });
});
