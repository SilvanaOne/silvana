import {
  ZkProgram,
  Field,
  UInt32,
  Struct,
  MerkleWitness,
  SelfProof,
} from "o1js";
import { AddProgramCommitment, calculateNewCommitment } from "./commitment.js";

export const TREE_DEPTH = 16;
export class Witness extends MerkleWitness(TREE_DEPTH) {}

export class AddProgramState extends Struct({
  sum: Field,
  root: Field,
  commitment: AddProgramCommitment,
}) {
  static assertEquals(a: AddProgramState, b: AddProgramState) {
    a.sum.assertEquals(b.sum);
    a.root.assertEquals(b.root);
    AddProgramCommitment.assertEquals(a.commitment, b.commitment);
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
        Witness,
        AddProgramCommitment,
        AddProgramCommitment,
      ],
      async method(
        input: AddProgramState,
        index: UInt32,
        state: Field,
        value: Field,
        witness: Witness,
        inputMoveCommitment: AddProgramCommitment,
        outputMoveCommitment: AddProgramCommitment
      ) {
        // Check value limits
        value.assertLessThan(100);

        // Check inclusion proof for state
        const witnessRoot = witness.calculateRoot(state);
        const witnessIndex = witness.calculateIndex();
        witnessRoot.assertEquals(
          input.root,
          "Witness root should match input root"
        );
        witnessIndex.assertEquals(
          index.value,
          "Witness index should match index"
        );

        // Calculate new sum, value and root
        const sum = input.sum.add(value);
        const new_value = state.add(value);
        const root = witness.calculateRoot(new_value);

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
          publicOutput: { sum, root, commitment },
        };
      },
    },
    multiply: {
      privateInputs: [
        UInt32,
        Field,
        Field,
        Witness,
        AddProgramCommitment,
        AddProgramCommitment,
      ],
      async method(
        input: AddProgramState,
        index: UInt32,
        state: Field,
        value: Field,
        witness: Witness,
        inputCommitment: AddProgramCommitment,
        outputCommitment: AddProgramCommitment
      ) {
        // Check value limits
        value.assertLessThan(100);

        // Check inclusion proof for state
        const witnessRoot = witness.calculateRoot(state);
        const witnessIndex = witness.calculateIndex();
        witnessRoot.assertEquals(
          input.root,
          "Witness root should match input root"
        );
        witnessIndex.assertEquals(
          index.value,
          "Witness index should match index"
        );

        // Calculate new sum, value and root
        const sum = input.sum.add(state.mul(value).sub(state));
        const new_value = state.mul(value);
        const root = witness.calculateRoot(new_value);

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
          publicOutput: { sum, root, commitment },
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
