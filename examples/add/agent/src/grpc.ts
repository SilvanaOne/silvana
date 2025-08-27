import { createGrpcTransport } from "@connectrpc/connect-node";
import { createClient } from "@connectrpc/connect";
import {
  CoordinatorService,
  RetrieveSecretRequestSchema,
} from "./proto/silvana/coordinator/v1/coordinator_pb.js";
import { create } from "@bufbuild/protobuf";

// Static client instance to be reused
let coordinatorClient: ReturnType<typeof createClient<typeof CoordinatorService>> | null = null;
let sessionId: string | null = null;
let jobId: string | null = null;

/**
 * Gets the coordinator client instance, initializing if necessary
 * @returns Object containing the client, sessionId, and jobId
 */
function getCoordinatorClient() {
  if (coordinatorClient === null) {
    // Check for required environment variables
    sessionId = process.env.SESSION_ID || null;
    jobId = process.env.JOB_ID || null;

    if (!sessionId) {
      throw new Error("SESSION_ID environment variable is required");
    }

    if (!jobId) {
      throw new Error("JOB_ID environment variable is required");
    }

    // Create gRPC client over TCP (accessible from Docker container)
    const transport = createGrpcTransport({
      baseUrl: "http://host.docker.internal:50051",
    });
    
    coordinatorClient = createClient(CoordinatorService, transport);
  }

  // At this point, all values are guaranteed to be non-null due to the checks above
  return {
    client: coordinatorClient as ReturnType<typeof createClient<typeof CoordinatorService>>,
    sessionId: sessionId as string,
    jobId: jobId as string
  };
}

/**
 * Retrieves a secret value from the coordinator service
 * @param key The name/key of the secret to retrieve
 * @returns The secret value if found, null otherwise
 */
export async function getSecret(key: string): Promise<string | null> {
  try {
    const { client, sessionId, jobId } = getCoordinatorClient();

    // Create the request
    const request = create(RetrieveSecretRequestSchema, {
      jobId: jobId,
      sessionId: sessionId,
      name: key,
    });

    console.log(`Retrieving secret: ${key}`);

    // Make the gRPC call
    const response = await client.retrieveSecret(request);

    if (response.success && response.secretValue !== undefined) {
      console.log(` Successfully retrieved secret: ${key}`);
      return response.secretValue;
    } else {
      console.log(`L Failed to retrieve secret: ${key} - ${response.message}`);
      return null;
    }
  } catch (error: any) {
    console.error(`Error retrieving secret '${key}':`, error.message);
    return null;
  }
}