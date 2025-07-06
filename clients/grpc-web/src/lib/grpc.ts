"use client";

import {
  AgentEvent,
  AgentMessageEvent,
  Event,
  LogLevel,
  SubmitEventRequest,
} from "@/proto/silvana/events/v1/events_pb";

export async function createGrpcRequest(
  coordinatorId: string
): Promise<SubmitEventRequest> {
  const jobId = `job-${crypto.randomUUID()}`;
  const agentEvent: AgentMessageEvent = {
    coordinatorId: coordinatorId,
    developer: "silvana",
    agent: "example-agent",
    app: "test-app",
    jobId,
    sequences: [1001n, 1002n, 1003n],
    level: LogLevel.INFO,
    message: "Agent message from Example 2 - gRPC-Web test",
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

  return submitRequest;
}
