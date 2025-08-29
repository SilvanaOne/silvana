import { JsonProof, verify, PrivateKey, PublicKey, Mina, Cache } from "o1js";
import { AddProgramProof } from "./circuit.js";
import { compile } from "./state.js";
import { AddContract } from "./contract.js";
import { checkAddContractDeployment } from "./deploy.js";
import {
  initBlockchain,
  fetchMinaAccount,
  accountBalanceMina,
  sendTx,
} from "@silvana-one/mina-utils";
import {
  getBlockProof,
  setKv,
  getKv,
  getMetadata,
  getSecret,
  updateBlockStateDataAvailability,
  updateBlockProofDataAvailability,
  updateBlockSettlementTxHash,
} from "./grpc.js";

interface SettleParams {
  privateKey?: string; // Optional - will use secret if not provided
  contractAddress?: string; // Optional - will use settlement address from metadata if not provided
}

export async function settle(params: SettleParams): Promise<void> {
  console.log("🚀 Starting settlement process...");

  // Fetch settlement_admin metadata to get admin address and contract info
  console.log("🔍 Fetching settlement admin metadata...");
  const metadataResponse = await getMetadata("settlementAdmin");

  if (!metadataResponse.success) {
    throw new Error(
      `Failed to fetch settlement admin metadata: ${metadataResponse.message}`
    );
  }

  // Get the admin address from metadata value (if present) or use the AppInstance admin field
  let settlementAdminAddress = metadataResponse.value;
  const contractAddress =
    metadataResponse.settlementAddress || params.contractAddress;

  // Print AppInstance info
  console.log("📊 AppInstance Information:");
  console.log(`  - Instance ID: ${metadataResponse.appInstanceId}`);
  console.log(`  - App Name: ${metadataResponse.silvanaAppName}`);
  console.log(`  - Admin Address: ${metadataResponse.admin}`);
  console.log(`  - Settlement Admin: ${settlementAdminAddress ?? "none"}`);
  console.log(
    `  - Settlement Chain: ${metadataResponse.settlementChain ?? "none"}`
  );
  console.log(`  - Contract Address: ${contractAddress ?? "none"}`);
  console.log(`  - Current Sequence: ${metadataResponse.sequence}`);
  console.log(`  - Current Block: ${metadataResponse.blockNumber}`);
  console.log(
    `  - Last Proved Block: ${metadataResponse.lastProvedBlockNumber}`
  );
  console.log(
    `  - Last Settled Block: ${metadataResponse.lastSettledBlockNumber}`
  );

  if (!contractAddress) {
    throw new Error(
      "No contract address found in settlement address or params"
    );
  }

  if (!settlementAdminAddress) {
    if (params.privateKey) {
      console.log(
        `🔑 Using provided private key to derive settlement admin address...`
      );
      settlementAdminAddress = PrivateKey.fromBase58(params.privateKey)
        .toPublicKey()
        .toBase58();
      console.log(`  - Derived Settlement Admin: ${settlementAdminAddress}`);
    } else {
      throw new Error("No settlement admin address found in metadata");
    }
  }

  // Get the admin's private key - either from params or from secrets
  let senderPrivateKey: PrivateKey;

  if (params.privateKey) {
    console.log(`🔑 Using provided private key...`);
    senderPrivateKey = PrivateKey.fromBase58(params.privateKey);
  } else {
    console.log(
      `🔑 Retrieving admin private key for ${settlementAdminAddress}...`
    );
    const adminPrivateKeySecret = await getSecret(
      `sk_${settlementAdminAddress}`
    );

    if (!adminPrivateKeySecret) {
      throw new Error(
        `Failed to retrieve private key for admin ${settlementAdminAddress}`
      );
    }

    senderPrivateKey = PrivateKey.fromBase58(adminPrivateKeySecret);
  }

  const senderPublicKey = senderPrivateKey.toPublicKey();
  if (senderPublicKey.toBase58() !== settlementAdminAddress) {
    throw new Error(
      `Public key ${senderPublicKey.toBase58()} does not match settlement admin address ${settlementAdminAddress}`
    );
  }

  console.log(`✅ Admin public key verified: ${senderPublicKey.toBase58()}`);

  // Initialize blockchain for devnet
  await initBlockchain("devnet");

  // Check that the contract is deployed
  console.log("🔍 Checking contract deployment...");
  const isDeployed = await checkAddContractDeployment({
    contractAddress,
    adminAddress: settlementAdminAddress,
  });
  if (!isDeployed) {
    throw new Error(
      `Contract at ${contractAddress} is not deployed or not accessible`
    );
  }
  console.log("✅ Contract is deployed and accessible");

  // Create contract instance and fetch current state
  const contractPublicKey = PublicKey.fromBase58(contractAddress);
  const contract = new AddContract(contractPublicKey);

  // Fetch the contract state
  await fetchMinaAccount({ publicKey: contractPublicKey, force: true });
  await fetchMinaAccount({ publicKey: senderPublicKey, force: true });

  // Get the last settled block number from contract
  const lastSettledBlock = contract.blockNumber.get().toBigInt();
  console.log(`📊 Last settled block: ${lastSettledBlock}`);

  // Check sender balance
  const balance = await accountBalanceMina(senderPublicKey);
  console.log(`💰 Sender balance: ${balance} MINA`);
  if (balance < 0.2) {
    throw new Error(
      `Insufficient balance. Need at least 0.2 MINA, have ${balance} MINA`
    );
  }

  // Ensure the circuit is compiled
  console.log("📦 Compiling circuit...");
  const cache = Cache.FileSystem("./cache");
  const vk = await compile();
  console.log("vk AddProgram", vk.hash.toJSON());

  // Compile the contract as well
  console.log("📦 Compiling contract...");
  console.time("compiled contract");
  const vkContract = (await AddContract.compile({ cache })).verificationKey;
  console.timeEnd("compiled contract");
  console.log("vk AddContract", vkContract.hash.toJSON());

  // Start iterating from the next block
  let currentBlockNumber = lastSettledBlock + 1n;
  let nonce: number | null = null;
  const NONCE_KEY = `nonce_${senderPublicKey.toBase58()}`;

  console.log(`🔄 Starting settlement from block ${currentBlockNumber}`);

  while (true) {
    console.log(`\n📦 Processing block ${currentBlockNumber}...`);

    // Fetch block proof
    const blockProofResponse = await getBlockProof(currentBlockNumber);
    const blockProofSerialized = blockProofResponse.success
      ? blockProofResponse.blockProof
      : null;

    if (!blockProofSerialized) {
      console.log(`No proof available for block ${currentBlockNumber}`);
      console.log("✅ Settlement complete - reached latest proven block");
      break;
    }

    console.log(
      `✅ Block proof fetched (${blockProofSerialized.length} chars)`
    );

    // Parse and verify the proof
    const proofData = JSON.parse(blockProofSerialized);
    const proofJson = proofData.proof;

    // Deserialize the proof
    console.log("🔐 Deserializing and verifying block proof...");
    const blockProof: AddProgramProof = await AddProgramProof.fromJSON(
      JSON.parse(proofJson) as JsonProof
    );

    // Verify the proof
    const isValid = await verify(blockProof, vk);
    if (!isValid) {
      throw new Error(
        `Block proof verification failed for block ${currentBlockNumber}`
      );
    }
    console.log("✅ Block proof verified successfully");

    // Update block state data availability on Sui (using the same serialized data that contains both proof and state)
    console.log("📝 Updating block state data availability on Sui...");
    try {
      const updateStateDAResponse = await updateBlockStateDataAvailability(
        currentBlockNumber,
        blockProofSerialized // The serialized proof contains both proof and state data
      );

      if (!updateStateDAResponse.success) {
        throw new Error(
          `Failed to update block state DA: ${
            updateStateDAResponse.message || "Unknown error"
          }`
        );
      }

      console.log(
        `✅ Block state DA updated on Sui for block ${currentBlockNumber}`
      );
      console.log(
        `  Response: success=${updateStateDAResponse.success}, message=${updateStateDAResponse.message}, txHash=${updateStateDAResponse.txHash}`
      );
    } catch (error: any) {
      console.error(
        `❌ Failed to update block state DA on Sui: ${error.message}`
      );
      throw error; // Re-throw to stop settlement if state DA update fails
    }

    // Update block proof data availability on Sui
    console.log("📝 Updating block proof data availability on Sui...");
    try {
      // Use the serialized proof string we already have
      const updateDAResponse = await updateBlockProofDataAvailability(
        currentBlockNumber,
        blockProofSerialized
      );

      if (!updateDAResponse.success) {
        throw new Error(
          `Failed to update block proof DA: ${
            updateDAResponse.message || "Unknown error"
          }`
        );
      }

      console.log(
        `✅ Block proof DA updated on Sui for block ${currentBlockNumber}`
      );
      console.log(
        `  Response: success=${updateDAResponse.success}, message=${updateDAResponse.message}, txHash=${updateDAResponse.txHash}`
      );
    } catch (error: any) {
      console.error(
        `❌ Failed to update block proof DA on Sui: ${error.message}`
      );
      throw error; // Re-throw to stop settlement if DA update fails
    }

    // Extract proof details
    console.log("📊 Block proof details:");
    console.log(
      `  - Block Number (publicInput): ${blockProof.publicInput.blockNumber.toBigInt()}`
    );
    console.log(
      `  - Sequence (publicInput): ${blockProof.publicInput.sequence.toBigInt()}`
    );
    console.log(
      `  - Sum (publicInput): ${blockProof.publicInput.sum.toBigInt()}`
    );
    console.log(
      `  - Block Number (publicOutput): ${blockProof.publicOutput.blockNumber.toBigInt()}`
    );
    console.log(
      `  - Sequence (publicOutput): ${blockProof.publicOutput.sequence.toBigInt()}`
    );
    console.log(
      `  - Sum (publicOutput): ${blockProof.publicOutput.sum.toBigInt()}`
    );

    if (blockProof.publicOutput.blockNumber.toBigInt() !== currentBlockNumber) {
      throw new Error(
        `Block number mismatch: ${blockProof.publicOutput.blockNumber.toBigInt()} !== ${currentBlockNumber}`
      );
    }
    if (
      blockProof.publicInput.blockNumber.toBigInt() !==
      blockProof.publicOutput.blockNumber.toBigInt()
    ) {
      throw new Error(
        `Block number mismatch input-output: ${blockProof.publicInput.blockNumber.toBigInt()} !== ${blockProof.publicOutput.blockNumber.toBigInt()}`
      );
    }

    // Initialize nonce on first proof (after verification)
    if (nonce === null) {
      console.log("🔢 Initializing nonce...");

      // Read saved nonce
      const savedNonceResponse = await getKv(NONCE_KEY);
      const savedNonceStr = savedNonceResponse.success
        ? savedNonceResponse.value
        : null;
      const savedNonce = savedNonceStr ? parseInt(savedNonceStr, 10) : 0;

      // Fetch fresh account state
      await fetchMinaAccount({ publicKey: senderPublicKey, force: true });
      const onChainNonce = Number(
        Mina.getAccount(senderPublicKey).nonce.toBigint()
      );

      // Use the highest nonce
      nonce = Math.max(onChainNonce, savedNonce);
      console.log(`  On-chain nonce: ${onChainNonce}`);
      console.log(`  Saved nonce: ${savedNonce}`);
      console.log(`  Using nonce: ${nonce}`);
    }

    // Create and send settlement transaction
    console.log("📝 Creating settlement transaction...");
    const memo = `Settle block ${currentBlockNumber}`;
    console.time("prepared tx");

    // Fetch the contract state
    await fetchMinaAccount({ publicKey: contractPublicKey, force: true });

    try {
      const tx = await Mina.transaction(
        {
          sender: senderPublicKey,
          fee: 200_000_000, // 0.2 MINA
          memo: memo.substring(0, 30),
          nonce,
        },
        async () => {
          await contract.settle(blockProof);
        }
      );
      console.timeEnd("prepared tx");
      // Prove the transaction
      console.log("🔐 Proving transaction...");
      console.time("proved tx");
      await tx.prove();
      console.timeEnd("proved tx");

      // Sign and send the transaction
      console.log("📤 Sending transaction...");
      console.time("sent tx");
      const sentTx = await sendTx({
        tx: tx.sign([senderPrivateKey]),
        description: `Silvana AddContract: settle block ${currentBlockNumber}`,
        wait: false,
        verbose: true,
        retry: 3,
      });
      console.timeEnd("sent tx");
      if (
        !sentTx ||
        !sentTx.status ||
        sentTx.status !== "pending" ||
        !sentTx.hash
      ) {
        console.error("❌ Transaction failed:", sentTx);

        // Reset nonce on failure
        console.log("🔄 Resetting nonce to 0 due to failure");
        await setKv(NONCE_KEY, "0");

        throw new Error(
          `Settlement transaction failed for block ${currentBlockNumber}: ${
            sentTx?.status ?? "status unknown"
          } ${sentTx?.hash ?? "hash unknown"} ${
            sentTx && "errors" in sentTx ? sentTx.errors : "no errors"
          }`
        );
      }

      // Transaction successful
      const txHash = sentTx.hash;
      console.log(`✅ Transaction sent successfully!`);
      console.log(`  Transaction Hash: ${txHash}`);

      // Save transaction hash to blockchain
      const updateResult = await updateBlockSettlementTxHash(
        currentBlockNumber,
        txHash
      );
      if (!updateResult.success) {
        console.warn(
          `⚠️ Failed to update settlement tx hash on chain: ${updateResult.message}`
        );
      } else {
        console.log(
          `  Settlement tx hash saved on chain: ${updateResult.txHash}`
        );
      }

      // Increment and save nonce
      nonce++;
      await setKv(NONCE_KEY, nonce.toString());
      console.log(`  Updated nonce to: ${nonce}`);

      // Move to next block
      currentBlockNumber++;
    } catch (error: any) {
      console.error(
        `❌ Error settling block ${currentBlockNumber}:`,
        error.message
      );

      // Reset nonce on error
      console.log("🔄 Resetting nonce to 0 due to error");
      await setKv(NONCE_KEY, "0");

      // Re-throw the error to fail the job
      throw error;
    }
  }

  console.log("\n✅ Settlement process completed successfully");
}
