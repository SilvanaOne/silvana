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
 * TransitionData struct matching the Move definition in main.move
 *
 * Move struct:
 * public struct TransitionData has copy, drop {
 *     block_number: u64,
 *     sequence: u64,
 *     method: String,
 *     index: u32,
 *     value: u256,
 *     old_value: u256,
 *     old_commitment: CommitmentData,
 *     new_commitment: CommitmentData,
 * }
 */

// Element<Scalar> is a struct with bytes field (from group_ops.move line 22-24)
const ElementScalar = bcs.struct("Element", {
  bytes: bcs.vector(bcs.u8()),
});

export const CommitmentDataBcs = bcs.struct("CommitmentData", {
  actions_commitment: ElementScalar,
  actions_sequence: bcs.u64(),
  state_commitment: ElementScalar,
});

export const TransitionDataBcs = bcs.struct("TransitionData", {
  block_number: bcs.u64(),
  sequence: bcs.u64(),
  method: bcs.string(),
  index: bcs.u32(),
  value: bcs.u256(),
  old_value: bcs.u256(),
  old_commitment: CommitmentDataBcs,
  new_commitment: CommitmentDataBcs,
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
 * Raw TransitionData as returned by BCS deserialization
 */
export interface RawTransitionData {
  block_number: string; // BCS returns u64 as string
  sequence: string; // BCS returns u64 as string
  method: string;
  index: number;
  value: string; // BCS returns u256 as string
  old_value: string; // BCS returns u256 as string
  old_commitment: RawCommitmentData;
  new_commitment: RawCommitmentData;
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
 * Processed TransitionData with provable commitment representations
 */
export interface TransitionData {
  block_number: bigint; // u64 as bigint for numeric operations
  sequence: bigint; // u64 as bigint for numeric operations
  method: string;
  index: number;
  value: bigint; // u256 as bigint for numeric operations
  old_value: bigint; // u256 as bigint for numeric operations
  old_commitment: CommitmentData;
  new_commitment: CommitmentData;
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
 * Deserializes raw TransitionData from BCS bytes
 * @param data - The BCS serialized data as byte array or Uint8Array
 * @returns Raw TransitionData object (with number[] commitments)
 */
export function deserializeRawTransitionData(
  data: number[] | Uint8Array
): RawTransitionData {
  const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
  return TransitionDataBcs.parse(bytes);
}

/**
 * Deserializes TransitionData from BCS bytes and converts commitments to provable form
 * @param data - The BCS serialized data as byte array or Uint8Array
 * @returns Processed TransitionData object with Fr.Canonical.provable commitments
 */
export function deserializeTransitionData(
  data: number[] | Uint8Array
): TransitionData {
  const rawTransitionData = deserializeRawTransitionData(data);

  return {
    block_number: BigInt(rawTransitionData.block_number),
    sequence: BigInt(rawTransitionData.sequence),
    method: rawTransitionData.method,
    index: rawTransitionData.index,
    value: BigInt(rawTransitionData.value),
    old_value: BigInt(rawTransitionData.old_value),
    old_commitment: {
      actions_commitment: scalar(
        commitmentArrayToBigInt(
          rawTransitionData.old_commitment.actions_commitment.bytes
        )
      ),
      actions_sequence: BigInt(
        rawTransitionData.old_commitment.actions_sequence
      ),
      state_commitment: scalar(
        commitmentArrayToBigInt(
          rawTransitionData.old_commitment.state_commitment.bytes
        )
      ),
    },
    new_commitment: {
      actions_commitment: scalar(
        commitmentArrayToBigInt(
          rawTransitionData.new_commitment.actions_commitment.bytes
        )
      ),
      actions_sequence: BigInt(
        rawTransitionData.new_commitment.actions_sequence
      ),
      state_commitment: scalar(
        commitmentArrayToBigInt(
          rawTransitionData.new_commitment.state_commitment.bytes
        )
      ),
    },
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
 * Serializes TransitionData to BCS bytes
 * @param transitionData - The TransitionData object to serialize
 * @returns BCS serialized bytes
 */
export function serializeTransitionData(
  transitionData: TransitionData
): Uint8Array {
  // Convert back to raw format for serialization
  const rawTransitionData: RawTransitionData = {
    block_number: transitionData.block_number.toString(),
    sequence: transitionData.sequence.toString(),
    method: transitionData.method,
    index: transitionData.index,
    value: transitionData.value.toString(),
    old_value: transitionData.old_value.toString(),
    old_commitment: {
      actions_commitment: {
        bytes: bigIntToCommitmentArray(
          provableToBigInt(transitionData.old_commitment.actions_commitment)
        ),
      },
      actions_sequence:
        transitionData.old_commitment.actions_sequence.toString(),
      state_commitment: {
        bytes: bigIntToCommitmentArray(
          provableToBigInt(transitionData.old_commitment.state_commitment)
        ),
      },
    },
    new_commitment: {
      actions_commitment: {
        bytes: bigIntToCommitmentArray(
          provableToBigInt(transitionData.new_commitment.actions_commitment)
        ),
      },
      actions_sequence:
        transitionData.new_commitment.actions_sequence.toString(),
      state_commitment: {
        bytes: bigIntToCommitmentArray(
          provableToBigInt(transitionData.new_commitment.state_commitment)
        ),
      },
    },
  };

  return TransitionDataBcs.serialize(rawTransitionData).toBytes();
}

/**
 * Create AddProgramCommitment objects from TransitionData
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
 * Process TransitionData into commitment objects ready for AddProgram
 * @param transitionData - Deserialized TransitionData with provable commitments
 * @returns Processed commitment objects
 */
export function processCommitments(
  transitionData: TransitionData
): ProcessedCommitments {
  return {
    oldCommitment: {
      actionsCommitment: transitionData.old_commitment.actions_commitment,
      stateCommitment: transitionData.old_commitment.state_commitment,
      actionsSequence: UInt64.from(
        transitionData.old_commitment.actions_sequence
      ),
      actionsRPower: rScalarPow(transitionData.old_commitment.actions_sequence),
    },
    newCommitment: {
      actionsCommitment: transitionData.new_commitment.actions_commitment,
      stateCommitment: transitionData.new_commitment.state_commitment,
      actionsSequence: UInt64.from(
        transitionData.new_commitment.actions_sequence
      ),
      actionsRPower: rScalarPow(transitionData.new_commitment.actions_sequence),
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
