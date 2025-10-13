import { describe, it } from "node:test";
import assert from "node:assert";
import { createApp } from "./helpers/create.js";
import { deployAddContract } from "../src/deploy.js";
import fs from "node:fs/promises";
import path from "node:path";

describe("Deploy App for Coordinator", async () => {
  it("should create app and save to .env.app", async () => {
    console.log("🚀 Deploying AddContract to Mina...");

    try {
      const { contractAddress, adminAddress, txHash, verificationKey, nonce } =
        await deployAddContract();
      console.log("✅ AddContract deployed successfully!");
      console.log(`  Contract Address: ${contractAddress}`);
      console.log(`  Transaction Hash: ${txHash}`);
      console.log(`  Verification Key Hash: ${verificationKey.hash.toJSON()}`);
      console.log(`  Next Nonce: ${nonce}`);

      // Save contract info to environment for later use
      process.env.ADD_CONTRACT_ADDRESS = contractAddress;
      process.env.ADD_CONTRACT_ADMIN_ADDRESS = adminAddress;
      process.env.ADD_CONTRACT_TX_HASH = txHash;
      process.env.ADD_CONTRACT_NONCE = nonce.toString();
    } catch (error) {
      console.error("❌ AddContract deployment failed:", error);
      console.log("⚠️ Continuing with Sui app deployment...");
    }

    console.log("\n🚀 Creating new app for coordinator testing...");

    // Get package ID from environment
    const packageID = process.env.APP_PACKAGE_ID;
    assert.ok(packageID !== undefined, "APP_PACKAGE_ID is not set");
    console.log("📦 Package ID:", packageID);
    const contractAddress = process.env.ADD_CONTRACT_ADDRESS;
    const adminAddress = process.env.ADD_CONTRACT_ADMIN_ADDRESS;
    const chain = process.env.MINA_CHAIN;
    if (!contractAddress || !adminAddress || !chain) {
      throw new Error("AddContract deployment failed");
    }

    // Create the app with Mina contract info
    const nonce = process.env.ADD_CONTRACT_NONCE
      ? parseInt(process.env.ADD_CONTRACT_NONCE)
      : 0;

    const appID = await createApp({
      contractAddress,
      adminAddress,
      chain,
      nonce,
    });
    assert.ok(appID !== undefined, "appID is not set");

    console.log("✅ App created successfully!");
    console.log("📦 App ID:", appID);
    console.log(
      "📦 Registry:",
      process.env.SILVANA_REGISTRY || "Created test registry"
    );

    // Get registry info - if it was created during the test, it will be in the environment
    // The createApp function sets it if it creates a new registry
    const registryAddress =
      process.env.SILVANA_REGISTRY ||
      process.env.TEST_REGISTRY_ADDRESS ||
      "[Registry was created during test]";
    const registryPackageID = process.env.SILVANA_REGISTRY_PACKAGE || "";
    const appInstanceID = process.env.APP_INSTANCE_ID || "";

    // Prepare .env.app content
    const envContent = [
      `# Coordinator Test Environment`,
      `# Generated at: ${new Date().toISOString()}`,
      ``,
      `# App Configuration`,
      `APP_OBJECT_ID=${appID}`,
      `APP_INSTANCE_ID=${appInstanceID}`,
      `APP_PACKAGE_ID=${packageID}`,
      ``,
      `# Registry Configuration`,
      `SILVANA_REGISTRY=${registryAddress}`,
      `SILVANA_REGISTRY_PACKAGE=${registryPackageID}`,
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
      `# Mina Contract Configuration`,
      `MINA_CHAIN=${process.env.MINA_CHAIN || "mina:devnet"}`,
      process.env.ADD_CONTRACT_ADDRESS
        ? `ADD_CONTRACT_ADDRESS=${process.env.ADD_CONTRACT_ADDRESS}`
        : `# ADD_CONTRACT_ADDRESS=<not deployed>`,
      process.env.ADD_CONTRACT_TX_HASH
        ? `ADD_CONTRACT_TX_HASH=${process.env.ADD_CONTRACT_TX_HASH}`
        : `# ADD_CONTRACT_TX_HASH=<not deployed>`,
      `MINA_APP_ADMIN=${process.env.MINA_APP_ADMIN}`,
      ``,
    ].join("\n");

    // Write to .env.app file
    const envPath = path.join(process.cwd(), ".env.app");
    await fs.writeFile(envPath, envContent, "utf-8");

    console.log("💾 Configuration saved to .env.app");
    console.log("App instance ID:", appInstanceID);
    console.log("Mina contract address:", contractAddress);
    console.log("Mina admin address:", adminAddress);
    console.log("\n📋 Next steps:");
    console.log(
      "Copy SILVANA_REGISTRY_PACKAGE and SILVANA_REGISTRY to your coordinator's .env"
    );
    console.log("\n📦 Registry Details:");
    console.log(`SILVANA_REGISTRY=${registryAddress}`);
    console.log(`SILVANA_REGISTRY_PACKAGE=${registryPackageID}`);
  });
});
