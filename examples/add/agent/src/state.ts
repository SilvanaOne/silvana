import { TransitionData } from "./transition.js";
import {
  AddMap,
  AddProgramState,
  AddProgram,
  AddProgramProof,
} from "./circuit.js";
import { AddProgramCommitment } from "./commitment.js";
import { createClient } from "@connectrpc/connect";
import {
  CoordinatorService,
  ReadDataAvailabilityRequestSchema,
} from "./proto/silvana/coordinator/v1/coordinator_pb.js";
import { create } from "@bufbuild/protobuf";
import { UInt32, Field, VerificationKey, Cache, verify } from "o1js";
import { processCommitments } from "./transition.js";

export interface SequenceState {
  sequence: number;
  transition: TransitionData;
  dataAvailability?: string;
}

let vk: VerificationKey | undefined = undefined;

export async function compile(): Promise<VerificationKey> {
  if (vk === undefined) {
    const cache = Cache.FileSystem("./cache");
    console.log("compiling...");
    console.time("compiled");
    vk = (await AddProgram.compile({ cache })).verificationKey;
    console.timeEnd("compiled");
    console.log("vk", vk.hash.toJSON());
  }
  return vk;
}

export async function getStateAndProof(params: {
  sequenceStates: SequenceState[];
  client: ReturnType<typeof createClient<typeof CoordinatorService>>;
  sessionId: string;
  sequence: bigint;
}): Promise<
  | {
      state: AddProgramState;
      map: AddMap;
      proof?: AddProgramProof;
    }
  | undefined
> {
  const { sequenceStates, client, sessionId, sequence } = params;

  if (sequenceStates.length === 0) {
    return undefined;
  }

  // Sort sequence states by sequence number (lowest first)
  const sortedStates = sequenceStates.sort((a, b) => a.sequence - b.sequence);

  // Initialize state and map - either from data availability or create new
  let state: AddProgramState;
  let map: AddMap;

  // Check if the first sequence state has data availability
  if (sortedStates[0].dataAvailability) {
    try {
      console.log(
        `Reading data availability for hash: ${sortedStates[0].dataAvailability}`
      );

      const readDataRequest = create(ReadDataAvailabilityRequestSchema, {
        daHash: sortedStates[0].dataAvailability,
        sessionId: sessionId,
      });

      const readDataResponse = await client.readDataAvailability(
        readDataRequest
      );

      if (readDataResponse.success && readDataResponse.data) {
        console.log(
          `Successfully read data from Walrus, deserializing state...`
        );
        const deserializedState = AddProgramState.deserialize(
          readDataResponse.data
        );
        state = deserializedState.state;
        map = deserializedState.map;
        console.log(
          `Loaded state from data availability: sequence ${state.sequence.toBigInt()}, sum ${state.sum.toBigInt()}`
        );
      } else {
        console.error(
          `Failed to read data from Walrus: ${readDataResponse.message}`
        );
        return undefined;
      }
    } catch (error) {
      console.error(`Error reading data availability:`, error);
      return undefined;
    }
  } else {
    // Create initial state (sequence 0)
    const initialState = AddProgramState.create();
    state = initialState.state;
    map = initialState.map;
    console.log(
      `Created initial state: sequence ${state.sequence.toBigInt()}, sum ${state.sum.toBigInt()}`
    );
  }

  // Process all sequence states sequentially
  let finalProof: AddProgramProof | undefined = undefined;
  let startProcessingFromIndex = 0;

  // If we loaded state from data availability, skip the first sequence state
  // as it's already applied to the loaded state
  if (sortedStates[0].dataAvailability) {
    startProcessingFromIndex = 1;
    console.log(
      `Skipping sequence ${sortedStates[0].sequence} as it's already applied in loaded state`
    );
  }

  for (let i = startProcessingFromIndex; i < sortedStates.length; i++) {
    const sequenceState = sortedStates[i];
    const transitionData = sequenceState.transition;
    const currentSequence = BigInt(sequenceState.sequence);

    // Determine if this is the sequence we need to prove
    const shouldProve = currentSequence === sequence;
    if (shouldProve) {
      const vk = await compile();
      if (vk === undefined) {
        throw new Error("vk is not set");
      }
    }

    console.log(
      `Processing sequence ${currentSequence}, method: ${transitionData.method}, shouldProve: ${shouldProve}`
    );

    // Process commitments for this transition
    const commitments = processCommitments(transitionData);

    // Apply the operation based on method type
    if (transitionData.method === "add") {
      if (shouldProve) {
        // Generate proof for this sequence
        console.time(`proving add for sequence ${currentSequence}`);
        const proofResult = await AddProgram.add(
          state,
          UInt32.from(transitionData.index),
          Field(transitionData.old_value),
          Field(transitionData.value),
          map,
          new AddProgramCommitment(commitments.oldCommitment),
          new AddProgramCommitment(commitments.newCommitment)
        );
        console.timeEnd(`proving add for sequence ${currentSequence}`);
        finalProof = proofResult.proof;
        state = proofResult.proof.publicOutput;
        map = proofResult.auxiliaryOutput;
      } else {
        // Use rawMethods for non-proving sequences
        const result = await AddProgram.rawMethods.add(
          state,
          UInt32.from(transitionData.index),
          Field(transitionData.old_value),
          Field(transitionData.value),
          map,
          new AddProgramCommitment(commitments.oldCommitment),
          new AddProgramCommitment(commitments.newCommitment)
        );
        state = result.publicOutput;
        map = result.auxiliaryOutput;
      }
    } else if (transitionData.method === "multiply") {
      if (shouldProve) {
        // Generate proof for this sequence
        console.time(`proving multiply for sequence ${currentSequence}`);
        const proofResult = await AddProgram.multiply(
          state,
          UInt32.from(transitionData.index),
          Field(transitionData.old_value),
          Field(transitionData.value),
          map,
          new AddProgramCommitment(commitments.oldCommitment),
          new AddProgramCommitment(commitments.newCommitment)
        );
        console.timeEnd(`proving multiply for sequence ${currentSequence}`);
        finalProof = proofResult.proof;
        state = proofResult.proof.publicOutput;
        map = proofResult.auxiliaryOutput;
      } else {
        // Use rawMethods for non-proving sequences
        const result = await AddProgram.rawMethods.multiply(
          state,
          UInt32.from(transitionData.index),
          Field(transitionData.old_value),
          Field(transitionData.value),
          map,
          new AddProgramCommitment(commitments.oldCommitment),
          new AddProgramCommitment(commitments.newCommitment)
        );
        state = result.publicOutput;
        map = result.auxiliaryOutput;
      }
    } else {
      throw new Error(`Unsupported method: ${transitionData.method}`);
    }

    if (shouldProve) {
      const proof = finalProof;
      if (proof) {
        if (vk === undefined) {
          throw new Error("vk is not set");
        }
        console.time(`verifying proof for sequence ${currentSequence}`);
        const ok = await verify(proof, vk);
        console.timeEnd(`verifying proof for sequence ${currentSequence}`);
        if (!ok) {
          throw new Error(
            `Proof verification failed for sequence ${currentSequence}`
          );
        } else {
          console.log(
            `Proof verification passed for sequence ${currentSequence}`
          );
        }
      } else {
        throw new Error(
          `No proof found for sequence ${currentSequence}, shouldProve: ${shouldProve}`
        );
      }
    }

    console.log(
      `Completed sequence ${currentSequence}, new sum: ${state.sum.toBigInt()}`
    );
  }

  return {
    state,
    map,
    proof: finalProof,
  };
}
