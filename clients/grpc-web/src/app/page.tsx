"use client";

import { useState, useEffect } from "react";
import { createGrpcWebTransport } from "@connectrpc/connect-web";
import { createClient } from "@connectrpc/connect";
import {
  EventSchema,
  SilvanaEventsService,
} from "@/proto/silvana/events/v1/events_pb";
import { createGrpcRequest } from "@/lib/grpc";
import { wsconnect } from "@nats-io/nats-core";
import {
  DeliverPolicy,
  jetstream,
  jetstreamManager,
  Consumer,
} from "@nats-io/jetstream";
import { fromBinary } from "@bufbuild/protobuf";
import { serialize } from "@/lib/serialize";

const stream = "silvana";

export default function Home() {
  const [eventResponseLog, setEventResponseLog] = useState<string[]>([]);
  const [queryResponseLog, setQueryResponseLog] = useState<string[]>([]);
  const [natsMessageLog, setNatsMessageLog] = useState<string[]>([]);
  const [isLoading, setIsLoading] = useState<boolean>(false);
  const [sendRequestProcessed, setSendRequestProcessed] = useState<
    number | null
  >(null);
  const [queryRoundtripDelay, setQueryRoundtripDelay] = useState<number | null>(
    null
  );
  const [natsRoundtripDelay, setNatsRoundtripDelay] = useState<number | null>(
    null
  );
  const [error, setError] = useState<string | null>(null);
  const [grpcConnection, setGrpcConnection] = useState<
    "connecting" | "connected" | "error"
  >("connecting");
  const [natsConnection, setNatsConnection] = useState<
    "connecting" | "connected" | "error"
  >("connecting");

  const [grpc, setGrpc] = useState<ReturnType<
    typeof createClient<typeof SilvanaEventsService>
  > | null>(null);
  const [nats, setNats] = useState<ReturnType<typeof jetstream> | null>(null);
  const [natsConsumer, setNatsConsumer] = useState<Consumer | null>(null);

  useEffect(() => {
    const initializeGrpc = async () => {
      try {
        const transport = createGrpcWebTransport({
          baseUrl: "https://rpc.silvana.dev",
        });
        const grpc = createClient(SilvanaEventsService, transport);

        const coordinatorId = `coord-browser-${crypto.randomUUID()}`;
        let testQueryResponse = await grpc.getAgentMessageEventsBySequence({
          sequence: 1002n,
          coordinatorId: "test-coordinator",
        });
        if (testQueryResponse?.success === true) {
          setGrpc(grpc);
          setGrpcConnection("connected");
        } else {
          setError(
            `Failed to connect to Silvana RPC: ${
              testQueryResponse?.message ?? "Unknown error"
            }`
          );
          setGrpcConnection("error");
        }
      } catch (error: any) {
        setError(error instanceof Error ? error.message : "Unknown error");
      }
    };

    initializeGrpc();
  }, []);

  useEffect(() => {
    const initializeNats = async () => {
      try {
        const nc = await wsconnect({
          servers: ["wss://rpc.silvana.dev:8080/ws"],
          timeout: 2000,
        });
        const stream = "silvana";
        const js = jetstream(nc, { timeout: 2_000 });
        const jsm = await jetstreamManager(nc);
        await jsm.streams.get(stream);
        const consumerName = crypto.randomUUID();
        const consumerInfo = await jsm.consumers.add(stream, {
          name: consumerName,
          deliver_policy: DeliverPolicy.StartTime,
          opt_start_time: new Date().toISOString(),
        });
        const consumer = await js.consumers.get(stream, consumerInfo.config);
        console.log("Waiting for NATS message");
        let message = await consumer.next({
          expires: 1000,
        });
        console.log("Received NATS message:", message?.seq);
        while (message !== null) {
          console.log("Received NATS message:", message.seq);
          message.ack();
          console.log("Waiting for NATS message");
          message = await consumer.next({
            expires: 1000,
          });
          console.log("Received next NATS message:", message?.seq);
        }
        setNats(js);
        setNatsConsumer(consumer);
        setNatsConnection("connected");
      } catch (error: any) {
        setError(error instanceof Error ? error.message : "Unknown NATS error");
        setNatsConnection("error");
      }
    };

    initializeNats();
  }, []);

  async function processNatsMessages(params: {
    coordinatorId: string;
    start: number;
  }): Promise<void> {
    if (!nats || !natsConsumer) {
      setError("NATS not initialized");
      return;
    }
    const { coordinatorId, start } = params;
    try {
      let message = await natsConsumer.next({
        expires: 1000,
      });

      while (message && Date.now() - start < 5000) {
        console.log("NATS message:", message.seq);
        message.ack();
        const payload = message.data;
        try {
          const event = fromBinary(EventSchema, payload);
          if (
            event.eventType.case === "agent" &&
            event.eventType.value.event.case === "message"
          ) {
            const agentEvent = event.eventType.value.event.value;
            if (agentEvent.coordinatorId === coordinatorId) {
              const endNats = Date.now();
              console.log("received message", agentEvent);
              setNatsMessageLog([
                `Received NATS message: ${serialize(agentEvent)}`,
              ]);
              setNatsRoundtripDelay(endNats - start);
              return;
            }
          }
        } catch (error: any) {
          console.log(`decode failed: ${error.message}`);
        }
        message = await natsConsumer.next({
          expires: 1000,
        });
      }
    } catch (error: any) {
      console.log(`consume failed: ${error.message}`);
      setError(error instanceof Error ? error.message : "Unknown NATS error");
      setNatsConnection("error");
      return;
    }

    setError("NATS message timeout");
    setNatsConnection("error");
  }

  async function sendGrpcQuery(params: {
    coordinatorId: string;
    start: number;
  }): Promise<void> {
    if (!grpc) {
      setError("gRPC not initialized");
      return;
    }
    const { coordinatorId, start } = params;
    try {
      console.log("🔍 Querying AgentMessageEvents via gRPC-Web client...");
      let queryResponse = await grpc.getAgentMessageEventsBySequence({
        sequence: 1002n,
        coordinatorId,
      });
      while (queryResponse.returnedCount === 0 && Date.now() - start < 5000) {
        queryResponse = await grpc.getAgentMessageEventsBySequence({
          sequence: 1002n,
          coordinatorId,
        });
      }
      const endQuery = Date.now();
      if (queryResponse.returnedCount === 0) {
        setError("Query timed out");
        setIsLoading(false);
        return;
      }
      const durationQuery = endQuery - start;
      console.log(`Query processed in ${durationQuery}ms`);
      console.log("gRPC Query Response:", queryResponse);
      setQueryResponseLog([serialize(queryResponse)]);
      setQueryRoundtripDelay(durationQuery);
    } catch (error: any) {
      setError(
        error instanceof Error ? error.message : "Unknown GPRC-Web error"
      );
    }
  }

  const handleSendGrpcRequest = async () => {
    if (isLoading || !grpc) return;
    try {
      setIsLoading(true);
      setSendRequestProcessed(null);
      setNatsRoundtripDelay(null);
      setQueryRoundtripDelay(null);
      setNatsMessageLog([]);
      setQueryResponseLog([]);
      setEventResponseLog([]);
      setEventResponseLog([...eventResponseLog, "Sending gRPC-Web request..."]);
      const coordinatorId = `coord-browser-${crypto.randomUUID()}`;
      const request = await createGrpcRequest(coordinatorId);
      console.log("sending request: ", request);
      const startRequest = Date.now();
      const response = await grpc.submitEvent(request);
      const endRequest = Date.now();
      const durationRequest = endRequest - startRequest;
      console.log(`Send request processed in ${durationRequest}ms`);
      console.log("Response on send request: ", response);
      setSendRequestProcessed(durationRequest);
      setEventResponseLog([serialize(response)]);
      const endNatsPromise = processNatsMessages({
        coordinatorId,
        start: startRequest,
      });
      const endQueryPromise = sendGrpcQuery({
        coordinatorId,
        start: startRequest,
      });

      const [endNats, endQuery] = await Promise.all([
        endNatsPromise,
        endQueryPromise,
      ]);
    } catch (error: any) {
      setError(error instanceof Error ? error.message : "Unknown error");
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div className="grid grid-rows-[1fr] items-start justify-items-center min-h-screen p-4 pt-8 pb-20 gap-8 sm:p-8 sm:pt-12 font-[family-name:var(--font-geist-sans)]">
      <main className="flex flex-col gap-[24px] items-center sm:items-start w-full max-w-6xl">
        <p className="text-sm/6 text-center sm:text-left font-[family-name:var(--font-geist-mono)] tracking-[-.01em]">
          This is a simple example of a gRPC-Web and NATS application using
          Silvana RPC.
        </p>
        {/* Error Display */}
        {/* Connection Status */}
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 w-full max-w-4xl">
          <div className="bg-gray-50 dark:bg-gray-900/20 rounded-lg p-4 border border-gray-200 dark:border-gray-800">
            <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              gRPC-Web Status
            </h4>
            <div
              className={`text-sm font-mono flex items-center gap-2 ${
                grpcConnection === "connected"
                  ? "text-green-700 dark:text-green-300"
                  : grpcConnection === "connecting"
                  ? "text-yellow-700 dark:text-yellow-300"
                  : "text-red-700 dark:text-red-300"
              }`}
            >
              <div
                className={`w-2 h-2 rounded-full ${
                  grpcConnection === "connected"
                    ? "bg-green-500"
                    : grpcConnection === "connecting"
                    ? "bg-yellow-500"
                    : "bg-red-500"
                }`}
              ></div>
              {grpcConnection === "connected"
                ? "Connected"
                : grpcConnection === "connecting"
                ? "Connecting..."
                : "Error"}
            </div>
          </div>
          <div className="bg-gray-50 dark:bg-gray-900/20 rounded-lg p-4 border border-gray-200 dark:border-gray-800">
            <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              NATS Status
            </h4>
            <div
              className={`text-sm font-mono flex items-center gap-2 ${
                natsConnection === "connected"
                  ? "text-green-700 dark:text-green-300"
                  : natsConnection === "connecting"
                  ? "text-yellow-700 dark:text-yellow-300"
                  : "text-red-700 dark:text-red-300"
              }`}
            >
              <div
                className={`w-2 h-2 rounded-full ${
                  natsConnection === "connected"
                    ? "bg-green-500"
                    : natsConnection === "connecting"
                    ? "bg-yellow-500"
                    : "bg-red-500"
                }`}
              ></div>
              {natsConnection === "connected"
                ? "Connected"
                : natsConnection === "connecting"
                ? "Connecting..."
                : "Error"}
            </div>
          </div>
        </div>
        {error && (
          <div className="bg-red-50 dark:bg-red-900/20 rounded-lg p-4 w-full max-w-4xl border border-red-200 dark:border-red-800">
            <h4 className="text-sm font-medium text-red-700 dark:text-red-300 mb-2">
              Error
            </h4>
            <div className="text-sm text-red-900 dark:text-red-100 font-mono">
              {error}
            </div>
          </div>
        )}

        {/* Statistics */}
        <div className="grid grid-cols-1 sm:grid-cols-3 gap-4 w-full max-w-4xl">
          <div className="bg-blue-50 dark:bg-blue-900/20 rounded-lg p-4 text-center border border-blue-200 dark:border-blue-800">
            <h4 className="text-sm font-medium text-blue-700 dark:text-blue-300 mb-1">
              Send Request Processed
            </h4>
            <div className="text-2xl font-bold text-blue-900 dark:text-blue-100">
              {sendRequestProcessed} ms
            </div>
          </div>
          <div className="bg-green-50 dark:bg-green-900/20 rounded-lg p-4 text-center border border-green-200 dark:border-green-800">
            <h4 className="text-sm font-medium text-green-700 dark:text-green-300 mb-1">
              Query Roundtrip Delay
            </h4>
            <div className="text-2xl font-bold text-green-900 dark:text-green-100">
              {queryRoundtripDelay} ms
            </div>
          </div>
          <div className="bg-purple-50 dark:bg-purple-900/20 rounded-lg p-4 text-center border border-purple-200 dark:border-purple-800">
            <h4 className="text-sm font-medium text-purple-700 dark:text-purple-300 mb-1">
              NATS Roundtrip Delay
            </h4>
            <div className="text-2xl font-bold text-purple-900 dark:text-purple-100">
              {natsRoundtripDelay} ms
            </div>
          </div>
        </div>

        <div className="flex gap-4 items-center flex-col sm:flex-row">
          <button
            className="rounded-full border border-solid border-black/[.08] dark:border-white/[.145] transition-colors flex items-center justify-center hover:bg-[#f2f2f2] dark:hover:bg-[#1a1a1a] hover:border-transparent font-medium text-sm sm:text-base h-10 sm:h-12 px-4 sm:px-5 w-full sm:w-auto md:w-[316px] disabled:opacity-50 disabled:cursor-not-allowed"
            onClick={handleSendGrpcRequest}
            disabled={isLoading}
          >
            {isLoading ? (
              <div className="flex items-center gap-2">
                <div className="animate-spin rounded-full h-4 w-4 border-2 border-gray-300 border-t-gray-600"></div>
                <span>Sending...</span>
              </div>
            ) : (
              "Send Event"
            )}
          </button>
        </div>

        {/* Log Windows */}
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-6 w-full mt-8">
          {/* gRPC-Web Request Log */}
          <div className="flex flex-col">
            <h3 className="text-lg font-semibold mb-2 text-center">
              gRPC-Web Event Response
            </h3>
            <div className="bg-gray-100 dark:bg-gray-800 rounded-lg p-4 h-64 overflow-y-auto border border-gray-200 dark:border-gray-700">
              <div className="font-mono text-xs text-gray-700 dark:text-gray-300 whitespace-pre-wrap">
                {eventResponseLog.length > 0 ? (
                  eventResponseLog.map((log, index) => (
                    <div key={index} className="mb-1">
                      {log}
                    </div>
                  ))
                ) : (
                  <div className="text-gray-500 dark:text-gray-400 italic">
                    No response yet...
                  </div>
                )}
              </div>
            </div>
          </div>

          {/* gRPC-Web Response Log */}
          <div className="flex flex-col">
            <h3 className="text-lg font-semibold mb-2 text-center">
              gRPC-Web Query Response
            </h3>
            <div className="bg-gray-100 dark:bg-gray-800 rounded-lg p-4 h-64 overflow-y-auto border border-gray-200 dark:border-gray-700">
              <div className="font-mono text-xs text-gray-700 dark:text-gray-300 whitespace-pre-wrap">
                {queryResponseLog.length > 0 ? (
                  queryResponseLog.map((log, index) => (
                    <div key={index} className="mb-1">
                      {log}
                    </div>
                  ))
                ) : (
                  <div className="text-gray-500 dark:text-gray-400 italic">
                    No response yet...
                  </div>
                )}
              </div>
            </div>
          </div>

          {/* NATS Message Log */}
          <div className="flex flex-col">
            <h3 className="text-lg font-semibold mb-2 text-center">
              NATS Message
            </h3>
            <div className="bg-gray-100 dark:bg-gray-800 rounded-lg p-4 h-64 overflow-y-auto border border-gray-200 dark:border-gray-700">
              <div className="font-mono text-xs text-gray-700 dark:text-gray-300 whitespace-pre-wrap">
                {natsMessageLog.length > 0 ? (
                  natsMessageLog.map((log, index) => (
                    <div key={index} className="mb-1">
                      {log}
                    </div>
                  ))
                ) : (
                  <div className="text-gray-500 dark:text-gray-400 italic">
                    No messages yet...
                  </div>
                )}
              </div>
            </div>
          </div>
        </div>
      </main>
      <footer className="row-start-3 flex gap-[24px] flex-wrap items-center justify-center"></footer>
    </div>
  );
}
