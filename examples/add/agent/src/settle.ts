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

interface SettleParams {
  privateKey: string;
  contractAddress: string;
  getBlockProof: (blockNumber: bigint) => Promise<string | null>;
  saveKey: (key: string, value: string) => Promise<void>;
  readKey: (key: string) => Promise<string | null>;
  saveSettlementTxHash: (txHash: string, blockNumber: bigint) => Promise<void>;
}

export async function settle(params: SettleParams): Promise<void> {
  const {
    contractAddress,
    getBlockProof,
    saveKey,
    readKey,
    saveSettlementTxHash,
  } = params;

  console.log("🚀 Starting settlement process...");
  console.log(`Contract Address: ${contractAddress}`);
  const senderPrivateKey = PrivateKey.fromBase58(params.privateKey);
  const senderPublicKey = senderPrivateKey.toPublicKey();
  const adminAddress = senderPublicKey;
  console.log(`Admin Address: ${adminAddress}`);

  // Initialize blockchain for devnet
  await initBlockchain("devnet");

  // Check that the contract is deployed
  console.log("🔍 Checking contract deployment...");
  const isDeployed = await checkAddContractDeployment(contractAddress);
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

  // Compile the contract as well
  console.log("📦 Compiling contract...");
  await AddContract.compile({ cache });

  // Start iterating from the next block
  let currentBlockNumber = lastSettledBlock + 1n;
  let nonce: number | null = null;
  const NONCE_KEY = `nonce_${senderPublicKey.toBase58()}`;

  console.log(`🔄 Starting settlement from block ${currentBlockNumber}`);

  while (true) {
    console.log(`\n📦 Processing block ${currentBlockNumber}...`);

    // Fetch block proof
    const blockProofSerialized = await getBlockProof(currentBlockNumber);

    if (!blockProofSerialized) {
      console.log(`❌ No proof available for block ${currentBlockNumber}`);
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

    // Extract proof details
    console.log("📊 Block proof details:");
    console.log(
      `  - Block Number: ${blockProof.publicOutput.blockNumber.toBigInt()}`
    );
    console.log(
      `  - Final Sequence: ${blockProof.publicOutput.sequence.toBigInt()}`
    );
    console.log(`  - Final Sum: ${blockProof.publicOutput.sum.toBigInt()}`);

    // Initialize nonce on first proof (after verification)
    if (nonce === null) {
      console.log("🔢 Initializing nonce...");

      // Fetch fresh account state
      await fetchMinaAccount({ publicKey: senderPublicKey, force: true });
      const onChainNonce = Number(
        Mina.getAccount(senderPublicKey).nonce.toBigint()
      );

      // Read saved nonce
      const savedNonceStr = await readKey(NONCE_KEY);
      const savedNonce = savedNonceStr ? parseInt(savedNonceStr, 10) : 0;

      // Use the highest nonce
      nonce = Math.max(onChainNonce, savedNonce);
      console.log(`  On-chain nonce: ${onChainNonce}`);
      console.log(`  Saved nonce: ${savedNonce}`);
      console.log(`  Using nonce: ${nonce}`);
    }

    // Create and send settlement transaction
    console.log("📝 Creating settlement transaction...");
    const memo = `Settle block ${currentBlockNumber}`;

    try {
      const tx = await Mina.transaction(
        {
          sender: senderPublicKey,
          fee: 200_000_000, // 0.2 MINA
          memo: memo.substring(0, 30),
          nonce: nonce,
        },
        async () => {
          await contract.settle(blockProof);
        }
      );

      // Prove the transaction
      console.log("🔐 Proving transaction...");
      await tx.prove();

      // Sign and send the transaction
      console.log("📤 Sending transaction...");
      const sentTx = await sendTx({
        tx: tx.sign([senderPrivateKey]),
        description: `settle block ${currentBlockNumber}`,
        wait: false,
        verbose: true,
      });

      if (
        !sentTx ||
        !sentTx.status ||
        sentTx.status !== "pending" ||
        !sentTx.hash
      ) {
        console.error("❌ Transaction failed:", sentTx);

        // Reset nonce on failure
        console.log("🔄 Resetting nonce to 0 due to failure");
        await saveKey(NONCE_KEY, "0");

        throw new Error(
          `Settlement transaction failed for block ${currentBlockNumber}: ${sentTx?.status}`
        );
      }

      // Transaction successful
      const txHash = sentTx.hash;
      console.log(`✅ Transaction sent successfully!`);
      console.log(`  Transaction Hash: ${txHash}`);

      // Save transaction hash
      await saveSettlementTxHash(txHash, currentBlockNumber);

      // Increment and save nonce
      nonce++;
      await saveKey(NONCE_KEY, nonce.toString());
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
      await saveKey(NONCE_KEY, "0");

      // Re-throw the error to fail the job
      throw error;
    }
  }

  console.log("\n✅ Settlement process completed successfully");
}
