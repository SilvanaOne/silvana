import { describe, it } from "node:test";
import assert from "node:assert";
import { createApp } from "./helpers/create.js";
import fs from "node:fs/promises";
import path from "node:path";

describe("Deploy App for Coordinator", async () => {
  it("should create app and save to .env.app", async () => {
    console.log("🚀 Creating new app for coordinator testing...");

    // Get package ID from environment
    const packageID = process.env.APP_PACKAGE_ID;
    assert.ok(packageID !== undefined, "APP_PACKAGE_ID is not set");
    console.log("📦 Package ID:", packageID);

    // Create the app
    const appID = await createApp();
    assert.ok(appID !== undefined, "appID is not set");

    console.log("✅ App created successfully!");
    console.log("📦 App ID:", appID);

    // Prepare .env.app content
    const envContent = [
      `# Coordinator Test Environment`,
      `# Generated at: ${new Date().toISOString()}`,
      ``,
      `# App Configuration`,
      `APP_OBJECT_ID=${appID}`,
      `APP_PACKAGE_ID=${packageID}`,
      ``,
      `# Copy these to your coordinator .env:`,
      `COORDINATION_PACKAGE_ID=${packageID}`,
      `COORDINATION_MODULE=main`,
      ``,
      `# Chain Configuration`,
      `SUI_CHAIN=${process.env.SUI_CHAIN || "devnet"}`,
      `SUI_ADDRESS=${process.env.SUI_ADDRESS}`,
      `SUI_SECRET_KEY=${process.env.SUI_SECRET_KEY}`,
      ``,
    ].join("\n");

    // Write to .env.app file
    const envPath = path.join(process.cwd(), ".env.app");
    await fs.writeFile(envPath, envContent, "utf-8");

    console.log("💾 Configuration saved to .env.app");
    console.log("\n📋 Next steps:");
    console.log("1. Copy COORDINATION_PACKAGE_ID to your coordinator's .env");
    console.log("2. Start the coordinator: cargo run --bin coordinator");
    console.log("3. Run send.test.ts to generate events");

    return appID;
  });
});
