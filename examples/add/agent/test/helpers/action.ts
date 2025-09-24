import { Transaction } from "@mysten/sui/transactions";
import { executeTx, waitTx } from "@silvana-one/coordination";
import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";
import { getSuiAddress } from "@silvana-one/coordination";
import { SUI_CLOCK_OBJECT_ID } from "@mysten/sui/utils";

interface JobCreatedEvent {
  agent: string;
  agent_method: string;
  app: string;
  app_instance: string;
  app_instance_method: string;
  created_at: string;
  data: number[];
  description: string;
  developer: string;
  job_sequence: string;
  sequences: any;
  status: {
    variant: string;
    fields: any;
  };
}

interface ActionResult {
  index: number;
  value: bigint;
  new_sum: bigint;
  new_value: bigint;
  old_sum: bigint;
  old_value: bigint;
  old_state_commitment: bigint;
  new_state_commitment: bigint;
  old_actions_commitment: bigint;
  new_actions_commitment: bigint;
  old_actions_sequence: bigint;
  new_actions_sequence: bigint;
  jobCreatedEvent?: JobCreatedEvent;
}

export async function action(params: {
  action: "add" | "multiply";
  value: number;
  index: number;
  appID?: string;
  appInstanceID?: string;
}): Promise<ActionResult> {
  const {
    action,
    value,
    index,
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
  // public fun add(app: &mut App, instance: &mut AppInstance, index: u32, value: u256, clock: &Clock, ctx: &mut TxContext)
  // public fun multiply(app: &mut App, instance: &mut AppInstance, index: u32, value: u256, clock: &Clock, ctx: &mut TxContext)
  const args = [
    tx.object(appID),
    tx.object(appInstanceID),
    tx.pure.u32(index),
    tx.pure.u256(value),
    tx.object(SUI_CLOCK_OBJECT_ID),
  ];

  tx.moveCall({
    package: packageID,
    module: "main",
    function: action,
    arguments: args,
  });

  tx.setSender(address);
  tx.setGasBudget(200_000_000);

  const result = await executeTx({
    tx,
    keyPair,
  });
  if (!result) {
    throw new Error("Failed to create action");
  }
  const { digest } = result;
  const waitResult = await waitTx(digest);
  if (waitResult.errors) {
    console.log(`Errors for tx ${digest}:`, waitResult.errors);
    throw new Error("Transaction failed");
  }

  // waitResult contains the full transaction with events
  const events = waitResult.events;
  if (!events) {
    throw new Error("No events found");
  }
  //console.log("Events:", events);

  let actionResult: ActionResult | null = null;
  let jobCreatedEvent: JobCreatedEvent | null = null;

  for (const event of events) {
    if (
      event.type.endsWith("::main::ValueAddedEvent") ||
      event.type.endsWith("::main::ValueMultipliedEvent")
    ) {
      const json = event.parsedJson as any;

      // Updated for nested CommitmentData structure
      actionResult = {
        index: json.index,
        value: BigInt(json.amount_added || json.multiplier),
        new_sum: BigInt(json.new_sum),
        new_value: BigInt(json.new_value),
        old_sum: BigInt(json.old_sum),
        old_value: BigInt(json.old_value),
        old_actions_commitment: convertCommitment(
          json.old_commitment?.actions_commitment
        ),
        new_actions_commitment: convertCommitment(
          json.new_commitment?.actions_commitment
        ),
        old_state_commitment: convertCommitment(
          json.old_commitment?.state_commitment
        ),
        new_state_commitment: convertCommitment(
          json.new_commitment?.state_commitment
        ),
        old_actions_sequence: BigInt(
          json.old_commitment?.actions_sequence || 0
        ),
        new_actions_sequence: BigInt(
          json.new_commitment?.actions_sequence || 0
        ),
      };
    } else if (event.type.endsWith("::jobs::JobCreatedEvent")) {
      jobCreatedEvent = event.parsedJson as JobCreatedEvent;
    }
  }

  if (!actionResult) {
    throw new Error("No ValueAddedEvent or ValueMultipliedEvent found");
  }

  // Add JobCreatedEvent to the result if found
  if (jobCreatedEvent) {
    actionResult.jobCreatedEvent = jobCreatedEvent;
  }

  return actionResult;
}

function convertCommitment(data: { bytes: Uint8Array }): bigint {
  if (!data?.bytes || !Array.isArray(data.bytes) || data.bytes.length !== 32) {
    throw new Error("Invalid commitment");
  }
  // Convert bytes array to bigint (big-endian interpretation)
  let result = 0n;
  for (let i = 0; i < data.bytes.length; i++) {
    result = (result << 8n) + BigInt(data.bytes[i]);
  }
  return result;
}
