import { createClient } from "@connectrpc/connect";
import { createGrpcTransport } from "@connectrpc/connect-node";
import { SilvanaEventsService } from "../proto/silvana/events/v1/events_pb.js";
import { config } from "../config/config.js";

const testServer = process.env.TEST_SERVER;

if (!testServer) {
  throw new Error("TEST_SERVER is not set");
}

const transport = createGrpcTransport({
  baseUrl: testServer,
  //...{ tls: config.grpc.useSsl },
});

export const grpc = createClient(SilvanaEventsService, transport);

// export class SilvanaGrpcClient {
//   private client;

//   constructor() {
//     const transport = createConnectTransport({
//       baseUrl: config.grpc.url, // e.g. "https://rpc-dev.silvana.dev:50051"
//       httpVersion: "2",
//       ...{ tls: config.grpc.useSsl },
//     });

//     this.client = createClient(SilvanaEventsService, transport);

//     this.client.
//   }

//   /** Build a typed AgentMessageEvent using bufbuild/protobuf helpers */
//   createAgentMessageEvent(params: {
//     coordinatorId: string;
//     developer: string;
//     agent: string;
//     app: string;
//     jobId: string;
//     sequences: bigint[];
//     level: LogLevel;
//     message: string;
//     eventTimestamp?: bigint;
//   }): Event {
//     const agentMessageEvent = create(AgentMessageEventSchema, {
//       coordinatorId: params.coordinatorId,
//       developer: params.developer,
//       agent: params.agent,
//       app: params.app,
//       jobId: params.jobId,
//       sequences: params.sequences,
//       level: params.level,
//       message: params.message,
//       eventTimestamp: params.eventTimestamp ?? BigInt(Date.now()),
//     });

//     const agentEvent = create(AgentEventSchema, {
//       event: {
//         case: "message",
//         value: agentMessageEvent,
//       },
//     });

//     return create(EventSchema, {
//       eventType: {
//         case: "agent",
//         value: agentEvent,
//       },
//     });
//   }

//   /** Unary RPC ‑ single event */
//   async submitEvent(event: Event) {
//     return this.client.submitEvent(this.eventToPlainObject(event));
//   }

//   /** Unary RPC ‑ batch of events */
//   async submitEvents(events: Event[]) {
//     const plain = {
//       events: events.map((e) => this.eventToPlainObject(e)),
//     };
//     return this.client.submitEvents(plain);
//   }

//   async getAgentMessageEventsBySequence(
//     request: GetAgentMessageEventsBySequenceRequest
//   ): Promise<GetAgentMessageEventsBySequenceResponse> {
//     const plainRequest = {
//       sequence: request.sequence.toString(),
//       limit: request.limit,
//       offset: request.offset,
//       coordinator_id: request.coordinatorId,
//       developer: request.developer,
//       agent: request.agent,
//       app: request.app,
//     };
//     return this.client.getAgentMessageEventsBySequence(plainRequest);
//   }

//   async searchCoordinatorMessageEvents(
//     request: SearchCoordinatorMessageEventsRequest
//   ): Promise<SearchCoordinatorMessageEventsResponse> {
//     const plainRequest = {
//       search_query: request.searchQuery,
//       limit: request.limit,
//       offset: request.offset,
//       coordinator_id: request.coordinatorId,
//     };
//     return this.client.searchCoordinatorMessageEvents(plainRequest);
//   }

//   /** Translate generated Event into the plain JS object expected on the wire */
//   private eventToPlainObject(event: Event): any {
//     if (event.eventType.case === "agent") {
//       const agentEvent = event.eventType.value;
//       if (agentEvent.event.case === "message") {
//         const msg = agentEvent.event.value;
//         return {
//           agent: {
//             message: {
//               coordinator_id: msg.coordinatorId,
//               developer: msg.developer,
//               agent: msg.agent,
//               app: msg.app,
//               job_id: msg.jobId,
//               sequences: msg.sequences.map((s) => s.toString()),
//               event_timestamp: msg.eventTimestamp.toString(),
//               level: msg.level,
//               message: msg.message,
//             },
//           },
//         };
//       }
//     }
//     throw new Error(`Unsupported event type: ${event.eventType.case}`);
//   }

//   /** No explicit close needed – underlying HTTP/2 session is managed automatically */
//   close() {}
// }

// export class BufEventFactory {
//   static createAgentMessageEvent(params: {
//     coordinatorId: string;
//     developer: string;
//     agent: string;
//     app: string;
//     jobId: string;
//     sequences: bigint[];
//     level: LogLevel;
//     message: string;
//     eventTimestamp?: bigint;
//   }): Event {
//     const client = new SilvanaGrpcClient();
//     return client.createAgentMessageEvent(params);
//   }

//   static createSequenceRequest(params: {
//     sequence: bigint;
//     coordinatorId: string;
//     limit?: number;
//     offset?: number;
//   }) {
//     return {
//       sequence: params.sequence,
//       coordinatorId: params.coordinatorId,
//       limit: params.limit,
//       offset: params.offset,
//     };
//   }
// }
