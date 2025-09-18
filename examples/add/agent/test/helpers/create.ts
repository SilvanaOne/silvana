import { Transaction } from "@mysten/sui/transactions";
import {
  executeTx,
  waitTx,
  createTestRegistry,
  getSuiAddress,
  AgentRegistry,
} from "@silvana-one/coordination";
import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
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
  process.env.SUI_KEY = suiSecretKey;

  const packageID = process.env.APP_PACKAGE_ID;
  if (!packageID) {
    throw new Error("APP_PACKAGE_ID is not set");
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
  if (!registryPackageID) {
    throw new Error(
      "SILVANA_REGISTRY_PACKAGE is not set, run silvana config to get it"
    );
  }

  // Initialize keyPair early since we need it for method transactions
  const keyPair = Ed25519Keypair.fromSecretKey(suiSecretKey);
  const address = await getSuiAddress({
    secretKey: suiSecretKey,
  });
  console.log("sender:", address);

  if (!registryAddress) {
    console.log("SILVANA_REGISTRY not set, creating a test registry...");

    const transaction = new Transaction();
    const testRegistry = await AgentRegistry.createAgentRegistry({
      name: "Test Registry for Silvana Add App",
      transaction,
    });

    transaction.setSender(keyPair.toSuiAddress());
    transaction.setGasBudget(100_000_000);

    const registryResult = await executeTx({
      tx: transaction,
      keyPair,
    });

    if (!registryResult) {
      throw new Error("Failed to create registry - no result");
    }

    if (registryResult.error) {
      throw new Error(`Failed to create registry: ${registryResult.error}`);
    }

    if (!registryResult.tx?.objectChanges) {
      throw new Error("Failed to create registry - no object changes");
    }

    // Find the created registry object
    const registryObject = registryResult.tx.objectChanges.find(
      (obj: any) =>
        obj.type === "created" &&
        obj.objectType?.includes("::registry::SilvanaRegistry")
    );

    if (!registryObject || !("objectId" in registryObject)) {
      throw new Error("Failed to find created registry object");
    }

    registryAddress = registryObject.objectId;
    console.log("Registry created with address:", registryAddress);

    if (registryResult.digest) {
      await waitTx(registryResult.digest);
    }

    console.log("Created test registry:", registryAddress);
  }

  // Ensure we have a valid registry address
  if (!registryAddress) {
    throw new Error("Registry address is not set after creation");
  }
  process.env.SILVANA_REGISTRY = registryAddress;

  const registry = new AgentRegistry({ registry: registryAddress });
  const developer = await registry.getDeveloper({ name: developerName });

  if (!developer) {
    if (!process.env.DOCKER_IMAGE) {
      throw new Error("DOCKER_IMAGE is not set");
    }
    console.log("Creating developer...");
    const transaction = new Transaction();
    registry.createDeveloper({
      name: developerName,
      developerOwner: address, // Use the sender's address as the developer owner
      github: "",
      image: "",
      description: "",
      site: "",
      transaction,
    });
    registry.createAgent({
      developer: developerName,
      name: agentName,
      image: "",
      description: "Add Agent",
      site: "",
      chains: ["sui:devnet", "mina:devnet", "zeko:testnet"],
      transaction,
    });

    // Create app
    registry.createApp({
      name: appName,
      owner: address, // Use the sender's address as the app owner
      description: appDescription,
      transaction,
    });

    // Add methods to the app BEFORE creating the app instance
    console.log(
      "Adding methods to agent in registry using docker image:",
      process.env.DOCKER_IMAGE
    );
    registry.addAgentMethod({
      developer: developerName,
      agent: agentName,
      method: "prove",
      dockerImage: process.env.DOCKER_IMAGE,
      dockerSha256: undefined,
      minMemoryGb: 3,
      minCpuCores: 8,
      requiresTee: false,
      transaction,
    });

    console.log("Adding methods to app in registry...");
    registry.addMethodToApp({
      appName,
      methodName: "init",
      description: "Initialize app state",
      developerName,
      agentName,
      agentMethod: "prove",
      transaction,
    });
    registry.addMethodToApp({
      appName,
      methodName: "add",
      description: "Prove addition",
      developerName,
      agentName,
      agentMethod: "prove",
      transaction,
    });

    registry.addMethodToApp({
      appName,
      methodName: "multiply",
      description: "Prove multiplication",
      developerName,
      agentName,
      agentMethod: "prove",
      transaction,
    });

    registry.addMethodToApp({
      appName,
      methodName: "merge",
      description: "Merge proofs",
      developerName,
      agentName,
      agentMethod: "prove",
      transaction,
    });

    registry.addMethodToApp({
      appName,
      methodName: "settle",
      description: "Settle to Mina or Zeko",
      developerName,
      agentName,
      agentMethod: "prove",
      transaction,
    });

    transaction.setSender(keyPair.toSuiAddress());
    transaction.setGasBudget(100_000_000);

    const result = await executeTx({
      tx: transaction,
      keyPair,
    });

    if (!result) {
      throw new Error("Failed to create developer and add methods to app");
    }

    const waitResult = await waitTx(result.digest);
    if (waitResult.errors) {
      console.log(`Errors for method tx ${result.digest}:`, waitResult.errors);
      throw new Error("Failed to create developer and add methods to app");
    }

    console.log("Developer and methods added to registry app successfully");
  }

  const existingApp = await registry.getApp({ name: appName });
  if (existingApp) {
    console.log("App data:", existingApp);
  }

  let appID: string | undefined = undefined;

  // Create app
  const tx = new Transaction();

  // Call create_app with the registry, settlement info, and clock
  // Create vectors for chains and addresses
  const chains = params.chain ? [params.chain] : [];
  const addresses = params.contractAddress
    ? [params.contractAddress] // Will be wrapped as Some(address) in the vector
    : []; // Empty vector if no address

  console.log("Creating app instance:", { registryAddress, chains, addresses });

  const app = tx.moveCall({
    target: `${packageID}::main::create_app`,
    arguments: [
      tx.object(registryAddress), // SilvanaRegistry reference
      tx.pure.vector("string", chains), // vector of settlement chains
      tx.pure("vector<option<string>>", addresses), // vector of Option<String>
      tx.pure("u64", 10 * 60 * 1000), // block creation interval in milliseconds
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

  const transaction = new Transaction();
  registry.addMetadata({
    appInstanceId: appInstanceID,
    key: "settlementAdmin",
    value: params.adminAddress,
    transaction,
  });

  transaction.setSender(address);
  transaction.setGasBudget(100_000_000);

  const metadataResult = await executeTx({
    tx: transaction,
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
