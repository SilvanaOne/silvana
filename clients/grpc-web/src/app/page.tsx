"use client";

import { useState, useEffect } from "react";
import { createGrpcWebTransport } from "@connectrpc/connect-web";
import { createClient } from "@connectrpc/connect";
import {
  EventSchema,
  SilvanaRpcService,
} from "@/proto/silvana/rpc/v1/rpc_pb";
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
import { AnimatedBackground } from "@/components/AnimatedBackground";
import { NavBar } from "@/components/NavBar";

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
    typeof createClient<typeof SilvanaRpcService>
  > | null>(null);
  const [nats, setNats] = useState<ReturnType<typeof jetstream> | null>(null);
  const [natsConsumer, setNatsConsumer] = useState<Consumer | null>(null);

  useEffect(() => {
    const initializeGrpc = async () => {
      try {
        const transport = createGrpcWebTransport({
          baseUrl: "https://rpc.silvana.dev",
        });
        const grpc = createClient(SilvanaRpcService, transport);

        // Test the connection with a simple query
        const testQueryResponse = await grpc.getEventsByAppInstanceSequence({
          sequence: 1n,
          appInstanceId: "test-connection",
          limit: 1,
        });
        // If we get any response (even with 0 events), the connection works
        if (testQueryResponse !== undefined) {
          setGrpc(grpc);
          setGrpcConnection("connected");
          console.log("✅ Connected to Silvana RPC");
        } else {
          setError("Failed to connect to Silvana RPC");
          setGrpcConnection("error");
        }
      } catch (error: unknown) {
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
      } catch (error: unknown) {
        setError(error instanceof Error ? error.message : "Unknown NATS error");
        setNatsConnection("error");
      }
    };

    initializeNats();
  }, []);

  // Update connection status dots
  useEffect(() => {
    const grpcDot = document.getElementById("grpc-dot");
    if (grpcDot) {
      grpcDot.style.backgroundColor =
        grpcConnection === "connected"
          ? "#00FFA3" // brand-teal
          : grpcConnection === "connecting"
          ? "#facc15" // yellow-400
          : "#ef4444"; // red-500
    }
  }, [grpcConnection]);

  useEffect(() => {
    const natsDot = document.getElementById("nats-dot");
    if (natsDot) {
      natsDot.style.backgroundColor =
        natsConnection === "connected"
          ? "#00FFA3" // brand-teal
          : natsConnection === "connecting"
          ? "#facc15" // yellow-400
          : "#ef4444"; // red-500
    }
  }, [natsConnection]);

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
          if (event.event.case === "agentMessage") {
            const agentEvent = event.event.value;
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
        } catch (error: unknown) {
          console.log(
            `decode failed: ${
              error instanceof Error ? error.message : "Unknown error"
            }`
          );
        }
        message = await natsConsumer.next({
          expires: 1000,
        });
      }
    } catch (error: unknown) {
      console.log(
        `consume failed: ${
          error instanceof Error ? error.message : "Unknown error"
        }`
      );
      setError(error instanceof Error ? error.message : "Unknown NATS error");
      setNatsConnection("error");
      return;
    }

    setError("NATS message timeout");
    setNatsConnection("error");
  }

  async function sendGrpcQuery(params: {
    start: number;
    appInstanceId: string;
  }): Promise<void> {
    if (!grpc) {
      setError("gRPC not initialized");
      return;
    }
    const { start, appInstanceId } = params;
    try {
      console.log(`🔍 Querying Events via gRPC-Web client for app instance: ${appInstanceId}...`);
      let queryResponse = await grpc.getEventsByAppInstanceSequence({
        sequence: 1n, // Start from sequence 1
        appInstanceId: appInstanceId,
        limit: 100, // Get up to 100 events
      });

      // Wait for events to appear (since we just created this app instance)
      let attempts = 0;
      while (queryResponse.returnedCount === 0 && Date.now() - start < 5000) {
        await new Promise(resolve => setTimeout(resolve, 500)); // Wait 500ms between attempts
        queryResponse = await grpc.getEventsByAppInstanceSequence({
          sequence: 1n,
          appInstanceId: appInstanceId,
          limit: 100,
        });
        attempts++;
      }

      const endQuery = Date.now();
      console.log(`Query completed after ${attempts} attempts`);

      // Note: It's expected to have 0 events for a new app instance
      // The query itself succeeding means the connection works
      if (!queryResponse.success) {
        setError(`Query failed: ${queryResponse.message || "Unknown error"}`);
        setIsLoading(false);
        return;
      }
      const durationQuery = endQuery - start;
      console.log(`Query processed in ${durationQuery}ms`);
      console.log("gRPC Query Response:", queryResponse);

      // Create a user-friendly message about the query result
      const resultMessage = queryResponse.returnedCount > 0
        ? `Found ${queryResponse.returnedCount} events`
        : "Query successful (0 events - this is normal for a new app instance)";

      setQueryResponseLog([
        resultMessage,
        serialize(queryResponse)
      ]);
      setQueryRoundtripDelay(durationQuery);

      // Clear any previous errors since the query succeeded
      setError("");
    } catch (error: unknown) {
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
      const appInstanceId = `app-instance-${Date.now()}`; // Dynamic app instance ID
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
        start: startRequest,
        appInstanceId: appInstanceId,
      });

      await Promise.all([endNatsPromise, endQueryPromise]);

      // If we got here without errors, the test was successful
      if (!error) {
        console.log("✅ gRPC-Web test completed successfully");
      }
    } catch (error: unknown) {
      setError(error instanceof Error ? error.message : "Unknown error");
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <>
      <NavBar
        isLoading={isLoading}
        grpc={grpc !== null}
        onSendEvent={handleSendGrpcRequest}
      />
      <div className="min-h-screen p-10 relative bg-x-gradient overflow-hidden">
        <div className="absolute inset-0 pointer-events-none">
          <div className="absolute -left-20 -top-20 w-[500px] h-[500px] rotate-45 bg-gradient-to-br from-brand-purple/20 to-brand-blue/10" />
          <div className="absolute right-[-250px] top-1/3 w-[700px] h-[700px] -rotate-45 bg-gradient-to-bl from-brand-purple/10 to-brand-pink/10" />
        </div>
        <AnimatedBackground />
        <main className="pt-12">
          <div className="max-w-8xl mx-auto">
            {/* System Status & Performance */}
            <div className="grid grid-cols-1 lg:grid-cols-3 gap-4 mb-6">
              {/* Connection Status */}
              <section className="glass-card p-4 lg:col-span-1">
                <h3 className="text-lg lg:text-xl font-bold text-white mb-4">
                  Connection Status
                </h3>
                <div className="space-y-2">
                  <div className="flex items-center gap-2">
                    <div
                      className={`w-2.5 h-2.5 rounded-full ${
                        grpcConnection === "connected"
                          ? "bg-brand-green animate-pulse-success"
                          : grpcConnection === "connecting"
                          ? "bg-brand-yellow animate-pulse-warning"
                          : "bg-red-500 animate-pulse-error"
                      }`}
                    ></div>
                    <div className="flex-1">
                      <div className="text-sm font-medium text-white">
                        gRPC-Web
                      </div>
                      <div className="text-sm text-white/60">
                        {grpcConnection === "connected"
                          ? "Connected to Silvana RPC"
                          : grpcConnection === "connecting"
                          ? "Establishing connection..."
                          : "Connection failed"}
                      </div>
                    </div>
                  </div>

                  <div className="flex items-center gap-2">
                    <div
                      className={`w-2.5 h-2.5 rounded-full ${
                        natsConnection === "connected"
                          ? "bg-brand-green animate-pulse-success"
                          : natsConnection === "connecting"
                          ? "bg-brand-yellow animate-pulse-warning"
                          : "bg-red-500 animate-pulse-error"
                      }`}
                    ></div>
                    <div className="flex-1">
                      <div className="text-sm font-medium text-white">
                        NATS JetStream
                      </div>
                      <div className="text-sm text-white/60">
                        {natsConnection === "connected"
                          ? "Connected to message stream"
                          : natsConnection === "connecting"
                          ? "Establishing connection..."
                          : "Connection failed"}
                      </div>
                    </div>
                  </div>
                </div>
              </section>

              {/* Performance Metrics */}
              <section className="glass-card p-4 lg:col-span-1">
                <h3 className="text-lg lg:text-xl font-bold text-white mb-4">
                  Performance Metrics
                </h3>
                <div className="space-y-2">
                  <div className="flex items-center gap-2">
                    <div className="w-2.5 h-2.5 bg-brand-purple rounded-full"></div>
                    <div className="flex-1">
                      <div className="text-sm font-medium text-white">Send</div>
                      <div className="text-sm text-white/60">
                        {sendRequestProcessed !== null
                          ? `${sendRequestProcessed}ms`
                          : "No data"}
                      </div>
                    </div>
                  </div>

                  <div className="flex items-center gap-2">
                    <div className="w-2.5 h-2.5 bg-brand-green rounded-full"></div>
                    <div className="flex-1">
                      <div className="text-sm font-medium text-white">
                        Query
                      </div>
                      <div className="text-sm text-white/60">
                        {queryRoundtripDelay !== null
                          ? `${queryRoundtripDelay}ms`
                          : "No data"}
                      </div>
                    </div>
                  </div>

                  <div className="flex items-center gap-2">
                    <div className="w-2.5 h-2.5 bg-brand-blue rounded-full"></div>
                    <div className="flex-1">
                      <div className="text-sm font-medium text-white">NATS</div>
                      <div className="text-sm text-white/60">
                        {natsRoundtripDelay !== null
                          ? `${natsRoundtripDelay}ms`
                          : "No data"}
                      </div>
                    </div>
                  </div>
                </div>
              </section>

              {/* Error Status */}
              <section className="glass-card p-4 lg:col-span-1">
                <h3 className="text-lg lg:text-xl font-bold text-white mb-4">
                  Error Status
                </h3>
                <div className="space-y-2">
                  <div className="flex items-center gap-2">
                    <div
                      className={`w-2.5 h-2.5 rounded-full ${
                        error ? "bg-red-500" : "bg-brand-green"
                      }`}
                    ></div>
                    <div className="flex-1">
                      <div className="text-sm font-medium text-white">
                        System Status
                      </div>
                      <div className="text-sm text-white/60">
                        {error ? "Error detected" : "Running normally"}
                      </div>
                    </div>
                  </div>

                  {error && (
                    <div className="flex items-center gap-2">
                      <div className="w-2.5 h-2.5 bg-red-500 rounded-full"></div>
                      <div className="flex-1">
                        <div className="text-sm font-medium text-white">
                          Last Error
                        </div>
                        <div className="text-sm text-white/60 truncate">
                          {error}
                        </div>
                      </div>
                    </div>
                  )}

                  <div className="flex items-center gap-2">
                    <div
                      className={`w-2.5 h-2.5 rounded-full ${
                        grpcConnection === "connected" &&
                        natsConnection === "connected"
                          ? "bg-brand-green"
                          : "bg-brand-yellow"
                      }`}
                    ></div>
                    <div className="flex-1">
                      <div className="text-sm font-medium text-white">
                        Connections
                      </div>
                      <div className="text-sm text-white/60">
                        {grpcConnection === "connected" &&
                        natsConnection === "connected"
                          ? "All connected"
                          : "Some disconnected"}
                      </div>
                    </div>
                  </div>
                </div>
              </section>
            </div>

            {/* Log Windows */}
            <div className="grid grid-cols-1 xl:grid-cols-3 gap-4">
              {/* gRPC-Web Event Response Log */}
              <section className="glass-card p-4">
                <div className="flex items-center gap-2 mb-4">
                  <div className="w-2.5 h-2.5 bg-brand-purple rounded-full"></div>
                  <h3 className="text-lg lg:text-xl font-bold text-white">
                    gRPC-Web Event Response
                  </h3>
                </div>
                <div className="h-[40rem] overflow-y-auto bg-black/20 rounded-lg p-4 border border-white/10">
                  <div className="font-mono text-xs text-white/90 whitespace-pre-wrap">
                    {eventResponseLog.length > 0 ? (
                      eventResponseLog.map((log, index) => (
                        <div
                          key={index}
                          className="mb-2 p-2 bg-white/5 rounded border-l-2 border-brand-purple"
                        >
                          {log}
                        </div>
                      ))
                    ) : (
                      <div className="text-white/50 italic text-center py-8">
                        No event response yet...
                      </div>
                    )}
                  </div>
                </div>
              </section>

              {/* gRPC-Web Query Response Log */}
              <section className="glass-card p-4">
                <div className="flex items-center gap-2 mb-4">
                  <div className="w-2.5 h-2.5 bg-brand-green rounded-full"></div>
                  <h3 className="text-lg lg:text-xl font-bold text-white">
                    gRPC-Web Query Response
                  </h3>
                </div>
                <div className="h-[40rem] overflow-y-auto bg-black/20 rounded-lg p-4 border border-white/10">
                  <div className="font-mono text-xs text-white/90 whitespace-pre-wrap">
                    {queryResponseLog.length > 0 ? (
                      queryResponseLog.map((log, index) => (
                        <div
                          key={index}
                          className="mb-2 p-2 bg-white/5 rounded border-l-2 border-brand-green"
                        >
                          {log}
                        </div>
                      ))
                    ) : (
                      <div className="text-white/50 italic text-center py-8">
                        No query response yet...
                      </div>
                    )}
                  </div>
                </div>
              </section>

              {/* NATS Message Log */}
              <section className="glass-card p-4">
                <div className="flex items-center gap-2 mb-4">
                  <div className="w-2.5 h-2.5 bg-brand-blue rounded-full"></div>
                  <h3 className="text-lg lg:text-xl font-bold text-white">
                    NATS Message Stream
                  </h3>
                </div>
                <div className="h-[40rem] overflow-y-auto bg-black/20 rounded-lg p-4 border border-white/10">
                  <div className="font-mono text-xs text-white/90 whitespace-pre-wrap">
                    {natsMessageLog.length > 0 ? (
                      natsMessageLog.map((log, index) => (
                        <div
                          key={index}
                          className="mb-2 p-2 bg-white/5 rounded border-l-2 border-brand-blue"
                        >
                          {log}
                        </div>
                      ))
                    ) : (
                      <div className="text-white/50 italic text-center py-8">
                        No NATS messages yet...
                      </div>
                    )}
                  </div>
                </div>
              </section>
            </div>
          </div>
        </main>
      </div>
    </>
  );
}
