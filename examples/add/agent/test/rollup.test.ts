import { describe, it } from "node:test";
import assert from "node:assert";
import { action } from "./helpers/action.js";
import { purge } from "./helpers/purge.js";
import { createApp } from "./helpers/create.js";
import {
  AddProgram,
  AddProgramProof,
  AddProgramState,
  AddMap,
} from "../src/circuit.js";
import { AddProgramCommitment } from "../src/commitment.js";
import {
  MerkleTree,
  Field,
  UInt32,
  Cache,
  VerificationKey,
  Encoding,
} from "o1js";
import { getSum } from "./helpers/sum.js";
import { getState } from "./helpers/state.js";
import { deserializeTransitionData, processCommitments } from "../src/transition.js";
// Removed unused imports - scalar, R, rScalarPow now come from processCommitments

let appID: string | undefined = undefined;
let vk: VerificationKey | undefined = undefined;
let { state, map } = AddProgramState.create();
const proofs: AddProgramProof[] = [];

describe("Add Rollup", async () => {
  it("should create app", async () => {
    console.log("Creating a fresh app for rollup testing...");
    // Use default values for test
    appID = await createApp({
      contractAddress: "B62qmZB4E4KhmpYwoPDHe5c4yeQeAreCEwwgkGUrqSa6Ma3uC2RDZRY",
      chain: "mina:devnet",
      nonce: 1,
    });
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
    const newState = await getState({ appInstanceID });

    if (!result.jobCreatedEvent) {
      throw new Error("JobCreatedEvent is required for proof calculation");
    }

    console.time("add proof");

    // Deserialize TransitionData from the event
    const transitionData = deserializeTransitionData(result.jobCreatedEvent.data);

    // Process commitments with all calculations done in transition.ts
    const commitments = processCommitments(transitionData);

    // Calculate proof using processed commitments
    const proofResult = await AddProgram.add(
      state, // Current circuit state (maintained between operations)
      UInt32.from(transitionData.index),
      Field(transitionData.old_value),
      Field(transitionData.value),
      map, // Current map state (maintained between operations)
      new AddProgramCommitment(commitments.oldCommitment),
      new AddProgramCommitment(commitments.newCommitment)
    );

    console.timeEnd("add proof");
    proofs.push(proofResult.proof);
    const publicOutput = proofResult.proof.publicOutput;

    state = publicOutput;
    map = proofResult.auxiliaryOutput;

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

    const newState = await getState({ appInstanceID });

    if (!result.jobCreatedEvent) {
      throw new Error("JobCreatedEvent is required for proof calculation");
    }

    console.time("multiply proof from JobCreatedEvent");

    // Deserialize TransitionData from the event
    const transitionData = deserializeTransitionData(result.jobCreatedEvent.data);

    // Process commitments with all calculations done in transition.ts
    const commitments = processCommitments(transitionData);

    const serializedState = state.serialize(map);
    const { state: deserializedState, map: deserializedMap } =
      AddProgramState.deserialize(serializedState);
    const proofResult = await AddProgram.multiply(
      deserializedState,
      UInt32.from(transitionData.index),
      Field(transitionData.old_value),
      Field(transitionData.value),
      deserializedMap,
      new AddProgramCommitment(commitments.oldCommitment),
      new AddProgramCommitment(commitments.newCommitment)
    );
    console.timeEnd("multiply proof from JobCreatedEvent");
    proofs.push(proofResult.proof);
    const publicOutput = proofResult.proof.publicOutput;
    state = publicOutput;

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
    assert.ok(
      mergedProof.proof.publicOutput.sum.toBigInt() === 2n,
      "mergedProof sum is not 2"
    );
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
