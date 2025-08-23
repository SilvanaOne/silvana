import { bcs } from "@mysten/sui/bcs";
import { UInt64 } from "o1js";
import { scalar, rScalarPow } from "@silvana-one/mina-utils";
import type { CanonicalElement } from "@silvana-one/mina-utils";

/**
 * CommitmentData struct matching the Move definition in state.move
 *
 * Move struct:
 * public struct CommitmentData has copy, drop {
 *     actions_commitment: Element<Scalar>,
 *     actions_sequence: u64,
 *     state_commitment: Element<Scalar>,
 * }
 */

/**
 * JobData struct matching the Move definition in main.move
 *
 * Move struct:
 * public struct JobData has copy, drop {
 *     index: u32,
 *     value: u256,
 *     old_value: u256,
 *     old_commitment: CommitmentData,
 *     new_commitment: CommitmentData,
 *     block_number: u64,
 *     sequence: u64,
 * }
 */

// Element<Scalar> is a struct with bytes field (from group_ops.move line 22-24)
const ElementScalar = bcs.struct("Element", {
  bytes: bcs.vector(bcs.u8())
});

export const CommitmentDataBcs = bcs.struct("CommitmentData", {
  actions_commitment: ElementScalar,
  actions_sequence: bcs.u64(),
  state_commitment: ElementScalar,
});

export const JobDataBcs = bcs.struct("JobData", {
  index: bcs.u32(),
  value: bcs.u256(),
  old_value: bcs.u256(),
  old_commitment: CommitmentDataBcs,
  new_commitment: CommitmentDataBcs,
  block_number: bcs.u64(),
  sequence: bcs.u64(),
});

/**
 * Raw CommitmentData as returned by BCS deserialization
 */
export interface RawCommitmentData {
  actions_commitment: { bytes: number[] }; // Element struct with bytes vector
  actions_sequence: string; // BCS returns u64 as string
  state_commitment: { bytes: number[] }; // Element struct with bytes vector
}

/**
 * Raw JobData as returned by BCS deserialization
 */
export interface RawJobData {
  index: number;
  value: string; // BCS returns u256 as string
  old_value: string; // BCS returns u256 as string
  old_commitment: RawCommitmentData;
  new_commitment: RawCommitmentData;
  block_number: string; // BCS returns u64 as string
  sequence: string; // BCS returns u64 as string
}

/**
 * Processed CommitmentData with provable commitment representations
 */
export interface CommitmentData {
  actions_commitment: CanonicalElement; // Provable scalar
  actions_sequence: bigint; // u64 as bigint for numeric operations
  state_commitment: CanonicalElement; // Provable scalar
}

/**
 * Processed JobData with provable commitment representations
 */
export interface JobData {
  index: number;
  value: bigint; // u256 as bigint for numeric operations
  old_value: bigint; // u256 as bigint for numeric operations
  old_commitment: CommitmentData;
  new_commitment: CommitmentData;
  block_number: bigint; // u64 as bigint for numeric operations
  sequence: bigint; // u64 as bigint for numeric operations
}

/**
 * Convert commitment bytes to bigint for scalar operations
 * BLS12-381 scalars are encoded in big-endian format
 * @param bytes - Byte array representing Element<Scalar>
 * @returns BigInt representation
 */
export function commitmentArrayToBigInt(bytes: number[]): bigint {
  let result = 0n;
  for (let i = 0; i < bytes.length; i++) {
    result = result * 256n + BigInt(bytes[i]);
  }
  return result;
}

/**
 * Convert bigint back to commitment byte array
 * @param value - BigInt representation
 * @returns 32-byte array for BCS compatibility
 */
export function bigIntToCommitmentArray(value: bigint): number[] {
  const bytes: number[] = new Array(32).fill(0);
  let remaining = value;

  for (let i = 31; i >= 0; i--) {
    bytes[i] = Number(remaining & 0xffn);
    remaining = remaining >> 8n;
  }

  return bytes;
}

/**
 * Deserializes raw JobData from BCS bytes
 * @param data - The BCS serialized data as byte array or Uint8Array
 * @returns Raw JobData object (with number[] commitments)
 */
export function deserializeRawJobData(data: number[] | Uint8Array): RawJobData {
  const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
  return JobDataBcs.parse(bytes);
}

/**
 * Deserializes JobData from BCS bytes and converts commitments to provable form
 * @param data - The BCS serialized data as byte array or Uint8Array
 * @returns Processed JobData object with Fr.Canonical.provable commitments
 */
export function deserializeJobData(data: number[] | Uint8Array): JobData {
  const rawJobData = deserializeRawJobData(data);

  return {
    index: rawJobData.index,
    value: BigInt(rawJobData.value),
    old_value: BigInt(rawJobData.old_value),
    old_commitment: {
      actions_commitment: scalar(
        commitmentArrayToBigInt(rawJobData.old_commitment.actions_commitment.bytes)
      ),
      actions_sequence: BigInt(rawJobData.old_commitment.actions_sequence),
      state_commitment: scalar(
        commitmentArrayToBigInt(rawJobData.old_commitment.state_commitment.bytes)
      ),
    },
    new_commitment: {
      actions_commitment: scalar(
        commitmentArrayToBigInt(rawJobData.new_commitment.actions_commitment.bytes)
      ),
      actions_sequence: BigInt(rawJobData.new_commitment.actions_sequence),
      state_commitment: scalar(
        commitmentArrayToBigInt(rawJobData.new_commitment.state_commitment.bytes)
      ),
    },
    block_number: BigInt(rawJobData.block_number),
    sequence: BigInt(rawJobData.sequence),
  };
}

/**
 * Convert Fr.Canonical.provable back to bigint
 * @param provable - Fr.Canonical.provable object
 * @returns BigInt representation
 */
function provableToBigInt(provable: CanonicalElement): bigint {
  return provable.toBigInt();
}

/**
 * Serializes JobData to BCS bytes
 * @param jobData - The JobData object to serialize
 * @returns BCS serialized bytes
 */
export function serializeJobData(jobData: JobData): Uint8Array {
  // Convert back to raw format for serialization
  const rawJobData: RawJobData = {
    index: jobData.index,
    value: jobData.value.toString(),
    old_value: jobData.old_value.toString(),
    old_commitment: {
      actions_commitment: { bytes: bigIntToCommitmentArray(
        provableToBigInt(jobData.old_commitment.actions_commitment)
      ) },
      actions_sequence: jobData.old_commitment.actions_sequence.toString(),
      state_commitment: { bytes: bigIntToCommitmentArray(
        provableToBigInt(jobData.old_commitment.state_commitment)
      ) },
    },
    new_commitment: {
      actions_commitment: { bytes: bigIntToCommitmentArray(
        provableToBigInt(jobData.new_commitment.actions_commitment)
      ) },
      actions_sequence: jobData.new_commitment.actions_sequence.toString(),
      state_commitment: { bytes: bigIntToCommitmentArray(
        provableToBigInt(jobData.new_commitment.state_commitment)
      ) },
    },
    block_number: jobData.block_number.toString(),
    sequence: jobData.sequence.toString(),
  };

  return JobDataBcs.serialize(rawJobData).toBytes();
}

/**
 * Create AddProgramCommitment objects from JobData
 */
export interface ProcessedCommitments {
  oldCommitment: {
    actionsCommitment: CanonicalElement;
    stateCommitment: CanonicalElement;
    actionsSequence: UInt64;
    actionsRPower: CanonicalElement;
  };
  newCommitment: {
    actionsCommitment: CanonicalElement;
    stateCommitment: CanonicalElement;
    actionsSequence: UInt64;
    actionsRPower: CanonicalElement;
  };
}

/**
 * Process JobData into commitment objects ready for AddProgram
 * @param jobData - Deserialized JobData with provable commitments
 * @returns Processed commitment objects
 */
export function processCommitments(jobData: JobData): ProcessedCommitments {
  return {
    oldCommitment: {
      actionsCommitment: jobData.old_commitment.actions_commitment,
      stateCommitment: jobData.old_commitment.state_commitment,
      actionsSequence: UInt64.from(jobData.old_commitment.actions_sequence),
      actionsRPower: rScalarPow(jobData.old_commitment.actions_sequence),
    },
    newCommitment: {
      actionsCommitment: jobData.new_commitment.actions_commitment,
      stateCommitment: jobData.new_commitment.state_commitment,
      actionsSequence: UInt64.from(jobData.new_commitment.actions_sequence),
      actionsRPower: rScalarPow(jobData.new_commitment.actions_sequence),
    },
  };
}

/**
 * Helper function to convert bigint to hex string
 * @param element - bigint (bigint) representing an Element<Scalar>
 * @returns Hex string representation
 */
export function elementToHex(element: bigint | number[]): string {
  if (typeof element === "bigint") {
    // Convert bigint to hex with proper padding
    return "0x" + element.toString(16).padStart(64, "0");
  } else {
    // Legacy support for number arrays
    return "0x" + element.map((b) => b.toString(16).padStart(2, "0")).join("");
  }
}

/**
 * Helper function to convert hex string to bigint
 * @param hex - Hex string (with or without 0x prefix)
 * @returns bigint (bigint)
 */
export function hexToElement(hex: string): bigint {
  const cleanHex = hex.startsWith("0x") ? hex.slice(2) : hex;
  if (cleanHex.length !== 64) {
    throw new Error("Element<Scalar> hex must be 64 characters (32 bytes)");
  }

  return BigInt("0x" + cleanHex);
}

/**
 * Legacy helper function to convert Element<Scalar> bytes to hex string
 * @param elementBytes - 32 bytes representing an Element<Scalar> (as number[] from BCS)
 * @returns Hex string representation
 * @deprecated Use elementToHex with bigint instead
 */
export function elementBytesToHex(elementBytes: number[]): string {
  return (
    "0x" + elementBytes.map((b) => b.toString(16).padStart(2, "0")).join("")
  );
}

/**
 * Legacy helper function to convert hex string to Element<Scalar> bytes
 * @param hex - Hex string (with or without 0x prefix)
 * @returns 32-byte number array (for BCS compatibility)
 * @deprecated Use hexToElement with bigint instead
 */
export function hexToElementBytes(hex: string): number[] {
  const cleanHex = hex.startsWith("0x") ? hex.slice(2) : hex;
  if (cleanHex.length !== 64) {
    throw new Error("Element<Scalar> hex must be 64 characters (32 bytes)");
  }

  const bytes: number[] = [];
  for (let i = 0; i < 32; i++) {
    bytes.push(parseInt(cleanHex.substring(i * 2, i * 2 + 2), 16));
  }
  return bytes;
}
