import { TransitionData } from "./transition.js";
import {
  AddMap,
  AddProgramState,
  AddProgram,
  AddProgramProof,
} from "./circuit.js";
import { AddProgramCommitment } from "./commitment.js";
import { readDataAvailability } from "./grpc.js";
import {
  UInt32,
  Field,
  verify,
  JsonProof,
  UInt64,
  VerificationKey,
} from "o1js";
import { processCommitments } from "./transition.js";
import { compile } from "./compile.js";

export interface SequenceState {
  sequence: number;
  transition: TransitionData;
  dataAvailability?: string;
}

export async function merge(
  proof1Serialized: string,
  proof2Serialized: string
): Promise<string> {
  console.log("Starting proof merge...");
  console.log(`Proof 1 size: ${proof1Serialized.length} chars`);
  console.log(`Proof 2 size: ${proof2Serialized.length} chars`);

  // Ensure the circuit is compiled
  const { vkProgram } = await compile();
  if (!vkProgram) {
    throw new Error("Failed to compile circuit for merging");
  }

  // Deserialize the proofs
  console.log("Deserializing proof 1...");
  const proof1: AddProgramProof = await AddProgramProof.fromJSON(
    JSON.parse(proof1Serialized) as JsonProof
  );

  console.log("Deserializing proof 2...");
  const proof2: AddProgramProof = await AddProgramProof.fromJSON(
    JSON.parse(proof2Serialized) as JsonProof
  );

  // Verify both proofs before merging
  console.log("Verifying proof 1...");
  const ok1 = await verify(proof1, vkProgram);
  if (!ok1) {
    throw new Error("Proof 1 verification failed");
  }
  console.log("Proof 1 verified");

  console.log("Verifying proof 2...");
  const ok2 = await verify(proof2, vkProgram);
  if (!ok2) {
    throw new Error("Proof 2 verification failed");
  }
  console.log("Proof 2 verified");

  // Merge the proofs
  console.time("merging proofs");
  const mergedProof = await AddProgram.merge(
    proof1.publicInput,
    proof1,
    proof2
  );
  console.timeEnd("merging proofs");

  // Verify the merged proof
  console.log("Verifying merged proof...");
  const okMerged = await verify(mergedProof.proof, vkProgram);
  if (!okMerged) {
    throw new Error("Merged proof verification failed");
  }
  console.log("Merged proof verified");

  // Serialize the merged proof
  const mergedProofSerialized = JSON.stringify({
    proof: JSON.stringify(mergedProof.proof.toJSON()),
  });
  console.log(`Merged proof size: ${mergedProofSerialized.length} chars`);

  return mergedProofSerialized;
}

export async function getStateAndProof(params: {
  sequenceStates: SequenceState[];
  sequence: bigint;
  blockNumber: bigint;
}): Promise<
  | {
      state: AddProgramState;
      map: AddMap;
      proof?: AddProgramProof;
    }
  | undefined
> {
  const { sequenceStates, sequence, blockNumber } = params;

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

      const readDataResponse = await readDataAvailability(
        sortedStates[0].dataAvailability
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
  let vkProgram: VerificationKey | undefined = undefined;

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
      const { vkProgram } = await compile();
      if (vkProgram === undefined) {
        throw new Error("vkProgram is not set");
      }
    }

    console.log(
      `Processing sequence ${currentSequence}, method: ${transitionData.method}, shouldProve: ${shouldProve}`
    );

    // Process commitments for this transition
    const commitments = processCommitments(transitionData);
    if (shouldProve) {
      console.log(`Setting block number to ${blockNumber}`);
      state.blockNumber = UInt64.from(blockNumber);
      console.log(`State block number: ${state.blockNumber.toBigInt()}`);
    }

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
        if (vkProgram === undefined) {
          throw new Error("vk is not set");
        }
        console.time(`verifying proof for sequence ${currentSequence}`);
        const ok = await verify(proof, vkProgram);
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
