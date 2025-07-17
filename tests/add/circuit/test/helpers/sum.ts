import { Transaction } from "@mysten/sui/transactions";
import { suiClient } from "@silvana-one/coordination";
import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
import { getSuiAddress } from "./key.js";

export async function getSum(
  params: {
    appID?: string;
  } = {}
): Promise<number | undefined> {
  const { appID = process.env.APP_OBJECT_ID } = params;
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
  // public fun get_sum(app: &App): u256
  const args = [tx.object(appID)];

  const { Result: sum } = tx.moveCall({
    package: packageID,
    module: "main",
    function: "get_sum",
    arguments: args,
  });

  tx.setSender(address);
  tx.setGasBudget(10_000_000);
  const signedTx = await tx.sign({
    signer: keyPair,
    client: suiClient,
  });
  const txBytes = await tx.build({ client: suiClient });

  const result = await suiClient.dryRunTransactionBlock({
    transactionBlock: txBytes,
  });

  try {
    const sum = parseInt((result?.events[0]?.parsedJson as any)?.sum);
    return sum;
  } catch (error) {
    return undefined;
  }
}
