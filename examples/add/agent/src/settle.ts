import { JsonProof, verify, PrivateKey, PublicKey, Mina, Cache } from "o1js";
import { AddProgramProof } from "./circuit.js";
import { compile } from "./compile.js";
import { AddContract } from "./contract.js";
import { checkAddContractDeployment } from "./deploy.js";
import {
  initBlockchain,
  fetchMinaAccount,
  accountBalanceMina,
  sendTx,
  CanonicalBlockchain,
} from "@silvana-one/mina-utils";
import {
  getBlock,
  getBlockSettlement,
  getBlockProof,
  setKv,
  getKv,
  getMetadata,
  getSecret,
  updateBlockStateDataAvailability,
  updateBlockProofDataAvailability,
  updateBlockSettlementTxHash,
  updateBlockSettlementTxIncludedInBlock,
  rejectProof,
  proofEvent,
  ProofEventType,
  info,
} from "@silvana-one/agent";

interface SettleParams {
  privateKey?: string; // Optional - will use secret if not provided
  contractAddress?: string; // Optional - will use settlement address from metadata if not provided
  settlementChain: string; // Required - the chain to settle for
}

export async function settle(params: SettleParams): Promise<void> {
  console.log("🚀 Starting settlement process...");

  // Time tracking for graceful shutdown
  const startTime = Date.now();
  const maxRunTimeMs = 600 * 1000; // 10 minutes max runtime
  const stopAcceptingJobsMs = 400 * 1000; // Stop accepting new blocks after 400 seconds

  // Fetch settlement_admin metadata to get admin address and contract info
  console.log("🔍 Fetching settlement admin metadata...");
  const metadataResponse = await getMetadata("settlementAdmin");

  if (!metadataResponse.success || !metadataResponse.metadata) {
    throw new Error(
      `Failed to fetch settlement admin metadata: ${metadataResponse.message}`
    );
  }

  // Get the admin address from metadata value (if present) or use the AppInstance admin field
  let settlementAdminAddress = metadataResponse.metadata.value;

  // Use the required settlement chain
  if (
    params.settlementChain !== "mina:devnet" &&
    params.settlementChain !== "zeko:testnet" &&
    params.settlementChain !== "mina:mainnet"
  ) {
    throw new Error(
      `Unsupported settlement chain: ${params.settlementChain}, supported chains are: mina:devnet, zeko:testnet, mina:mainnet`
    );
  }
  const settlementChain: CanonicalBlockchain = params.settlementChain;
  console.log(`🔗 Settling for chain: ${settlementChain}`);

  // Verify the chain exists in settlements
  if (!metadataResponse.metadata.settlements?.[settlementChain]) {
    throw new Error(
      `Settlement chain '${settlementChain}' not found in app instance settlements`
    );
  }

  // Get the contract address for this chain
  const contractAddress =
    metadataResponse.metadata.settlements[settlementChain].settlementAddress ||
    params.contractAddress;

  // Print AppInstance info
  console.log("📊 AppInstance Information:");
  console.log(`  - Instance ID: ${metadataResponse.metadata.appInstanceId}`);
  console.log(`  - App Name: ${metadataResponse.metadata.silvanaAppName}`);
  console.log(`  - Admin Address: ${metadataResponse.metadata.admin}`);
  console.log(`  - Settlement Admin: ${settlementAdminAddress ?? "none"}`);
  console.log(`  - Settlement Chain: ${settlementChain}`);
  console.log(`  - Contract Address: ${contractAddress ?? "none"}`);
  console.log(
    `  - Available Chains: ${
      metadataResponse.metadata.settlements
        ? Object.keys(metadataResponse.metadata.settlements).join(", ")
        : "none"
    }`
  );
  console.log(`  - Current Sequence: ${metadataResponse.metadata.sequence}`);
  console.log(`  - Current Block: ${metadataResponse.metadata.blockNumber}`);
  console.log(
    `  - Last Proved Block: ${metadataResponse.metadata.lastProvedBlockNumber}`
  );

  // Get last settled block for this chain
  const lastSettledBlockForChain =
    metadataResponse.metadata.settlements?.[settlementChain]
      ?.lastSettledBlockNumber || 0n;
  console.log(
    `  - Last Settled Block (${settlementChain}): ${lastSettledBlockForChain}`
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

  // Initialize blockchain
  await initBlockchain(settlementChain);

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

  // Get the last settled block number from contract
  const lastSettledBlock = contract.blockNumber.get().toBigInt();
  console.log(`📊 Last settled block: ${lastSettledBlock}`);

  console.log("🔄 Started recording tx inclusion for blocks...");

  for (let i = lastSettledBlockForChain; i <= lastSettledBlock; i++) {
    if (i === 0n) {
      console.log("Skipping block 0");
      continue;
    }

    // Get block settlement for this specific chain
    const blockSettlement = await getBlockSettlement(i, settlementChain);
    if (
      !blockSettlement ||
      !blockSettlement.success ||
      !blockSettlement.blockSettlement
    ) {
      console.error(
        `No block settlement found for block ${i} on chain ${settlementChain}`
      );
    }

    if (
      !blockSettlement ||
      !blockSettlement.success ||
      !blockSettlement.blockSettlement ||
      blockSettlement.blockSettlement?.settlementTxIncludedInBlock === false
    ) {
      info(`Recording tx inclusion for block ${i} on chain ${settlementChain}`);
      const updateResult = await updateBlockSettlementTxIncludedInBlock(
        i,
        BigInt(Date.now()),
        settlementChain
      );
      if (!updateResult.success) {
        console.error(
          `Failed to update tx inclusion for block ${i} on ${settlementChain}: ${updateResult.message}`
        );
        throw new Error(
          `Failed to update tx inclusion for block ${i} on ${settlementChain}: ${updateResult.message}`
        );
      }
      console.log(
        `✅ Tx inclusion recorded for block ${i} on ${settlementChain}`
      );
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

  // Check sender balance
  await fetchMinaAccount({ publicKey: senderPublicKey, force: true });
  const balance = await accountBalanceMina(senderPublicKey);
  console.log(`💰 Sender balance: ${balance} MINA`);
  if (balance < 0.2) {
    throw new Error(
      `Insufficient balance. Need at least 0.2 MINA, have ${balance} MINA`
    );
  }

  // Start iterating from the next block
  let currentBlockNumber = lastSettledBlock + 1n;
  let nonce: number | null = null;
  const NONCE_KEY = `nonce_${senderPublicKey.toBase58()}`;

  console.log(`🔄 Starting settlement from block ${currentBlockNumber}`);

  while (true) {
    // Check if we should continue accepting new blocks
    const elapsedMs = Date.now() - startTime;
    const remainingMs = maxRunTimeMs - elapsedMs;
    const remainingForNewJobsMs = stopAcceptingJobsMs - elapsedMs;

    console.log(
      `Elapsed: ${Math.round(
        elapsedMs / 1000
      )}s, Max runtime remaining: ${Math.round(
        remainingMs / 1000
      )}s, New blocks cutoff in: ${Math.round(
        Math.max(0, remainingForNewJobsMs / 1000)
      )}s`
    );

    // Stop accepting new blocks after 400 seconds
    if (elapsedMs >= stopAcceptingJobsMs) {
      console.log("Reached 400 second cutoff - not accepting new blocks");
      break;
    }

    console.log(`\n📦 Processing block ${currentBlockNumber}...`);

    // Get both block and block settlement data
    const block = await getBlock(currentBlockNumber);
    if (!block || !block.success || !block.block) {
      console.log(`No block found for block ${currentBlockNumber}`);
      console.log("✅ Settlement complete - reached last block");
      break;
    }

    const blockSettlement = await getBlockSettlement(
      currentBlockNumber,
      settlementChain
    );
    const settlementData = blockSettlement?.blockSettlement;

    // Format block data for logging (exclude commitments, convert times to UTC)
    const formatTime = (timestamp: bigint | undefined) => {
      if (!timestamp) return "N/A";
      return new Date(Number(timestamp)).toUTCString();
    };

    console.log("📦 Block data:", {
      name: block.block.name,
      blockNumber: block.block.blockNumber?.toString(),
      startSequence: block.block.startSequence?.toString(),
      endSequence: block.block.endSequence?.toString(),
      timeSinceLastBlock: block.block.timeSinceLastBlock
        ? `${block.block.timeSinceLastBlock}ms`
        : "N/A",
      numberOfTransactions: block.block.numberOfTransactions?.toString(),
      stateDataAvailability: block.block.stateDataAvailability || "none",
      proofDataAvailability: block.block.proofDataAvailability || "none",
      createdAt: formatTime(block.block.createdAt),
      stateCalculatedAt: formatTime(block.block.stateCalculatedAt),
      provedAt: formatTime(block.block.provedAt),
    });

    console.log(`📦 Settlement data for ${settlementChain}:`, {
      settlementTxHash: settlementData?.settlementTxHash || "none",
      settlementTxIncludedInBlock:
        settlementData?.settlementTxIncludedInBlock || false,
      sentToSettlementAt: formatTime(settlementData?.sentToSettlementAt),
      settledAt: formatTime(settlementData?.settledAt),
    });

    let shouldSettle = true;
    if (settlementData?.settlementTxHash) {
      console.log(
        `Block ${currentBlockNumber} has already been settled on ${settlementChain}, txHash: ${settlementData.settlementTxHash}`
      );
      if (settlementData.sentToSettlementAt) {
        const now = BigInt(Date.now());
        console.log("now", now);
        console.log("sentToSettlementAt", settlementData.sentToSettlementAt);
        if (settlementData.sentToSettlementAt + 60n * 60n * 1000n > now) {
          console.log(
            `Block ${currentBlockNumber} was settled less than 1 hour ago on ${settlementChain}, skipping...`
          );
          shouldSettle = false;
        } else {
          console.log(
            `Block ${currentBlockNumber} was settled more than 1 hour ago on ${settlementChain}, resettling...`
          );
        }
      } else {
        console.log(
          `Block ${currentBlockNumber} was settled on ${settlementChain} but no sentToSettlementAt timestamp, skipping...`
        );
        shouldSettle = false;
      }
    }

    if (!shouldSettle) {
      console.log("Skipping block", currentBlockNumber);
      currentBlockNumber++;
      continue;
    }

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

    // Deserialize the proof with rejection on failure
    console.log("🔐 Deserializing and verifying block proof...");
    let blockProof: AddProgramProof;

    try {
      blockProof = await AddProgramProof.fromJSON(
        JSON.parse(proofJson) as JsonProof
      );
    } catch (error) {
      console.error(
        `Error deserializing block proof for block ${currentBlockNumber}:`,
        error
      );

      // Get sequences from the block to reject
      const sequences: bigint[] = [];
      if (block.block?.startSequence && block.block?.endSequence) {
        for (
          let seq = block.block.startSequence;
          seq <= block.block.endSequence;
          seq++
        ) {
          sequences.push(seq);
        }
      }

      const rejectProofResponse = await rejectProof(
        currentBlockNumber,
        sequences
      );
      if (!rejectProofResponse.success) {
        throw new Error(
          `Failed to reject proof for block ${currentBlockNumber}: ${rejectProofResponse.message}`
        );
      }
      throw error;
    }

    // Verify the proof with rejection on failure
    const { vkProgram } = await compile();

    try {
      const isValid = await verify(blockProof, vkProgram);
      if (!isValid) {
        await proofEvent({
          proofEventType: ProofEventType.PROOF_REJECTED,
          sequences: [],
          blockProof: true,
          blockNumber: currentBlockNumber,
          dataAvailability: "",
        });
        throw new Error(
          `Block proof verification failed for block ${currentBlockNumber}`
        );
      }
      await proofEvent({
        proofEventType: ProofEventType.PROOF_VERIFIED,
        sequences: [],
        blockProof: true,
        blockNumber: currentBlockNumber,
        dataAvailability: "",
      });
    } catch (error) {
      console.error(
        `Error verifying block proof for block ${currentBlockNumber}:`,
        error
      );

      // Get sequences from the block to reject
      const sequences: bigint[] = [];
      if (block.block?.startSequence && block.block?.endSequence) {
        for (
          let seq = block.block.startSequence;
          seq <= block.block.endSequence;
          seq++
        ) {
          sequences.push(seq);
        }
      }

      const rejectProofResponse = await rejectProof(
        currentBlockNumber,
        sequences
      );
      if (!rejectProofResponse.success) {
        throw new Error(
          `Failed to reject proof for block ${currentBlockNumber}: ${rejectProofResponse.message}`
        );
      }
      throw error;
    }

    console.log("✅ Block proof verified successfully");

    // Update block state data availability on Sui (using the same serialized data that contains both proof and state)
    console.log("📝 Updating block state data availability on Sui...");
    const updateStateDAResponse = await updateBlockStateDataAvailability(
      currentBlockNumber,
      blockProofSerialized // The serialized proof contains both proof and state data
    );

    if (!updateStateDAResponse.success) {
      console.error(
        `❌ Failed to update block state DA on Sui: ${
          updateStateDAResponse.message || "Unknown error"
        }`
      );
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

    // Update block proof data availability on Sui
    console.log("📝 Updating block proof data availability on Sui...");
    const updateDAResponse = await updateBlockProofDataAvailability(
      currentBlockNumber,
      blockProofSerialized
    );

    if (!updateDAResponse.success) {
      console.error(
        `❌ Failed to update block proof DA on Sui: ${
          updateDAResponse.message || "Unknown error"
        }`
      );
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

    await compile({ compileContract: true });

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

    const numberOfTransactions = block.block.numberOfTransactions;

    // Create and send settlement transaction
    console.log("📝 Creating settlement transaction...");
    const memo = `Silvana block ${currentBlockNumber} ${
      numberOfTransactions
        ? "(" + numberOfTransactions.toString() + " txs)"
        : ""
    }`;
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

      // Save transaction hash to blockchain for this specific chain
      const updateResult = await updateBlockSettlementTxHash(
        currentBlockNumber,
        txHash,
        settlementChain
      );
      if (!updateResult.success) {
        console.warn(
          `⚠️ Failed to update settlement tx hash on chain ${settlementChain}: ${updateResult.message}`
        );
      } else {
        console.log(
          `  Settlement tx hash saved on chain ${settlementChain}: ${updateResult.txHash}`
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
