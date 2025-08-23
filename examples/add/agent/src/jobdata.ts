import { bcs } from "@mysten/sui/bcs";
import { UInt64 } from "o1js";
import { Fr, scalar, rScalarPow } from "@silvana-one/mina-utils";

/**
 * JobData struct matching the Move definition in main.move
 *
 * Move struct:
 * public struct JobData has copy, drop {
 *     index: u32,
 *     value: u256,
 *     old_value: u256,
 *     old_actions_commitment: Element<Scalar>,
 *     old_state_commitment: Element<Scalar>,
 *     old_actions_sequence: u64,
 *     new_actions_commitment: Element<Scalar>,
 *     new_state_commitment: Element<Scalar>,
 *     new_actions_sequence: u64,
 *     block_number: u64,
 *     sequence: u64,
 * }
 */

// Element<Scalar> is serialized as bytes vector in Move
const ElementScalar = bcs.vector(bcs.u8());

export const JobDataBcs = bcs.struct("JobData", {
  index: bcs.u32(),
  value: bcs.u256(),
  old_value: bcs.u256(),
  old_actions_commitment: ElementScalar,
  old_state_commitment: ElementScalar,
  old_actions_sequence: bcs.u64(),
  new_actions_commitment: ElementScalar,
  new_state_commitment: ElementScalar,
  new_actions_sequence: bcs.u64(),
  block_number: bcs.u64(),
  sequence: bcs.u64(),
});

/**
 * Canonical Element type representing a BLS12-381 scalar as a bigint
 */
export type CanonicalElement = bigint;

/**
 * Raw JobData as returned by BCS deserialization
 */
export interface RawJobData {
  index: number;
  value: string; // BCS returns u256 as string
  old_value: string; // BCS returns u256 as string
  old_actions_commitment: number[]; // BCS returns fixedArray as number[]
  old_state_commitment: number[]; // BCS returns fixedArray as number[]
  old_actions_sequence: string; // BCS returns u64 as string
  new_actions_commitment: number[]; // BCS returns fixedArray as number[]
  new_state_commitment: number[]; // BCS returns fixedArray as number[]
  new_actions_sequence: string; // BCS returns u64 as string
  block_number: string; // BCS returns u64 as string
  sequence: string; // BCS returns u64 as string
}

/**
 * Processed JobData with provable commitment representations
 */
export interface JobData {
  index: number;
  value: string; // BCS returns u256 as string
  old_value: string; // BCS returns u256 as string
  old_actions_commitment: Fr.Canonical.provable; // Provable scalar
  old_state_commitment: Fr.Canonical.provable; // Provable scalar
  old_actions_sequence: string; // BCS returns u64 as string
  new_actions_commitment: Fr.Canonical.provable; // Provable scalar
  new_state_commitment: Fr.Canonical.provable; // Provable scalar
  new_actions_sequence: string; // BCS returns u64 as string
  block_number: string; // BCS returns u64 as string
  sequence: string; // BCS returns u64 as string
}

/**
 * Convert commitment bytes to bigint for scalar operations
 * BLS12-381 scalars are encoded in big-endian format
 * @param bytes - Byte array representing Element<Scalar>
 * @returns BigInt representation
 */
export function commitmentArrayToBigInt(bytes: number[]): CanonicalElement {
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
export function bigIntToCommitmentArray(value: CanonicalElement): number[] {
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
    value: rawJobData.value,
    old_value: rawJobData.old_value,
    old_actions_commitment: scalar(
      commitmentArrayToBigInt(rawJobData.old_actions_commitment)
    ),
    old_state_commitment: scalar(
      commitmentArrayToBigInt(rawJobData.old_state_commitment)
    ),
    old_actions_sequence: rawJobData.old_actions_sequence,
    new_actions_commitment: scalar(
      commitmentArrayToBigInt(rawJobData.new_actions_commitment)
    ),
    new_state_commitment: scalar(
      commitmentArrayToBigInt(rawJobData.new_state_commitment)
    ),
    new_actions_sequence: rawJobData.new_actions_sequence,
    block_number: rawJobData.block_number,
    sequence: rawJobData.sequence,
  };
}

/**
 * Convert Fr.Canonical.provable back to bigint
 * @param provable - Fr.Canonical.provable object
 * @returns BigInt representation
 */
function provableToBigInt(provable: Fr.Canonical.provable): CanonicalElement {
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
    value: jobData.value,
    old_value: jobData.old_value,
    old_actions_commitment: bigIntToCommitmentArray(
      provableToBigInt(jobData.old_actions_commitment)
    ),
    old_state_commitment: bigIntToCommitmentArray(
      provableToBigInt(jobData.old_state_commitment)
    ),
    old_actions_sequence: jobData.old_actions_sequence,
    new_actions_commitment: bigIntToCommitmentArray(
      provableToBigInt(jobData.new_actions_commitment)
    ),
    new_state_commitment: bigIntToCommitmentArray(
      provableToBigInt(jobData.new_state_commitment)
    ),
    new_actions_sequence: jobData.new_actions_sequence,
    block_number: jobData.block_number,
    sequence: jobData.sequence,
  };

  return JobDataBcs.serialize(rawJobData).toBytes();
}

/**
 * Create AddProgramCommitment objects from JobData
 */
export interface ProcessedCommitments {
  oldCommitment: {
    actionsCommitment: Fr.Canonical.provable;
    stateCommitment: Fr.Canonical.provable;
    actionsSequence: UInt64;
    actionsRPower: Fr.Canonical.provable;
  };
  newCommitment: {
    actionsCommitment: Fr.Canonical.provable;
    stateCommitment: Fr.Canonical.provable;
    actionsSequence: UInt64;
    actionsRPower: Fr.Canonical.provable;
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
      actionsCommitment: jobData.old_actions_commitment,
      stateCommitment: jobData.old_state_commitment,
      actionsSequence: UInt64.from(jobData.old_actions_sequence),
      actionsRPower: rScalarPow(BigInt(jobData.old_actions_sequence)),
    },
    newCommitment: {
      actionsCommitment: jobData.new_actions_commitment,
      stateCommitment: jobData.new_state_commitment,
      actionsSequence: UInt64.from(jobData.new_actions_sequence),
      actionsRPower: rScalarPow(BigInt(jobData.new_actions_sequence)),
    },
  };
}

/**
 * Helper function to convert CanonicalElement to hex string
 * @param element - CanonicalElement (bigint) representing an Element<Scalar>
 * @returns Hex string representation
 */
export function elementToHex(element: CanonicalElement | number[]): string {
  if (typeof element === "bigint") {
    // Convert bigint to hex with proper padding
    return "0x" + element.toString(16).padStart(64, "0");
  } else {
    // Legacy support for number arrays
    return "0x" + element.map((b) => b.toString(16).padStart(2, "0")).join("");
  }
}

/**
 * Helper function to convert hex string to CanonicalElement
 * @param hex - Hex string (with or without 0x prefix)
 * @returns CanonicalElement (bigint)
 */
export function hexToElement(hex: string): CanonicalElement {
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
 * @deprecated Use elementToHex with CanonicalElement instead
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
 * @deprecated Use hexToElement with CanonicalElement instead
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
