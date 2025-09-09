import { AddProgramProof, AddProgramState, AddMap } from "./circuit.js";

/**
 * Serialized proof and state data
 */
export interface SerializedProofAndState {
  proof: string;
  state: string;
}

/**
 * Serialize proof and state for submission
 * @param proof - The AddProgramProof to serialize
 * @param state - The AddProgramState to serialize
 * @param map - The AddMap to serialize
 * @returns Serialized proof and state as JSON string
 */
export function serializeProofAndState(
  proof: AddProgramProof,
  state: AddProgramState,
  map: AddMap
): string {
  const serializedProof = proof.toJSON();
  const serializedState = state.serialize(map);
  
  const proofAndState = {
    proof: JSON.stringify(serializedProof),
    state: serializedState,
  };
  
  return JSON.stringify(proofAndState);
}

/**
 * Serialize just the state (without proof) for submission
 * @param state - The AddProgramState to serialize
 * @param map - The AddMap to serialize
 * @returns Serialized state as string
 */
export function serializeState(
  state: AddProgramState,
  map: AddMap
): string {
  return state.serialize(map);
}

/**
 * Deserialize just the state data
 * @param serializedState - The serialized state data as string
 * @returns Deserialized state and map
 * @throws Error if deserialization fails
 */
export function deserializeState(serializedState: string): {
  state: AddProgramState;
  map: AddMap;
} {
  return AddProgramState.deserialize(serializedState);
}

/**
 * Deserialize proof and state data
 * @param serializedString - The serialized proof and state data as JSON string
 * @returns Deserialized proof and state
 * @throws Error if JSON parsing fails or data is invalid
 */
export async function deserializeProofAndState(serializedString: string): Promise<{
  proof: AddProgramProof;
  state: AddProgramState;
  map: AddMap;
}> {
  try {
    const serialized: SerializedProofAndState = JSON.parse(serializedString);
    
    if (!serialized.proof || !serialized.state) {
      throw new Error("Invalid serialized data: missing proof or state");
    }
    
    const proofData = JSON.parse(serialized.proof);
    const proof = await AddProgramProof.fromJSON(proofData);
    
    const { state, map } = AddProgramState.deserialize(serialized.state);
    
    return {
      proof,
      state,
      map,
    };
  } catch (error) {
    if (error instanceof SyntaxError) {
      throw new Error(`Failed to parse JSON: ${error.message}`);
    }
    throw error;
  }
}