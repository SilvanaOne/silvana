// import { connect, NatsConnection, Subscription, JSONCodec } from "nats";
// import { config } from "../config/config.js";
// import type { Event as SilvanaEvent } from "../proto/silvana/events/v1/events_pb.js";

// export class SilvanaNatsClient {
//   private connection: NatsConnection | null = null;
//   private codec = JSONCodec();

//   async connect(): Promise<void> {
//     try {
//       this.connection = await connect({
//         servers: config.nats.servers,
//         timeout: 10000,
//         reconnect: true,
//         maxReconnectAttempts: 10,
//         reconnectTimeWait: 2000,
//       });

//       console.log("Connected to NATS server");
//     } catch (error) {
//       console.error("Failed to connect to NATS:", error);
//       throw error;
//     }
//   }

//   async subscribeToEvents(
//     callback: (event: any) => void
//   ): Promise<Subscription> {
//     if (!this.connection) {
//       throw new Error("NATS connection not established");
//     }

//     try {
//       const subscription = this.connection.subscribe(config.nats.subject);

//       console.log("Subscribed to NATS subject:", config.nats.subject);

//       (async () => {
//         for await (const message of subscription) {
//           try {
//             const event = this.codec.decode(message.data) as any;
//             callback(event);
//           } catch (error) {
//             console.error("Failed to decode NATS message:", error);
//           }
//         }
//       })();

//       return subscription;
//     } catch (error) {
//       console.error("Failed to subscribe to NATS:", error);
//       throw error;
//     }
//   }

//   async close(): Promise<void> {
//     if (this.connection) {
//       await this.connection.close();
//       this.connection = null;
//       console.log("NATS connection closed");
//     }
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
//               sequences: messageEvent.sequences.map((seq) => seq.toString()),
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
