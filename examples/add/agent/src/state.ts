import { TransitionData } from "./transition.js";
import { AddMap, AddProgramState } from "./circuit.js";
import { createClient } from "@connectrpc/connect";
import {
  CoordinatorService,
  ReadDataAvailabilityRequestSchema,
} from "./proto/silvana/coordinator/v1/coordinator_pb.js";
import { create } from "@bufbuild/protobuf";

export interface SequenceState {
  sequence: number;
  transition: TransitionData;
  dataAvailability?: string;
}

export async function getState(
  sequenceStates: SequenceState[],
  client: ReturnType<typeof createClient<typeof CoordinatorService>>,
  sessionId: string
): Promise<
  | {
      state: AddProgramState;
      map: AddMap;
    }
  | undefined
> {
  if (sequenceStates.length === 0) {
    return undefined;
  }

  // Sort sequence states by sequence number (lowest first)
  const sortedStates = sequenceStates.sort((a, b) => a.sequence - b.sequence);

  // Since sequence 0 (initial state) has no transition data, sequenceStates start with sequence 1
  // If sequence 1 has no data_availability, we still need to create initial state for sequence 0
  if (sortedStates[0].sequence === 1 && !sortedStates[0].dataAvailability) {
    // Create initial state for sequence 0
    console.log(
      `Sequence 1 has no data availability, creating initial state for sequence 0`
    );
    const { state, map } = AddProgramState.create();
    return { state, map };
  }

  // For sequences with data availability, read from Walrus
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
        const { state, map } = AddProgramState.deserialize(
          readDataResponse.data
        );
        return { state, map };
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
  }

  // No data availability, cannot deserialize state
  console.warn(`No data availability for sequence ${sortedStates[0].sequence}`);
  return undefined;
}
