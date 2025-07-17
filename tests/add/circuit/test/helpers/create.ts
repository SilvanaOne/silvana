import { Transaction } from "@mysten/sui/transactions";
import { executeTx, waitTx } from "@silvana-one/coordination";
import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
import { getSuiAddress } from "./key.js";
import { SUI_CLOCK_OBJECT_ID } from "@mysten/sui/utils";

export async function createApp(): Promise<string> {
  const suiSecretKey: string = process.env.SUI_SECRET_KEY!;

  if (!suiSecretKey) {
    throw new Error("Missing environment variable SUI_SECRET_KEY");
  }

  const packageID = process.env.APP_PACKAGE_ID;
  if (!packageID) {
    throw new Error("PACKAGE_ID is not set");
  }

  let appID: string | undefined = undefined;
  const keyPair = Ed25519Keypair.fromSecretKey(suiSecretKey);
  const address = await getSuiAddress({
    secretKey: suiSecretKey,
  });

  const tx = new Transaction();

  const args = [tx.object(SUI_CLOCK_OBJECT_ID)];

  const { Result: app } = tx.moveCall({
    package: packageID,
    module: "main",
    function: "create_app",
    arguments: args,
  });
  tx.transferObjects(
    [
      {
        Result: app,
      },
    ],
    address
  );

  tx.setSender(address);
  tx.setGasBudget(100_000_000);

  const result = await executeTx({
    tx,
    keyPair,
  });
  if (!result) {
    throw new Error("Failed to create app");
  }
  const { tx: createAppTx, digest, events } = result;
  const waitResult = await waitTx(digest);
  if (waitResult.errors) {
    console.log(`Errors for tx ${digest}:`, waitResult.errors);
  }
  if (waitResult.errors) {
    console.log(`Errors for tx ${digest}:`, waitResult.errors);
    throw new Error("create app transaction failed");
  }
  createAppTx.objectChanges?.map((change) => {
    if (change.type === "created" && change.objectType.includes("main::App")) {
      appID = change.objectId;
    }
  });
  // console.log("Created App:", {
  //   createAppTx,
  //   objectChanges: createAppTx.objectChanges,
  //   digest,
  //   events,
  //   appID,
  // });
  if (!appID) {
    throw new Error("appId is not set");
  }
  return appID;
}
