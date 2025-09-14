"use client";

import { create } from "@bufbuild/protobuf";
import {
  AgentMessageEventSchema,
  EventSchema,
  LogLevel,
  SubmitEventRequest,
  SubmitEventRequestSchema,
} from "@/proto/silvana/rpc/v1/rpc_pb";

export async function createGrpcRequest(
  coordinatorId: string
): Promise<SubmitEventRequest> {
  const jobId = `job-${crypto.randomUUID()}`;

  // Create AgentMessageEvent using the schema
  const agentMessageEvent = create(AgentMessageEventSchema, {
    coordinatorId: coordinatorId,
    sessionId: `session-${Date.now()}`,
    jobId,
    level: LogLevel.INFO,
    message: `Agent message from gRPC-Web test at ${new Date().toISOString()}`,
    eventTimestamp: BigInt(Date.now()),
  });

  // Create Event with the agent message
  const event = create(EventSchema, {
    event: {
      case: "agentMessage",
      value: agentMessageEvent,
    },
  });

  // Create SubmitEventRequest
  const submitRequest = create(SubmitEventRequestSchema, {
    event: event,
  });

  return submitRequest;
}
