import { bcs } from "@mysten/sui/bcs";

/**
 * ProofMergeData struct matching the Move definition in prover.move
 *
 * Move struct:
 * public struct ProofMergeData has copy, drop {
 *     block_number: u64,
 *     sequences1: vector<u64>,
 *     sequences2: vector<u64>,
 * }
 */

export const ProofMergeDataBcs = bcs.struct("ProofMergeData", {
  block_number: bcs.u64(),
  sequences1: bcs.vector(bcs.u64()),
  sequences2: bcs.vector(bcs.u64()),
});

/**
 * Raw ProofMergeData as returned by BCS deserialization
 */
export interface RawProofMergeData {
  block_number: string; // BCS returns u64 as string
  sequences1: string[]; // BCS returns vector<u64> as string[]
  sequences2: string[]; // BCS returns vector<u64> as string[]
}

/**
 * Processed ProofMergeData with numeric types for operations
 */
export interface ProofMergeData {
  block_number: bigint; // u64 as bigint for numeric operations
  sequences1: bigint[]; // vector<u64> as bigint[] for numeric operations
  sequences2: bigint[]; // vector<u64> as bigint[] for numeric operations
}

/**
 * Deserializes raw ProofMergeData from BCS bytes
 * @param data - The BCS serialized data as byte array or Uint8Array
 * @returns Raw ProofMergeData object (with string representations)
 */
export function deserializeRawProofMergeData(data: number[] | Uint8Array): RawProofMergeData {
  const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
  return ProofMergeDataBcs.parse(bytes);
}

/**
 * Deserializes ProofMergeData from BCS bytes and converts to numeric types
 * @param data - The BCS serialized data as byte array or Uint8Array
 * @returns Processed ProofMergeData object with bigint types
 */
export function deserializeProofMergeData(data: number[] | Uint8Array): ProofMergeData {
  const rawProofMergeData = deserializeRawProofMergeData(data);

  return {
    block_number: BigInt(rawProofMergeData.block_number),
    sequences1: rawProofMergeData.sequences1.map(seq => BigInt(seq)),
    sequences2: rawProofMergeData.sequences2.map(seq => BigInt(seq)),
  };
}

/**
 * Serializes ProofMergeData to BCS bytes
 * @param proofMergeData - The ProofMergeData object to serialize
 * @returns BCS serialized bytes
 */
export function serializeProofMergeData(proofMergeData: ProofMergeData): Uint8Array {
  // Convert back to raw format for serialization
  const rawProofMergeData: RawProofMergeData = {
    block_number: proofMergeData.block_number.toString(),
    sequences1: proofMergeData.sequences1.map(seq => seq.toString()),
    sequences2: proofMergeData.sequences2.map(seq => seq.toString()),
  };

  return ProofMergeDataBcs.serialize(rawProofMergeData).toBytes();
}

/**
 * Display ProofMergeData in a readable format
 * @param proofMergeData - The ProofMergeData object to display
 */
export function displayProofMergeData(proofMergeData: ProofMergeData): void {
  console.log(`ProofMergeData:`);
  console.log(`  block_number: ${proofMergeData.block_number}`);
  console.log(`  sequences1: [${proofMergeData.sequences1.join(', ')}]`);
  console.log(`  sequences2: [${proofMergeData.sequences2.join(', ')}]`);
  console.log(`  Total sequences to merge: ${proofMergeData.sequences1.length + proofMergeData.sequences2.length}`);
}