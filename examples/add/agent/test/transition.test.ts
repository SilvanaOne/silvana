import { describe, it } from "node:test";
import assert from "node:assert";
import { deserializeTransitionData, TransitionData } from "../src/transition.js";

describe("TransitionData Deserialization", () => {
  it("should deserialize TransitionData from JobCreatedEvent data", () => {
    // Fresh data from current rollup test (add operation: index 1, value 1)
    const jobCreatedEventData = [
      1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,3,97,100,100,1,0,0,0,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,32,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1,116,105,110,105,1,0,0,0,0,0,0,0,32,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,32,91,30,149,212,112,213,95,240,231,74,108,68,19,170,163,45,205,194,243,248,43,83,174,52,97,32,156,4,113,185,131,164,2,0,0,0,0,0,0,0,32,20,159,168,194,9,171,101,95,212,128,163,175,247,209,109,199,43,106,57,67,228,185,95,207,121,9,244,45,156,23,165,83
    ];

    const transitionData: TransitionData = deserializeTransitionData(jobCreatedEventData);


    // Basic assertions using actual values from Sui rollup test
    assert.strictEqual(transitionData.block_number, 1n, "Block number should be 1");
    assert.strictEqual(transitionData.sequence, 0n, "Sequence should be 0");
    assert.strictEqual(transitionData.method, "add", "Method should be add");
    assert.strictEqual(transitionData.index, 1, "Index should be 1");
    assert.strictEqual(transitionData.value, 1n, "Value should be 1");
    assert.strictEqual(transitionData.old_value, 0n, "Old value should be 0");
    // Exact assertions based on current rollup test values
    assert.strictEqual(transitionData.old_commitment.actions_sequence, 1n, "Old actions sequence should be 1");
    assert.strictEqual(transitionData.new_commitment.actions_sequence, 2n, "New actions sequence should be 2");
    
    // Exact commitment value assertions from current rollup test
    const oldActionsCommitmentBigInt = transitionData.old_commitment.actions_commitment.toBigInt();
    const oldStateCommitmentBigInt = transitionData.old_commitment.state_commitment.toBigInt();
    const newActionsCommitmentBigInt = transitionData.new_commitment.actions_commitment.toBigInt();
    const newStateCommitmentBigInt = transitionData.new_commitment.state_commitment.toBigInt();
    
    // Assert exact commitment values from current rollup test
    assert.strictEqual(oldActionsCommitmentBigInt, 6248033897n, "Old actions commitment should match rollup test value");
    assert.strictEqual(oldStateCommitmentBigInt, 0n, "Old state commitment should match rollup test value");
    assert.strictEqual(newActionsCommitmentBigInt, 41214508720617712057645573797159612427662368158173771695682160867969771078564n, "New actions commitment should match rollup test value");
    assert.strictEqual(newStateCommitmentBigInt, 9328350379599323964930814050805468085812740822563546972518289385183440774483n, "New state commitment should match rollup test value");
    
    // Check commitment objects are ProvableElements 
    assert.ok(typeof transitionData.old_commitment.actions_commitment.toBigInt === 'function', "Old actions commitment should be ProvableElement");
    assert.ok(typeof transitionData.old_commitment.state_commitment.toBigInt === 'function', "Old state commitment should be ProvableElement");
    assert.ok(typeof transitionData.new_commitment.actions_commitment.toBigInt === 'function', "New actions commitment should be ProvableElement");
    assert.ok(typeof transitionData.new_commitment.state_commitment.toBigInt === 'function', "New state commitment should be ProvableElement");

  });

  it("should match exact bigint values from Sui rollup test", () => {
    // Fresh data from current rollup test (same as first test)
    const jobCreatedEventData = [
      1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,3,97,100,100,1,0,0,0,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,32,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1,116,105,110,105,1,0,0,0,0,0,0,0,32,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,32,91,30,149,212,112,213,95,240,231,74,108,68,19,170,163,45,205,194,243,248,43,83,174,52,97,32,156,4,113,185,131,164,2,0,0,0,0,0,0,0,32,20,159,168,194,9,171,101,95,212,128,163,175,247,209,109,199,43,106,57,67,228,185,95,207,121,9,244,45,156,23,165,83
    ];

    const transitionData: TransitionData = deserializeTransitionData(jobCreatedEventData);

    // Get bigint values directly from ProvableElements
    const oldActionsCommitmentBigInt = transitionData.old_commitment.actions_commitment.toBigInt();
    const oldStateCommitmentBigInt = transitionData.old_commitment.state_commitment.toBigInt();
    const newActionsCommitmentBigInt = transitionData.new_commitment.actions_commitment.toBigInt();
    const newStateCommitmentBigInt = transitionData.new_commitment.state_commitment.toBigInt();


    // Assert exact matches with Sui rollup test values
    assert.strictEqual(oldActionsCommitmentBigInt, 6248033897n, "Old actions commitment should match Sui value");
    assert.strictEqual(oldStateCommitmentBigInt, 0n, "Old state commitment should match Sui value");
    assert.strictEqual(newActionsCommitmentBigInt, 41214508720617712057645573797159612427662368158173771695682160867969771078564n, "New actions commitment should match Sui value");
    assert.strictEqual(newStateCommitmentBigInt, 9328350379599323964930814050805468085812740822563546972518289385183440774483n, "New state commitment should match Sui value");

  });

  it("should have correct field types and structure", () => {
    // Fresh data from current rollup test (same as other tests) 
    const jobCreatedEventData = [
      1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,3,97,100,100,1,0,0,0,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,32,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1,116,105,110,105,1,0,0,0,0,0,0,0,32,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,32,91,30,149,212,112,213,95,240,231,74,108,68,19,170,163,45,205,194,243,248,43,83,174,52,97,32,156,4,113,185,131,164,2,0,0,0,0,0,0,0,32,20,159,168,194,9,171,101,95,212,128,163,175,247,209,109,199,43,106,57,67,228,185,95,207,121,9,244,45,156,23,165,83
    ];

    const transitionData: TransitionData = deserializeTransitionData(jobCreatedEventData);

    // Verify all fields are present and have expected types
    assert.ok(typeof transitionData.block_number === "bigint", "Block number should be bigint");
    assert.ok(typeof transitionData.sequence === "bigint", "Sequence should be bigint");
    assert.ok(typeof transitionData.method === "string", "Method should be string");
    assert.ok(typeof transitionData.index === "number", "Index should be number");
    assert.ok(typeof transitionData.value === "bigint", "Value should be bigint");
    assert.ok(typeof transitionData.old_value === "bigint", "Old value should be bigint");
    assert.ok(typeof transitionData.old_commitment.actions_commitment.toBigInt === "function", "Old actions commitment should be ProvableElement");
    assert.ok(typeof transitionData.old_commitment.state_commitment.toBigInt === "function", "Old state commitment should be ProvableElement");
    assert.ok(typeof transitionData.old_commitment.actions_sequence === "bigint", "Old actions sequence should be bigint");
    assert.ok(typeof transitionData.new_commitment.actions_commitment.toBigInt === "function", "New actions commitment should be ProvableElement");
    assert.ok(typeof transitionData.new_commitment.state_commitment.toBigInt === "function", "New state commitment should be ProvableElement");
    assert.ok(typeof transitionData.new_commitment.actions_sequence === "bigint", "New actions sequence should be bigint");

    // Verify nested structure exists
    assert.ok(transitionData.old_commitment !== undefined, "Old commitment should exist");
    assert.ok(transitionData.new_commitment !== undefined, "New commitment should exist");
    
    // Verify exact values match expected (same as other tests)
    assert.strictEqual(transitionData.block_number, 1n, "Block number should be 1n");
    assert.strictEqual(transitionData.sequence, 0n, "Sequence should be 0n");
    assert.strictEqual(transitionData.method, "add", "Method should be add");
    assert.strictEqual(transitionData.index, 1, "Index should be 1");
    assert.strictEqual(transitionData.value, 1n, "Value should be 1n"); 
    assert.strictEqual(transitionData.old_value, 0n, "Old value should be 0n");
    assert.strictEqual(transitionData.old_commitment.actions_sequence, 1n, "Old actions sequence should be 1n");
    assert.strictEqual(transitionData.new_commitment.actions_sequence, 2n, "New actions sequence should be 2n");

  });
});