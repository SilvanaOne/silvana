import { Transaction } from "@mysten/sui/transactions";
import {
  executeTx,
  waitTx,
  createTestRegistry,
} from "@silvana-one/coordination";
import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
import { getSuiAddress } from "./key.js";
import { SUI_CLOCK_OBJECT_ID } from "@mysten/sui/utils";

const developerName = "AddDeveloper";
const agentName = "AddAgent";
const appName = "add_app";
const appDescription = "Silvana Add App";

export async function createApp(params: {
  contractAddress: string;
  adminAddress: string;
  chain: string;
  nonce: number;
}): Promise<string> {
  const suiSecretKey: string = process.env.SUI_SECRET_KEY!;

  if (!suiSecretKey) {
    throw new Error("Missing environment variable SUI_SECRET_KEY");
  }

  const packageID = process.env.APP_PACKAGE_ID;
  if (!packageID) {
    throw new Error("PACKAGE_ID is not set");
  }

  if (!params.adminAddress) {
    throw new Error("Missing admin address");
  }

  if (!params.contractAddress) {
    throw new Error("Missing contract address");
  }

  // Get registry from env or create a test one
  let registryAddress = process.env.SILVANA_REGISTRY;
  const registryPackageID = process.env.SILVANA_REGISTRY_PACKAGE;

  // Initialize keyPair early since we need it for method transactions
  const keyPair = Ed25519Keypair.fromSecretKey(suiSecretKey);
  const address = await getSuiAddress({
    secretKey: suiSecretKey,
  });
  console.log("sender:", address);

  if (!registryAddress) {
    console.log("SILVANA_REGISTRY not set, creating a test registry...");

    // Set the registry package ID if provided
    if (registryPackageID) {
      process.env.SILVANA_REGISTRY_PACKAGE = registryPackageID;
    }

    // Set SUI_KEY for createTestRegistry
    process.env.SUI_KEY = suiSecretKey;

    const testRegistry = await createTestRegistry({
      registryName: "Test Registry for Silvana Add App",
      developerName,
      appName,
      appDescription,
      testAgentName: agentName,
      testAgentChains: [
        "sui:testnet",
        "sui:devnet",
        "mina:devnet",
        "zeko:testnet",
      ],
    });

    registryAddress = testRegistry.registryAddress;
    console.log("Created test registry:", registryAddress);

    // Add methods to the app BEFORE creating the app instance
    console.log("Adding methods to agent in registry...");
    const methodTx = new Transaction();

    // Create and add the agent 'prove' method using full registry::add_method interface
    methodTx.moveCall({
      target: `${
        registryPackageID || process.env.SILVANA_REGISTRY_PACKAGE
      }::registry::add_method`,
      arguments: [
        methodTx.object(registryAddress),
        methodTx.pure.string(developerName),
        methodTx.pure.string(agentName),
        methodTx.pure.string("prove"),
        methodTx.pure.string("docker.io/dfstio/add:latest"),
        methodTx.pure.option("string", null),
        methodTx.pure.u16(4),
        methodTx.pure.u16(4),
        methodTx.pure.bool(false),
        methodTx.object(SUI_CLOCK_OBJECT_ID),
      ],
    });

    console.log("Adding methods to app in registry...");

    // Create and add the 'init' method
    const initAppMethod = methodTx.moveCall({
      target: `${
        registryPackageID || process.env.SILVANA_REGISTRY_PACKAGE
      }::app_method::new`,
      arguments: [
        methodTx.pure.option("string", "Initialize app state"),
        methodTx.pure.string(developerName),
        methodTx.pure.string(agentName),
        methodTx.pure.string("prove"),
      ],
    });

    methodTx.moveCall({
      target: `${
        registryPackageID || process.env.SILVANA_REGISTRY_PACKAGE
      }::registry::add_method_to_app`,
      arguments: [
        methodTx.object(registryAddress),
        methodTx.pure.string(appName),
        methodTx.pure.string("init"),
        initAppMethod,
      ],
    });

    // Create and add the 'add' method
    const addAppMethod = methodTx.moveCall({
      target: `${
        registryPackageID || process.env.SILVANA_REGISTRY_PACKAGE
      }::app_method::new`,
      arguments: [
        methodTx.pure.option("string", "Prove addition"), // No description
        methodTx.pure.string(developerName),
        methodTx.pure.string(agentName),
        methodTx.pure.string("prove"),
      ],
    });

    methodTx.moveCall({
      target: `${
        registryPackageID || process.env.SILVANA_REGISTRY_PACKAGE
      }::registry::add_method_to_app`,
      arguments: [
        methodTx.object(registryAddress),
        methodTx.pure.string(appName),
        methodTx.pure.string("add"),
        addAppMethod,
      ],
    });

    // Create and add the 'multiply' method
    const multiplyAppMethod = methodTx.moveCall({
      target: `${
        registryPackageID || process.env.SILVANA_REGISTRY_PACKAGE
      }::app_method::new`,
      arguments: [
        methodTx.pure.option("string", "Prove multiplication"), // No description
        methodTx.pure.string(developerName),
        methodTx.pure.string(agentName),
        methodTx.pure.string("prove"),
      ],
    });

    methodTx.moveCall({
      target: `${
        registryPackageID || process.env.SILVANA_REGISTRY_PACKAGE
      }::registry::add_method_to_app`,
      arguments: [
        methodTx.object(registryAddress),
        methodTx.pure.string(appName),
        methodTx.pure.string("multiply"),
        multiplyAppMethod,
      ],
    });

    // Create and add the 'merge' method
    const mergeAppMethod = methodTx.moveCall({
      target: `${
        registryPackageID || process.env.SILVANA_REGISTRY_PACKAGE
      }::app_method::new`,
      arguments: [
        methodTx.pure.option("string", "Merge proofs"),
        methodTx.pure.string(developerName),
        methodTx.pure.string(agentName),
        methodTx.pure.string("prove"),
      ],
    });

    methodTx.moveCall({
      target: `${
        registryPackageID || process.env.SILVANA_REGISTRY_PACKAGE
      }::registry::add_method_to_app`,
      arguments: [
        methodTx.object(registryAddress),
        methodTx.pure.string(appName),
        methodTx.pure.string("merge"),
        mergeAppMethod,
      ],
    });

    // Create and add the 'settle' method
    const settleAppMethod = methodTx.moveCall({
      target: `${
        registryPackageID || process.env.SILVANA_REGISTRY_PACKAGE
      }::app_method::new`,
      arguments: [
        methodTx.pure.option("string", "Settle to Mina or Zeko"),
        methodTx.pure.string(developerName),
        methodTx.pure.string(agentName),
        methodTx.pure.string("prove"),
      ],
    });

    methodTx.moveCall({
      target: `${
        registryPackageID || process.env.SILVANA_REGISTRY_PACKAGE
      }::registry::add_method_to_app`,
      arguments: [
        methodTx.object(registryAddress),
        methodTx.pure.string(appName),
        methodTx.pure.string("settle"),
        settleAppMethod,
      ],
    });

    methodTx.setSender(keyPair.toSuiAddress());
    methodTx.setGasBudget(100_000_000);

    const methodResult = await executeTx({
      tx: methodTx,
      keyPair,
    });

    if (!methodResult) {
      throw new Error("Failed to add methods to app");
    }

    const methodWaitResult = await waitTx(methodResult.digest);
    if (methodWaitResult.errors) {
      console.log(
        `Errors for method tx ${methodResult.digest}:`,
        methodWaitResult.errors
      );
      throw new Error("Failed to add methods to app");
    }

    console.log("Methods added to registry app successfully");

    // Save for later use in the test
    process.env.TEST_REGISTRY_ADDRESS = registryAddress;
  }

  // Ensure we have a valid registry address
  if (!registryAddress) {
    throw new Error("Registry address is not set after creation");
  }

  let appID: string | undefined = undefined;

  const tx = new Transaction();

  // Call create_app with the registry, settlement info, and clock
  const app = tx.moveCall({
    target: `${packageID}::main::create_app`,
    arguments: [
      tx.object(registryAddress), // SilvanaRegistry reference
      tx.pure.option("string", params.chain), // settlement_chain (e.g., "mina:devnet")
      tx.pure.option("string", params.contractAddress), // settlement_address
      tx.object(SUI_CLOCK_OBJECT_ID), // Clock reference
    ],
  });

  // Transfer the created app to the sender
  tx.transferObjects([app], tx.pure.address(address));

  tx.setSender(address);
  tx.setGasBudget(100_000_000);

  const result = await executeTx({
    tx,
    keyPair,
  });
  if (!result) {
    throw new Error("Failed to create app");
  }
  const { digest } = result;
  const waitResult = await waitTx(digest);
  if (waitResult.errors) {
    console.log(`Errors for tx ${digest}:`, waitResult.errors);
    throw new Error("create app transaction failed");
  }

  // waitResult contains the full transaction details
  const createAppTx = waitResult;

  let appInstanceID: string | undefined = undefined;

  createAppTx.objectChanges?.map((change: any) => {
    if (change.type === "created" && change.objectType) {
      if (change.objectType.includes("::main::App")) {
        appID = change.objectId;
      } else if (change.objectType.includes("::app_instance::AppInstance")) {
        appInstanceID = change.objectId;
      }
    }
  });

  if (!appID) {
    console.error("Failed to find App object in transaction results");
    console.error(
      "Object changes:",
      JSON.stringify(createAppTx.objectChanges, null, 2)
    );
    throw new Error("appId is not set");
  }

  if (!appInstanceID) {
    console.error("Failed to find AppInstance object in transaction results");
    throw new Error("AppInstance ID is not set");
  }

  // Save AppInstance ID for use in tests
  process.env.APP_INSTANCE_ID = appInstanceID;
  console.log("AppInstance ID:", appInstanceID);

  // Add metadata and kv to the AppInstance
  console.log("Adding metadata and kv to AppInstance...");
  const metadataTx = new Transaction();

  // Add contractAddress to metadata
  metadataTx.moveCall({
    target: `${
      registryPackageID || process.env.SILVANA_REGISTRY_PACKAGE
    }::app_instance::add_metadata`,
    arguments: [
      metadataTx.object(appInstanceID),
      metadataTx.pure.string("settlementAdmin"),
      metadataTx.pure.string(params.adminAddress),
    ],
  });

  // Add chain to metadata
  // metadataTx.moveCall({
  //   target: `${
  //     registryPackageID || process.env.SILVANA_REGISTRY_PACKAGE
  //   }::app_instance::add_metadata`,
  //   arguments: [
  //     metadataTx.object(appInstanceID),
  //     metadataTx.pure.string("chain"),
  //     metadataTx.pure.string(params.chain),
  //   ],
  // });

  // Add nonce to kv (convert number to string)
  // metadataTx.moveCall({
  //   target: `${
  //     registryPackageID || process.env.SILVANA_REGISTRY_PACKAGE
  //   }::app_instance::add_kv`,
  //   arguments: [
  //     metadataTx.object(appInstanceID),
  //     metadataTx.pure.string("nonce"),
  //     metadataTx.pure.string(params.nonce.toString()),
  //   ],
  // });

  metadataTx.setSender(address);
  metadataTx.setGasBudget(100_000_000);

  const metadataResult = await executeTx({
    tx: metadataTx,
    keyPair,
  });

  if (!metadataResult) {
    throw new Error("Failed to add metadata and kv");
  }

  const metadataWaitResult = await waitTx(metadataResult.digest);
  if (metadataWaitResult.errors) {
    console.log(
      `Errors for metadata tx ${metadataResult.digest}:`,
      metadataWaitResult.errors
    );
    throw new Error("Failed to add metadata and kv");
  }

  console.log("Metadata and kv added successfully");

  // Initialize the app with the instance
  console.log("Initializing app with instance...");
  const initTx = new Transaction();

  // public fun init_app_with_instance(app: &App, instance: &mut AppInstance, clock: &Clock, ctx: &mut TxContext)
  initTx.moveCall({
    target: `${packageID}::main::init_app_with_instance`,
    arguments: [
      initTx.object(appID),
      initTx.object(appInstanceID),
      initTx.object(SUI_CLOCK_OBJECT_ID),
    ],
  });

  initTx.setSender(address);
  initTx.setGasBudget(100_000_000);

  const initResult = await executeTx({
    tx: initTx,
    keyPair,
  });

  if (!initResult) {
    throw new Error("Failed to initialize app");
  }

  const initWaitResult = await waitTx(initResult.digest);
  if (initWaitResult.errors) {
    console.log(
      `Errors for init tx ${initResult.digest}:`,
      initWaitResult.errors
    );
    throw new Error("Failed to initialize app");
  }

  console.log("App initialized successfully");

  return appID;
}
