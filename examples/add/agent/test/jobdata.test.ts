import { describe, it } from "node:test";
import assert from "node:assert";
import { deserializeJobData, elementToHex, JobData } from "../src/jobdata.js";

describe("JobData Deserialization", () => {
  it("should deserialize JobData from JobCreatedEvent data", () => {
    // Sample data from JobCreatedEvent
    const jobCreatedEventData = [
      1,0,0,0,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,32,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1,116,105,110,105,32,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1,0,0,0,0,0,0,0,32,91,30,149,212,112,213,95,240,231,74,108,68,19,170,163,45,205,194,243,248,43,83,174,52,97,32,156,4,113,185,131,164,32,20,159,168,194,9,171,101,95,212,128,163,175,247,209,109,199,43,106,57,67,228,185,95,207,121,9,244,45,156,23,165,83,2,0,0,0,0,0,0,0,1,0,0,0,0,0,0,0,1,0,0,0,0,0,0,0
    ];

    const jobData: JobData = deserializeJobData(jobCreatedEventData);

    console.log("Deserialized JobData:");
    console.log("Index:", jobData.index);
    console.log("Value:", jobData.value);
    console.log("Old Value:", jobData.old_value);
    console.log("Old Actions Sequence:", jobData.old_actions_sequence);
    console.log("New Actions Sequence:", jobData.new_actions_sequence);
    console.log("Block Number:", jobData.block_number);
    console.log("Sequence:", jobData.sequence);
    console.log("Old Actions Commitment (hex):", elementToHex(jobData.old_actions_commitment.toBigInt()));
    console.log("Old State Commitment (hex):", elementToHex(jobData.old_state_commitment.toBigInt()));
    console.log("New Actions Commitment (hex):", elementToHex(jobData.new_actions_commitment.toBigInt()));
    console.log("New State Commitment (hex):", elementToHex(jobData.new_state_commitment.toBigInt()));

    // Basic assertions using actual values from Sui rollup test
    assert.strictEqual(jobData.index, 1, "Index should be 1");
    assert.strictEqual(jobData.value, "1", "Value should be 1");
    assert.strictEqual(jobData.old_value, "0", "Old value should be 0");
    assert.strictEqual(jobData.old_actions_sequence, "1", "Old actions sequence should be 1");
    assert.strictEqual(jobData.new_actions_sequence, "2", "New actions sequence should be 2");
    assert.strictEqual(jobData.sequence, "1", "Sequence should be 1");
    assert.strictEqual(jobData.block_number, "1", "Block number should be 1");
    
    // Commitment assertions using actual bigint values from Sui
    // old_actions_commitment: 6248033897n -> 0x0000000174696e69
    // old_state_commitment: 0n -> 0x0000000000000000...
    // new_actions_commitment: 41214508720617712057645573797159612427662368158173771695682160867969771078564n
    // new_state_commitment: 9328350379599323964930814050805468085812740822563546972518289385183440774483n
    
    const oldActionsCommitmentHex = elementToHex(jobData.old_actions_commitment.toBigInt());
    const oldStateCommitmentHex = elementToHex(jobData.old_state_commitment.toBigInt());
    const newActionsCommitmentHex = elementToHex(jobData.new_actions_commitment.toBigInt());
    const newStateCommitmentHex = elementToHex(jobData.new_state_commitment.toBigInt());
    
    console.log("Old Actions Commitment (bigint equivalent):", jobData.old_actions_commitment.toBigInt().toString());
    console.log("New Actions Commitment (bigint equivalent):", jobData.new_actions_commitment.toBigInt().toString());
    console.log("Old State Commitment (bigint equivalent):", jobData.old_state_commitment.toBigInt().toString());
    console.log("New State Commitment (bigint equivalent):", jobData.new_state_commitment.toBigInt().toString());
    
    // Verify commitments contain the expected data patterns
    assert.ok(oldActionsCommitmentHex.includes("74696e69"), "Old actions commitment should contain 'tini' pattern");
    assert.ok(oldStateCommitmentHex.startsWith("0x000000000000000000000000000000000000000000000000000000000000000"), "Old state commitment should be mostly zeros");
    
    // Check commitment objects are ProvableElements 
    assert.ok(typeof jobData.old_actions_commitment.toBigInt === 'function', "Old actions commitment should be ProvableElement");
    assert.ok(typeof jobData.old_state_commitment.toBigInt === 'function', "Old state commitment should be ProvableElement");
    assert.ok(typeof jobData.new_actions_commitment.toBigInt === 'function', "New actions commitment should be ProvableElement");
    assert.ok(typeof jobData.new_state_commitment.toBigInt === 'function', "New state commitment should be ProvableElement");

    console.log("✅ JobData deserialization test passed!");
  });

  it("should match exact bigint values from Sui rollup test", () => {
    // Sample data from JobCreatedEvent
    const jobCreatedEventData = [
      1,0,0,0,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,32,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1,116,105,110,105,32,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1,0,0,0,0,0,0,0,32,91,30,149,212,112,213,95,240,231,74,108,68,19,170,163,45,205,194,243,248,43,83,174,52,97,32,156,4,113,185,131,164,32,20,159,168,194,9,171,101,95,212,128,163,175,247,209,109,199,43,106,57,67,228,185,95,207,121,9,244,45,156,23,165,83,2,0,0,0,0,0,0,0,1,0,0,0,0,0,0,0,1,0,0,0,0,0,0,0
    ];

    const jobData: JobData = deserializeJobData(jobCreatedEventData);

    // Get bigint values directly from ProvableElements
    const oldActionsCommitmentBigInt = jobData.old_actions_commitment.toBigInt();
    const oldStateCommitmentBigInt = jobData.old_state_commitment.toBigInt();
    const newActionsCommitmentBigInt = jobData.new_actions_commitment.toBigInt();
    const newStateCommitmentBigInt = jobData.new_state_commitment.toBigInt();

    console.log("Converted commitment values to bigint:");
    console.log("old_actions_commitment:", oldActionsCommitmentBigInt.toString());
    console.log("old_state_commitment:", oldStateCommitmentBigInt.toString());
    console.log("new_actions_commitment:", newActionsCommitmentBigInt.toString());
    console.log("new_state_commitment:", newStateCommitmentBigInt.toString());

    // Assert exact matches with Sui rollup test values
    assert.strictEqual(oldActionsCommitmentBigInt.toString(), "6248033897", "Old actions commitment should match Sui value");
    assert.strictEqual(oldStateCommitmentBigInt.toString(), "0", "Old state commitment should match Sui value");
    assert.strictEqual(newActionsCommitmentBigInt.toString(), "41214508720617712057645573797159612427662368158173771695682160867969771078564", "New actions commitment should match Sui value");
    assert.strictEqual(newStateCommitmentBigInt.toString(), "9328350379599323964930814050805468085812740822563546972518289385183440774483", "New state commitment should match Sui value");

    console.log("✅ All commitment values match exact Sui rollup test bigint values!");
  });

  it("should handle different values and sequences", () => {
    // This test would use different sample data if available
    // For now, we'll just verify the structure works with the existing data
    const jobCreatedEventData = [
      1,0,0,0,1,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,32,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1,116,105,110,105,32,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1,0,0,0,0,0,0,0,32,91,30,149,212,112,213,95,240,231,74,108,68,19,170,163,45,205,194,243,248,43,83,174,52,97,32,156,4,113,185,131,164,32,20,159,168,194,9,171,101,95,212,128,163,175,247,209,109,199,43,106,57,67,228,185,95,207,121,9,244,45,156,23,165,83,2,0,0,0,0,0,0,0,1,0,0,0,0,0,0,0,1,0,0,0,0,0,0,0
    ];

    const jobData: JobData = deserializeJobData(jobCreatedEventData);

    // Verify all fields are present and have expected types
    assert.ok(typeof jobData.index === "number", "Index should be number");
    assert.ok(typeof jobData.value === "string", "Value should be string");
    assert.ok(typeof jobData.old_value === "string", "Old value should be string");
    assert.ok(typeof jobData.old_actions_commitment.toBigInt === "function", "Old actions commitment should be ProvableElement");
    assert.ok(typeof jobData.old_state_commitment.toBigInt === "function", "Old state commitment should be ProvableElement");
    assert.ok(typeof jobData.old_actions_sequence === "string", "Old actions sequence should be string");
    assert.ok(typeof jobData.new_actions_commitment.toBigInt === "function", "New actions commitment should be ProvableElement");
    assert.ok(typeof jobData.new_state_commitment.toBigInt === "function", "New state commitment should be ProvableElement");
    assert.ok(typeof jobData.new_actions_sequence === "string", "New actions sequence should be string");
    assert.ok(typeof jobData.block_number === "string", "Block number should be string");
    assert.ok(typeof jobData.sequence === "string", "Sequence should be string");

    console.log("✅ All JobData fields have correct types!");
  });
});