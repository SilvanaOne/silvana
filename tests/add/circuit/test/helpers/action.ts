import { Transaction } from "@mysten/sui/transactions";
import { executeTx, waitTx } from "@silvana-one/coordination";
import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
import { getSuiAddress } from "./key.js";

export async function action(params: {
  action: "add" | "multiply";
  value: number;
  index: number;
  appID?: string;
}): Promise<{
  index: number;
  value: bigint;
  new_sum: bigint;
  new_value: bigint;
  old_sum: bigint;
  old_value: bigint;
}> {
  const { action, value, index, appID = process.env.APP_OBJECT_ID } = params;
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

  const keyPair = Ed25519Keypair.fromSecretKey(suiSecretKey);
  const address = await getSuiAddress({
    secretKey: suiSecretKey,
  });

  const tx = new Transaction();
  // public fun add(app: &mut App, index: u32, value: u256, ctx: &mut TxContext)
  const args = [tx.object(appID), tx.pure.u32(index), tx.pure.u256(value)];

  tx.moveCall({
    package: packageID,
    module: "main",
    function: action,
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

  // console.log("Created Action:", {
  //   actionTx,
  //   objectChanges: actionTx.objectChanges,
  //   digest,
  //   events,
  // });
  const events = actionTx.events;
  if (!events) {
    throw new Error("No events found");
  }
  //console.log("Events:", events);
  for (const event of events) {
    if (
      event.type.endsWith("::main::ValueAddedEvent") ||
      event.type.endsWith("::main::ValueMultipliedEvent")
    ) {
      const json = event.parsedJson as any;
      //console.log("json", json);
      return {
        index: json.index,
        value: BigInt(json.amount_added || json.multiplier),
        new_sum: BigInt(json.new_sum),
        new_value: BigInt(json.new_value),
        old_sum: BigInt(json.old_sum),
        old_value: BigInt(json.old_value),
      };
    }
  }
  throw new Error("No events found");
}
