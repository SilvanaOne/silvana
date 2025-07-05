// import { config } from "../config/config.js";
// import fetch from "node-fetch";
// import {
//   Event as SilvanaEvent,
//   GetAgentMessageEventsBySequenceRequest,
//   SearchCoordinatorMessageEventsRequest,
// } from "../gen/events_pb.js";

// export class SilvanaGrpcWebClient {
//   private baseUrl: string;

//   constructor() {
//     this.baseUrl = config.grpcWeb.url;
//   }

//   async submitEvent(event: SilvanaEvent): Promise<any> {
//     return this.makeRequest(
//       "/silvana.events.SilvanaEventsService/SubmitEvent",
//       this.eventToPlainObject(event)
//     );
//   }

//   async submitEvents(events: SilvanaEvent[]): Promise<any> {
//     return this.makeRequest(
//       "/silvana.events.SilvanaEventsService/SubmitEvents",
//       { events: events.map((event) => this.eventToPlainObject(event)) }
//     );
//   }

//   async getAgentMessageEventsBySequence(
//     request: GetAgentMessageEventsBySequenceRequest
//   ): Promise<any> {
//     const plainRequest = {
//       sequence: request.sequence.toString(),
//       limit: request.limit,
//       offset: request.offset,
//       coordinator_id: request.coordinatorId,
//       developer: request.developer,
//       agent: request.agent,
//       app: request.app,
//     };

//     return this.makeRequest(
//       "/silvana.events.SilvanaEventsService/GetAgentMessageEventsBySequence",
//       plainRequest
//     );
//   }

//   async searchCoordinatorMessageEvents(
//     request: SearchCoordinatorMessageEventsRequest
//   ): Promise<any> {
//     const plainRequest = {
//       search_query: request.searchQuery,
//       limit: request.limit,
//       offset: request.offset,
//       coordinator_id: request.coordinatorId,
//     };

//     return this.makeRequest(
//       "/silvana.events.SilvanaEventsService/SearchCoordinatorMessageEvents",
//       plainRequest
//     );
//   }

//   private async makeRequest(method: string, data: any): Promise<any> {
//     const response = await fetch(`${this.baseUrl}${method}`, {
//       method: "POST",
//       headers: {
//         "Content-Type": "application/grpc-web+proto",
//         "grpc-timeout": "10S",
//       },
//       body: JSON.stringify(data),
//     });

//     if (!response.ok) {
//       throw new Error(`gRPC-Web request failed: ${response.statusText}`);
//     }

//     return response.json();
//   }

//   private eventToPlainObject(event: SilvanaEvent): any {
//     if (event.eventType.case === "agent") {
//       const agentEvent = event.eventType.value;
//       if (agentEvent.event.case === "message") {
//         const messageEvent = agentEvent.event.value;
//         return {
//           agent: {
//             message: {
//               coordinator_id: messageEvent.coordinatorId,
//               developer: messageEvent.developer,
//               agent: messageEvent.agent,
//               app: messageEvent.app,
//               job_id: messageEvent.jobId,
//               sequences: messageEvent.sequences.map((seq) => seq.toString()), // Convert bigint to string
//               event_timestamp: messageEvent.eventTimestamp.toString(),
//               level: messageEvent.level,
//               message: messageEvent.message,
//             },
//           },
//         };
//       }
//     } else if (event.eventType.case === "coordinator") {
//       const coordinatorEvent = event.eventType.value;
//       if (coordinatorEvent.event.case === "coordinatorError") {
//         const errorEvent = coordinatorEvent.event.value;
//         return {
//           coordinator: {
//             coordinator_error: {
//               coordinator_id: errorEvent.coordinatorId,
//               event_timestamp: errorEvent.eventTimestamp.toString(),
//               level: errorEvent.level,
//               message: errorEvent.message,
//             },
//           },
//         };
//       }
//     }

//     throw new Error(`Unsupported event type: ${event.eventType.case}`);
//   }
// }
