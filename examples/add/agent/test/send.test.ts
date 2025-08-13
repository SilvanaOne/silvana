import { describe, it } from "node:test";
import assert from "node:assert";
import { action } from "./helpers/action.js";
import fs from "node:fs/promises";
import path from "node:path";
import dotenv from "dotenv";

// Load .env.app if it exists
const envAppPath = path.join(process.cwd(), '.env.app');
try {
  const envAppContent = await fs.readFile(envAppPath, 'utf-8');
  const envAppParsed = dotenv.parse(envAppContent);
  // Merge with process.env, giving priority to .env.app values
  Object.assign(process.env, envAppParsed);
  console.log("✅ Loaded configuration from .env.app");
} catch (err) {
  console.warn("⚠️  Could not load .env.app - make sure to run deploy.test.ts first");
}

describe("Send Actions to Generate Events", async () => {
  const appID = process.env.APP_OBJECT_ID;
  
  it("should verify app exists", async () => {
    assert.ok(appID !== undefined, "APP_OBJECT_ID is not set - run deploy.test.ts first");
    console.log("📦 Using App ID:", appID);
    console.log("📦 Using Package ID:", process.env.APP_PACKAGE_ID);
  });

  it("should send random actions every 20 seconds", async () => {
    console.log("🎲 Starting random action generator...");
    console.log("⏱️  Will send an action every 20 seconds");
    console.log("🛑 Press Ctrl+C to stop\n");
    
    let actionCount = 0;
    let currentSum = 0n;
    
    // Keep track of values at different indices for variety
    const indices = [1, 2, 3, 4, 5];
    const values: Map<number, bigint> = new Map();
    indices.forEach(i => values.set(i, 0n));
    
    // Continuous loop - will run until manually stopped
    while (true) {
      try {
        // Randomly choose action type
        const actionType = Math.random() < 0.5 ? "add" : "multiply";
        
        // Randomly choose index (1-5)
        const index = indices[Math.floor(Math.random() * indices.length)];
        
        // Generate appropriate random value
        let value: number;
        if (actionType === "add") {
          // Random value between 1 and 100 for addition
          value = Math.floor(Math.random() * 100) + 1;
        } else {
          // Random value between 2 and 10 for multiplication
          value = Math.floor(Math.random() * 9) + 2;
        }
        
        actionCount++;
        console.log(`\n🚀 Action #${actionCount}`);
        console.log(`   Type: ${actionType}`);
        console.log(`   Index: ${index}`);
        console.log(`   Value: ${value}`);
        console.log(`   Current value at index ${index}: ${values.get(index)}`);
        
        const startTime = Date.now();
        
        // Send the action
        const result = await action({
          action: actionType,
          value,
          index,
          appID,
        });
        
        const duration = Date.now() - startTime;
        
        // Update our local tracking
        values.set(index, result.new_value);
        currentSum = result.new_sum;
        
        console.log(`✅ Action completed in ${duration}ms`);
        console.log(`   Old sum: ${result.old_sum}`);
        console.log(`   New sum: ${result.new_sum}`);
        console.log(`   Old value at index ${index}: ${result.old_value}`);
        console.log(`   New value at index ${index}: ${result.new_value}`);
        console.log(`   Actions sequence: ${result.new_actions_sequence}`);
        
        // Display current state
        console.log(`\n📊 Current State:`);
        console.log(`   Total sum: ${currentSum}`);
        console.log(`   Values: ${Array.from(values.entries())
          .map(([i, v]) => `[${i}]=${v}`)
          .join(', ')}`);
        
        // Wait 20 seconds before next action
        console.log(`\n⏱️  Waiting 20 seconds before next action...`);
        await new Promise(resolve => setTimeout(resolve, 20000));
        
      } catch (error) {
        console.error("❌ Error sending action:", error);
        console.log("⏱️  Retrying in 20 seconds...");
        await new Promise(resolve => setTimeout(resolve, 20000));
      }
    }
  });
});