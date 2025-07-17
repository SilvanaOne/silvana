import {
  ZkProgram,
  Provable,
  Field,
  UInt32,
  Struct,
  MerkleWitness,
  MerkleTree,
  SelfProof,
} from "o1js";

export const TREE_DEPTH = 11; //TODO: increase depth

export class Witness extends MerkleWitness(TREE_DEPTH) {}

export class AddProgramState extends Struct({
  sum: Field,
  root: Field,
}) {
  static assertEquals(a: AddProgramState, b: AddProgramState) {
    a.sum.assertEquals(b.sum);
    a.root.assertEquals(b.root);
  }
}

export const AddProgram = ZkProgram({
  name: "AddProgram",
  publicInput: AddProgramState,
  publicOutput: AddProgramState,
  methods: {
    add: {
      privateInputs: [UInt32, Field, Field, Witness],
      async method(
        input: AddProgramState,
        index: UInt32,
        state: Field,
        value: Field,
        witness: Witness
      ) {
        value.assertLessThan(100);
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
        const sum = input.sum.add(value);
        const newRoot = witness.calculateRoot(state.add(value));
        return { publicOutput: { sum, root: newRoot } };
      },
    },
    multiply: {
      privateInputs: [UInt32, Field, Field, Witness],
      async method(
        input: AddProgramState,
        index: UInt32,
        state: Field,
        value: Field,
        witness: Witness
      ) {
        value.assertLessThan(100);
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
        const sum = input.sum.add(state.mul(value).sub(state));
        const newRoot = witness.calculateRoot(state.mul(value));
        return { publicOutput: { sum, root: newRoot } };
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
