import {
  ZkProgram,
  Field,
  UInt32,
  Struct,
  SelfProof,
  UInt64,
  Experimental,
  Encoding,
} from "o1js";
import { AddProgramCommitment, calculateNewCommitment } from "./commitment.js";
import {
  serializeIndexedMap,
  deserializeIndexedMerkleMap,
} from "@silvana-one/storage";
import { R, scalar } from "@silvana-one/mina-utils";

export const MAP_HEIGHT = 10;
const IndexedMerkleMap = Experimental.IndexedMerkleMap;
type IndexedMerkleMap = Experimental.IndexedMerkleMap;

export class AddMap extends IndexedMerkleMap(MAP_HEIGHT) {}

export class AddProgramState extends Struct({
  blockNumber: UInt64,
  sequence: UInt64,
  sum: Field,
  root: Field,
  length: Field,
  commitment: AddProgramCommitment,
}) {
  static assertEquals(a: AddProgramState, b: AddProgramState) {
    a.blockNumber.assertEquals(b.blockNumber);
    a.sequence.assertEquals(b.sequence);
    a.sum.assertEquals(b.sum);
    a.root.assertEquals(b.root);
    a.length.assertEquals(b.length);
    AddProgramCommitment.assertEquals(a.commitment, b.commitment);
  }

  serialize(map: AddMap): string {
    return JSON.stringify({
      blockNumber: this.blockNumber.toBigInt().toString(),
      sequence: this.sequence.toBigInt().toString(),
      sum: this.sum.toBigInt().toString(),
      root: this.root.toBigInt().toString(),
      length: this.length.toBigInt().toString(),
      commitment: this.commitment.serialize(),
      map: serializeIndexedMap(map),
    });
  }

  static deserialize(str: string): { state: AddProgramState; map: AddMap } {
    const { blockNumber, sequence, sum, root, length, commitment, map } =
      JSON.parse(str);
    const indexedMap = deserializeIndexedMerkleMap({
      serializedIndexedMap: map,
      type: AddMap,
    });
    return {
      state: new AddProgramState({
        blockNumber: UInt64.from(BigInt(blockNumber)),
        sequence: UInt64.from(BigInt(sequence)),
        sum: Field.from(BigInt(sum)),
        root: Field.from(BigInt(root)),
        length: Field.from(BigInt(length)),
        commitment: AddProgramCommitment.deserialize(commitment),
      }),
      map: indexedMap as AddMap,
    };
  }

  static create(): { state: AddProgramState; map: AddMap } {
    const map = new AddMap();
    const state = new AddProgramState({
      blockNumber: UInt64.from(0),
      sequence: UInt64.from(0),
      sum: Field(0),
      root: map.root,
      length: map.length,
      commitment: new AddProgramCommitment({
        stateCommitment: scalar(0n),
        actionsCommitment: scalar(
          Encoding.stringToFields("init")[0].toBigInt()
        ),
        actionsSequence: UInt64.from(1n),
        actionsRPower: R,
      }),
    });
    return { state, map };
  }
}

export const AddProgram = ZkProgram({
  name: "AddProgram",
  publicInput: AddProgramState,
  publicOutput: AddProgramState,
  methods: {
    add: {
      privateInputs: [
        UInt32,
        Field,
        Field,
        AddMap,
        AddProgramCommitment,
        AddProgramCommitment,
      ],
      auxiliaryOutput: AddMap,
      async method(
        input: AddProgramState,
        index: UInt32,
        state: Field,
        value: Field,
        map: AddMap,
        inputMoveCommitment: AddProgramCommitment,
        outputMoveCommitment: AddProgramCommitment
      ) {
        // Check value limits
        value.assertLessThan(100);

        // Check inclusion proof for state
        map.root.assertEquals(input.root, "Map root should match input root");
        map.length.assertEquals(
          input.length,
          "Map length should match input length"
        );

        // Calculate new sum, value and root
        map
          .getOption(index.value)
          .orElse(Field(0))
          .assertEquals(state, "State should match map");
        const sum = input.sum.add(value);
        const new_value = state.add(value);
        map.set(index.value, new_value);

        // Calculate new commitment
        const commitment = calculateNewCommitment({
          methodName: "add",
          inputCommitment: input.commitment,
          inputMoveCommitment,
          outputMoveCommitment,
          index,
          value,
          old_value: state,
          new_value,
          old_sum: input.sum,
          new_sum: sum,
        });
        return {
          publicOutput: new AddProgramState({
            blockNumber: input.blockNumber,
            sequence: input.sequence.add(1),
            sum,
            root: map.root,
            length: map.length,
            commitment,
          }),
          auxiliaryOutput: map,
        };
      },
    },
    multiply: {
      privateInputs: [
        UInt32,
        Field,
        Field,
        AddMap,
        AddProgramCommitment,
        AddProgramCommitment,
      ],
      auxiliaryOutput: AddMap,
      async method(
        input: AddProgramState,
        index: UInt32,
        state: Field,
        value: Field,
        map: AddMap,
        inputCommitment: AddProgramCommitment,
        outputCommitment: AddProgramCommitment
      ) {
        // Check value limits
        value.assertLessThan(100);

        // Check inclusion proof for state
        map.root.assertEquals(input.root, "Map root should match input root");
        map.length.assertEquals(
          input.length,
          "Map length should match input length"
        );

        // Calculate new sum, value and root
        map
          .getOption(index.value)
          .orElse(Field(0))
          .assertEquals(state, "State should match map");
        const sum = input.sum.add(state.mul(value).sub(state));
        const new_value = state.mul(value);
        map.set(index.value, new_value);

        // Calculate new commitment
        const commitment = calculateNewCommitment({
          methodName: "multiply",
          inputCommitment: input.commitment,
          inputMoveCommitment: inputCommitment,
          outputMoveCommitment: outputCommitment,
          index,
          value,
          old_value: state,
          new_value,
          old_sum: input.sum,
          new_sum: sum,
        });
        return {
          publicOutput: new AddProgramState({
            blockNumber: input.blockNumber,
            sequence: input.sequence.add(1),
            sum,
            root: map.root,
            length: map.length,
            commitment,
          }),
          auxiliaryOutput: map,
        };
      },
    },
    merge: {
      privateInputs: [SelfProof, SelfProof],
      async method(
        input: AddProgramState,
        proof1: SelfProof<AddProgramState, AddProgramState>,
        proof2: SelfProof<AddProgramState, AddProgramState>
      ) {
        proof1.verify();
        proof2.verify();
        AddProgramState.assertEquals(input, proof1.publicInput);
        AddProgramState.assertEquals(proof1.publicOutput, proof2.publicInput);
        return {
          publicOutput: proof2.publicOutput,
        };
      },
    },
  },
});

export class AddProgramProof extends ZkProgram.Proof(AddProgram) {}
