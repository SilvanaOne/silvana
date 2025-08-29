import { describe, it } from "node:test";
import assert from "node:assert";
import { AddProgram } from "../src/circuit.js";
import { Cache, VerificationKey } from "o1js";
import { AddContract } from "../src/contract.js";
import { initBlockchain } from "@silvana-one/mina-utils";

let vk: VerificationKey | undefined = undefined;

describe("Add Rollup", async () => {
  it.skip("should get ZkProgram constraints", async () => {
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
    // Initialize blockchain connection
    await initBlockchain("devnet");
    const cache = Cache.FileSystem("./cache");
    console.log("compiling...");
    console.time("compiled AddProgram");
    vk = (await AddProgram.compile({ cache })).verificationKey;
    console.timeEnd("compiled AddProgram");
    assert.ok(vk !== undefined, "vk is not set");
    console.log("vk", vk.hash.toJSON());
    console.time("compiled AddContract");
    const vkContract = (await AddContract.compile({ cache })).verificationKey;
    console.timeEnd("compiled AddContract");
    assert.ok(vkContract !== undefined, "vkContract is not set");
    console.log("vkContract", vkContract.hash.toJSON());
  });
});
