import { Transaction } from "@mysten/sui/transactions";
import { executeTx, waitTx, getSuiAddress } from "@silvana-one/coordination";
import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
import { SUI_CLOCK_OBJECT_ID } from "@mysten/sui/utils";

export async function purge(params: {
  proved_sequence: number;
  appID?: string;
  appInstanceID?: string;
}) {
  const {
    proved_sequence,
    appID = process.env.APP_OBJECT_ID,
    appInstanceID = process.env.APP_INSTANCE_ID,
  } = params;
  const suiSecretKey: string = process.env.SUI_SECRET_KEY!;

  if (!suiSecretKey) {
    throw new Error("Missing environment variable SUI_SECRET_KEY");
  }

  const packageID = process.env.APP_PACKAGE_ID;
  if (!packageID) {
    throw new Error("PACKAGE_ID is not set");
  }
  if (!appID) {
    throw new Error("APP_OBJECT_ID is not set");
  }
  if (!appInstanceID) {
    throw new Error("APP_INSTANCE_ID is not set");
  }

  const keyPair = Ed25519Keypair.fromSecretKey(suiSecretKey);
  const address = await getSuiAddress({
    secretKey: suiSecretKey,
  });

  const tx = new Transaction();
  // public fun purge_rollback_records(app: &mut App, instance: &mut AppInstance, proved_sequence: u64, clock: &Clock)
  const args = [
    tx.object(appID),
    tx.object(appInstanceID),
    tx.pure.u64(proved_sequence),
    tx.object(SUI_CLOCK_OBJECT_ID),
  ];

  tx.moveCall({
    package: packageID,
    module: "main",
    function: "purge_rollback_records",
    arguments: args,
  });

  tx.setSender(address);
  tx.setGasBudget(100_000_000);

  const result = await executeTx({
    tx,
    keyPair,
  });
  if (!result) {
    throw new Error("Failed to create action");
  }
  const { tx: actionTx, digest } = result;
  const waitResult = await waitTx(digest);
  if (waitResult.errors) {
    console.log(`Errors for tx ${digest}:`, waitResult.errors);
  }

  // console.log("Purged Rollback Records:", {
  //   actionTx,
  //   objectChanges: actionTx.objectChanges,
  //   digest,
  //   events,
  // });
}
