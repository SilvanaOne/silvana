// Import Buf-generated types and create function
import {
  Event as SilvanaEvent,
  EventSchema,
  AgentEventSchema,
  AgentMessageEventSchema,
  CoordinatorEventSchema,
  CoordinatorMessageEventSchema,
  SearchCoordinatorMessageEventsRequest,
  SearchCoordinatorMessageEventsRequestSchema,
  GetAgentMessageEventsBySequenceRequest,
  GetAgentMessageEventsBySequenceRequestSchema,
  LogLevel,
} from "../gen/events_pb.js";
import { create } from "@bufbuild/protobuf";

// Re-export LogLevel from the new Buf-generated types for convenience
export { LogLevel } from "../gen/events_pb.js";

/**
 * Modern Buf-based Event Factory using generated Buf types
 * Uses clean TypeScript interfaces and native bigint support
 */
export class BufEventFactory {
  static createAgentMessageEvent(params: {
    coordinatorId: string;
    developer: string;
    agent: string;
    app: string;
    jobId: string;
    sequences?: bigint[];
    level?: LogLevel;
    message: string;
    eventTimestamp?: bigint;
  }): SilvanaEvent {
    // Create AgentMessageEvent
    const agentMessageEvent = create(AgentMessageEventSchema, {
      coordinatorId: params.coordinatorId,
      developer: params.developer,
      agent: params.agent,
      app: params.app,
      jobId: params.jobId,
      sequences: params.sequences || [],
      eventTimestamp: params.eventTimestamp || BigInt(Date.now()),
      level: params.level || LogLevel.INFO,
      message: params.message,
    });

    // Create AgentEvent wrapper
    const agentEvent = create(AgentEventSchema, {
      event: {
        case: "message",
        value: agentMessageEvent,
      },
    });

    // Create top-level Event
    return create(EventSchema, {
      eventType: {
        case: "agent",
        value: agentEvent,
      },
    });
  }

  static createCoordinatorMessageEvent(params: {
    coordinatorId: string;
    level?: LogLevel;
    message: string;
    eventTimestamp?: bigint;
  }): SilvanaEvent {
    // Create CoordinatorMessageEvent
    const coordinatorMessageEvent = create(CoordinatorMessageEventSchema, {
      coordinatorId: params.coordinatorId,
      eventTimestamp: params.eventTimestamp || BigInt(Date.now()),
      level: params.level || LogLevel.INFO,
      message: params.message,
    });

    // Create CoordinatorEvent wrapper
    const coordinatorEvent = create(CoordinatorEventSchema, {
      event: {
        case: "coordinatorError",
        value: coordinatorMessageEvent,
      },
    });

    // Create top-level Event
    return create(EventSchema, {
      eventType: {
        case: "coordinator",
        value: coordinatorEvent,
      },
    });
  }

  static createSearchRequest(params: {
    searchQuery: string;
    limit?: number;
    offset?: number;
    coordinatorId?: string;
  }): SearchCoordinatorMessageEventsRequest {
    return create(SearchCoordinatorMessageEventsRequestSchema, {
      searchQuery: params.searchQuery,
      limit: params.limit || 10,
      offset: params.offset || 0,
      coordinatorId: params.coordinatorId,
    });
  }

  static createSequenceRequest(params: {
    sequence: bigint;
    limit?: number;
    offset?: number;
    coordinatorId?: string;
    developer?: string;
    agent?: string;
    app?: string;
  }): GetAgentMessageEventsBySequenceRequest {
    return create(GetAgentMessageEventsBySequenceRequestSchema, {
      sequence: params.sequence,
      limit: params.limit,
      offset: params.offset,
      coordinatorId: params.coordinatorId,
      developer: params.developer,
      agent: params.agent,
      app: params.app,
    });
  }
}
