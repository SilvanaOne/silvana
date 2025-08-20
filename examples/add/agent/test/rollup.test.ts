import { describe, it } from "node:test";
import assert from "node:assert";
import { action } from "./helpers/action.js";
import { purge } from "./helpers/purge.js";
import { createApp } from "./helpers/create.js";
import {
  AddProgram,
  AddProgramProof,
  AddProgramState,
} from "../src/circuit.js";
import { AddProgramCommitment } from "../src/commitment.js";
import {
  MerkleTree,
  Field,
  UInt32,
  Cache,
  VerificationKey,
  Encoding,
  UInt64,
} from "o1js";
import { getSum } from "./helpers/sum.js";
import { getState } from "./helpers/state.js";
import { scalar, R, rScalarPow } from "@silvana-one/mina-utils";

let appID: string | undefined = undefined;
let vk: VerificationKey | undefined = undefined;
const { state, map } = AddProgramState.create();
let serializedState: string = state.serialize(map);
const proofs: AddProgramProof[] = [];

describe("Add Rollup", async () => {
  it("should create app", async () => {
    console.log("Creating a fresh app for rollup testing...");
    appID = await createApp();
    assert.ok(appID !== undefined, "appID is not set");

    // Get the AppInstance ID from environment (set by createApp)
    const appInstanceID = process.env.APP_INSTANCE_ID;
    assert.ok(appInstanceID !== undefined, "appInstanceID is not set");

    // Verify initial state
    const initialState = await getState({ appInstanceID });
    assert.ok(initialState !== undefined, "state is not set");
    assert.ok(initialState.length === 1, "state length is not 1");
    assert.ok(initialState[0] === 0n, "initial sum is not 0");

    console.log("App ID:", appID);
    console.log("AppInstance ID:", appInstanceID);
    console.log("Initial state verified: sum = 0");
  });
  it("should get ZkProgram constraints", async () => {
    // Analyze the constraint count for both methods
    const methods = await AddProgram.analyzeMethods();
    const addMethodStats = (methods as any).add;
    const multiplyMethodStats = (methods as any).multiply;

    console.log(`\n=== ADD METHOD ===`);
    console.log(`Add constraints: ${addMethodStats.rows}`);
    console.log(`Gates breakdown:`);
    console.log(`  - Total gates: ${addMethodStats.gates.length}`);

    const addGateTypes = new Map<string, number>();
    for (const gate of addMethodStats.gates) {
      const typ = gate?.typ || gate?.type || "Unknown";
      addGateTypes.set(typ, (addGateTypes.get(typ) || 0) + 1);
    }

    console.log(`  - Gate types breakdown:`);
    for (const [type, count] of addGateTypes.entries()) {
      console.log(`    * ${type}: ${count}`);
    }
    console.log(`\n=== MULTIPLY METHOD ===`);
    console.log(`Multiply constraints: ${multiplyMethodStats.rows}`);
    console.log(`Gates breakdown:`);
    console.log(`  - Total gates: ${multiplyMethodStats.gates.length}`);

    const multiplyGateTypes = new Map<string, number>();
    for (const gate of multiplyMethodStats.gates) {
      const typ = gate?.typ || gate?.type || "Unknown";
      multiplyGateTypes.set(typ, (multiplyGateTypes.get(typ) || 0) + 1);
    }

    console.log(`  - Gate types breakdown:`);
    for (const [type, count] of multiplyGateTypes.entries()) {
      console.log(`    * ${type}: ${count}`);
    }
  });
  it("should compile", async () => {
    const cache = Cache.FileSystem("./cache");
    console.log("compiling...");
    console.time("compiled");
    vk = (await AddProgram.compile({ cache })).verificationKey;
    console.timeEnd("compiled");
    assert.ok(vk !== undefined, "vk is not set");
    console.log("vk", vk.hash.toJSON());
  });
  it("should add", async () => {
    const appInstanceID = process.env.APP_INSTANCE_ID;
    const initialState = await getState({ appInstanceID });
    assert.ok(initialState !== undefined, "initialState is not set");

    const result = await action({
      action: "add",
      value: 1,
      index: 1,
      appID,
      appInstanceID,
    });
    console.log("add result", result);
    assert.ok(result.index === 1, "index is not 1");
    assert.ok(result.new_sum === 1n, "new_sum is not 1");
    assert.ok(result.new_value === 1n, "new_value is not 1");
    assert.ok(result.old_sum === 0n, "old_sum is not 0");
    assert.ok(result.old_value === 0n, "old_value is not 0");
    const tree = new MerkleTree(TREE_DEPTH);
    for (let i = 1; i < initialState.length; i++) {
      console.log("setting leaf", i, initialState[i]);
      tree.setLeaf(BigInt(i), Field(initialState[i]));
    }
    assert.ok(
      tree.getRoot().toBigInt() === state[state.length - 1].root.toBigInt(),
      "tree root mismatch"
    );
    const witness = new Witness(tree.getWitness(BigInt(result.index)));
    const newState = await getState({ appInstanceID });
    assert.ok(newState.length === 2, "newState length is not 2");
    assert.ok(newState[0] === 1n, "newState[0] is not 1");
    assert.ok(newState[1] === 1n, "newState[1] is not 1");
    console.time("add proof");
    const proofResult = await AddProgram.add(
      state[state.length - 1],
      UInt32.from(result.index),
      Field(result.old_value),
      Field(result.value),
      witness,
      new AddProgramCommitment({
        actionsCommitment: scalar(result.old_actions_commitment),
        stateCommitment: scalar(result.old_state_commitment),
        actionsSequence: UInt64.from(result.old_actions_sequence),
        actionsRPower: rScalarPow(result.old_actions_sequence),
      }),
      new AddProgramCommitment({
        actionsCommitment: scalar(result.new_actions_commitment),
        stateCommitment: scalar(result.new_state_commitment),
        actionsSequence: UInt64.from(result.new_actions_sequence),
        actionsRPower: rScalarPow(result.new_actions_sequence),
      })
    );
    console.timeEnd("add proof");
    proofs.push(proofResult.proof);
    const publicOutput = proofResult.proof.publicOutput;

    state.push(publicOutput);
    tree.setLeaf(BigInt(result.index), Field(result.new_value));
    assert.ok(
      publicOutput.root.toBigInt() === tree.getRoot().toBigInt(),
      "newTracedState.publicOutput.root mismatch"
    );
    assert.ok(
      publicOutput.sum.toBigInt() === newState[0],
      "newTracedState.publicOutput.sum is not 1"
    );
  });
  it("should multiply", async () => {
    const appInstanceID = process.env.APP_INSTANCE_ID;
    const initialState = await getState({ appInstanceID });
    assert.ok(initialState !== undefined, "initialState is not set");

    const result = await action({
      action: "multiply",
      value: 2,
      index: 1,
      appID,
      appInstanceID,
    });
    console.log("multiply result", result);
    assert.ok(result.index === 1, "index is not 1");
    assert.ok(result.new_sum === 2n, "new_sum is not 2");
    assert.ok(result.new_value === 2n, "new_value is not 2");
    assert.ok(result.old_sum === 1n, "old_sum is not 1");
    assert.ok(result.old_value === 1n, "old_value is not 1");
    const tree = new MerkleTree(TREE_DEPTH);
    for (let i = 1; i < initialState.length; i++) {
      console.log("setting leaf", i, initialState[i]);
      tree.setLeaf(BigInt(i), Field(initialState[i]));
    }
    assert.ok(
      tree.getRoot().toBigInt() === state[state.length - 1].root.toBigInt(),
      "tree root mismatch"
    );
    const witness = new Witness(tree.getWitness(BigInt(result.index)));
    const newState = await getState({ appInstanceID });
    assert.ok(newState.length === 2, "newState length is not 2");
    assert.ok(newState[0] === 2n, "newState[0] is not 2");
    assert.ok(newState[1] === 2n, "newState[1] is not 2");
    console.time("multiply proof");
    const proofResult = await AddProgram.multiply(
      state[state.length - 1],
      UInt32.from(result.index),
      Field(result.old_value),
      Field(result.value),
      witness,
      new AddProgramCommitment({
        actionsCommitment: scalar(result.old_actions_commitment),
        stateCommitment: scalar(result.old_state_commitment),
        actionsSequence: UInt64.from(result.old_actions_sequence),
        actionsRPower: rScalarPow(result.old_actions_sequence),
      }),
      new AddProgramCommitment({
        actionsCommitment: scalar(result.new_actions_commitment),
        stateCommitment: scalar(result.new_state_commitment),
        actionsSequence: UInt64.from(result.new_actions_sequence),
        actionsRPower: rScalarPow(result.new_actions_sequence),
      })
    );
    console.timeEnd("multiply proof");
    proofs.push(proofResult.proof);
    const publicOutput = proofResult.proof.publicOutput;
    state.push(publicOutput);
    tree.setLeaf(BigInt(result.index), Field(result.new_value));
    assert.ok(
      publicOutput.root.toBigInt() === tree.getRoot().toBigInt(),
      "publicOutput.root mismatch"
    );
    assert.ok(
      publicOutput.sum.toBigInt() === newState[0],
      "publicOutput.sum is not 2"
    );
  });
  it("should get sum", async () => {
    const appInstanceID = process.env.APP_INSTANCE_ID;
    const sum = await getSum({ appInstanceID });
    assert.ok(sum !== undefined, "sum is not set");
    assert.ok(sum === 2, "sum is not 2");
  });

  it("should merge proofs", async () => {
    console.time("merge proofs");
    const mergedProof = await AddProgram.merge(
      proofs[0].publicInput,
      proofs[0],
      proofs[1]
    );
    console.timeEnd("merge proofs");
    assert.ok(mergedProof !== undefined, "mergedProof is not set");
  });
  it("should purge", async () => {
    const appInstanceID = process.env.APP_INSTANCE_ID;
    await purge({
      proved_sequence: 2,
      appID,
      appInstanceID,
    });
  });
});
