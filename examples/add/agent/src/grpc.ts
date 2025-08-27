import { createGrpcTransport } from "@connectrpc/connect-node";
import { createClient } from "@connectrpc/connect";
import {
  CoordinatorService,
  RetrieveSecretRequestSchema,
  GetJobRequestSchema,
  CompleteJobRequestSchema,
  FailJobRequestSchema,
  GetSequenceStatesRequestSchema,
  SubmitProofRequestSchema,
  SubmitStateRequestSchema,
  GetProofRequestSchema,
  GetBlockProofRequestSchema,
  ReadDataAvailabilityRequestSchema,
  type GetJobResponse,
  type CompleteJobResponse,
  type FailJobResponse,
  type GetSequenceStatesResponse,
  type SubmitProofResponse,
  type SubmitStateResponse,
  type GetProofResponse,
  type GetBlockProofResponse,
  type ReadDataAvailabilityResponse,
} from "./proto/silvana/coordinator/v1/coordinator_pb.js";
import { create } from "@bufbuild/protobuf";

// Static client instance to be reused
let coordinatorClient: ReturnType<
  typeof createClient<typeof CoordinatorService>
> | null = null;

// Environment variables - cached after first initialization
let sessionId: string | null = null;
let jobId: string | null = null;
let chain: string | null = null;
let coordinatorId: string | null = null;
let sessionPrivateKey: string | null = null;
let developer: string | null = null;
let agent: string | null = null;
let agentMethod: string | null = null;

/**
 * Gets the coordinator client instance and environment variables, initializing if necessary
 * @returns Object containing the client and all required environment variables
 */
function getCoordinatorClient(): {
  client: ReturnType<typeof createClient<typeof CoordinatorService>>;
  sessionId: string;
  chain: string;
  coordinatorId: string;
  sessionPrivateKey: string;
  developer: string;
  agent: string;
  agentMethod: string;
} {
  if (coordinatorClient === null) {
    // Read all environment variables
    sessionId = process.env.SESSION_ID || null;
    chain = process.env.CHAIN || null;
    coordinatorId = process.env.COORDINATOR_ID || null;
    sessionPrivateKey = process.env.SESSION_PRIVATE_KEY || null;
    developer = process.env.DEVELOPER || null;
    agent = process.env.AGENT || null;
    agentMethod = process.env.AGENT_METHOD || null;

    // Check for required environment variables
    if (!sessionId) {
      throw new Error("SESSION_ID environment variable is required");
    }
    // if (!chain) {
    //   throw new Error("CHAIN environment variable is required");
    // }
    // if (!coordinatorId) {
    //   throw new Error("COORDINATOR_ID environment variable is required");
    // }
    // if (!sessionPrivateKey) {
    //   throw new Error("SESSION_PRIVATE_KEY environment variable is required");
    // }
    if (!developer) {
      throw new Error("DEVELOPER environment variable is required");
    }
    if (!agent) {
      throw new Error("AGENT environment variable is required");
    }
    if (!agentMethod) {
      throw new Error("AGENT_METHOD environment variable is required");
    }

    // Create gRPC client over TCP (accessible from Docker container)
    const transport = createGrpcTransport({
      baseUrl: "http://host.docker.internal:50051",
    });

    coordinatorClient = createClient(CoordinatorService, transport);
  }

  // At this point, all values are guaranteed to be non-null due to the checks above
  return {
    client: coordinatorClient as ReturnType<
      typeof createClient<typeof CoordinatorService>
    >,
    sessionId: sessionId as string,
    chain: chain as string,
    coordinatorId: coordinatorId as string,
    sessionPrivateKey: sessionPrivateKey as string,
    developer: developer as string,
    agent: agent as string,
    agentMethod: agentMethod as string,
  };
}

/**
 * Retrieves a secret value from the coordinator service
 * @param key The name/key of the secret to retrieve
 * @returns The secret value if found, null otherwise
 */
export async function getSecret(key: string): Promise<string | null> {
  if (!jobId) {
    throw new Error("Call getJob() first");
  }
  try {
    const { client, sessionId } = getCoordinatorClient();

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

/**
 * Gets a job from the coordinator
 */
export async function getJob(): Promise<GetJobResponse> {
  const { client, sessionId, developer, agent, agentMethod } =
    getCoordinatorClient();

  const request = create(GetJobRequestSchema, {
    developer,
    agent,
    agentMethod,
    sessionId,
  });

  const response = await client.getJob(request);

  if (response.job) {
    jobId = response.job.jobId;
  }

  return response;
}

/**
 * Completes a job
 */
export async function completeJob(): Promise<CompleteJobResponse> {
  if (!jobId) {
    throw new Error("Call getJob() first");
  }
  const { client, sessionId } = getCoordinatorClient();

  const request = create(CompleteJobRequestSchema, {
    sessionId,
    jobId,
  });

  return await client.completeJob(request);
}

/**
 * Fails a job
 */
export async function failJob(errorMessage: string): Promise<FailJobResponse> {
  if (!jobId) {
    throw new Error("Call getJob() first");
  }
  const { client, sessionId } = getCoordinatorClient();

  const request = create(FailJobRequestSchema, {
    sessionId,
    jobId,
    errorMessage,
  });

  return await client.failJob(request);
}

/**
 * Gets sequence states
 */
export async function getSequenceStates(
  sequence: bigint
): Promise<GetSequenceStatesResponse> {
  if (!jobId) {
    throw new Error("Call getJob() first");
  }
  const { client, sessionId } = getCoordinatorClient();

  const request = create(GetSequenceStatesRequestSchema, {
    sessionId,
    jobId,
    sequence,
  });

  return await client.getSequenceStates(request);
}

/**
 * Submits a proof
 */
export async function submitProof(
  blockNumber: bigint,
  sequences: bigint[],
  proof: string,
  cpuTime: bigint,
  mergedSequences1?: bigint[],
  mergedSequences2?: bigint[]
): Promise<SubmitProofResponse> {
  if (!jobId) {
    throw new Error("Call getJob() first");
  }
  const { client, sessionId } = getCoordinatorClient();

  const request = create(SubmitProofRequestSchema, {
    sessionId,
    jobId,
    blockNumber,
    sequences,
    proof,
    cpuTime,
    mergedSequences1: mergedSequences1 || [],
    mergedSequences2: mergedSequences2 || [],
  });

  return await client.submitProof(request);
}

/**
 * Submits state
 */
export async function submitState(
  sequence: bigint,
  newStateData?: Uint8Array,
  serializedState?: string
): Promise<SubmitStateResponse> {
  if (!jobId) {
    throw new Error("Call getJob() first");
  }
  const { client, sessionId } = getCoordinatorClient();

  const request = create(SubmitStateRequestSchema, {
    sessionId,
    jobId,
    sequence,
    newStateData,
    serializedState,
  });

  return await client.submitState(request);
}

/**
 * Gets a proof
 */
export async function getProof(
  blockNumber: bigint,
  sequences: bigint[]
): Promise<GetProofResponse> {
  if (!jobId) {
    throw new Error("Call getJob() first");
  }
  const { client, sessionId } = getCoordinatorClient();

  const request = create(GetProofRequestSchema, {
    sessionId,
    blockNumber,
    sequences,
    jobId,
  });

  return await client.getProof(request);
}

/**
 * Gets a block proof
 */
export async function getBlockProof(
  blockNumber: bigint
): Promise<GetBlockProofResponse> {
  if (!jobId) {
    throw new Error("Call getJob() first");
  }
  const { client, sessionId } = getCoordinatorClient();

  const request = create(GetBlockProofRequestSchema, {
    sessionId,
    blockNumber,
    jobId,
  });

  return await client.getBlockProof(request);
}

/**
 * Reads data availability
 */
export async function readDataAvailability(
  daHash: string
): Promise<ReadDataAvailabilityResponse> {
  if (!jobId) {
    throw new Error("Call getJob() first");
  }
  const { client, sessionId } = getCoordinatorClient();

  const request = create(ReadDataAvailabilityRequestSchema, {
    sessionId,
    daHash,
  });

  return await client.readDataAvailability(request);
}
