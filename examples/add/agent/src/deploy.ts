import {
  PrivateKey,
  VerificationKey,
  Mina,
  AccountUpdate,
  PublicKey,
} from "o1js";
import { AddContract } from "./contract.js";
import {
  fetchMinaAccount,
  initBlockchain,
  accountBalanceMina,
  sendTx,
  CanonicalBlockchain,
} from "@silvana-one/mina-utils";
import { compile } from "./compile.js";

const expectedTxStatus = "pending";

export async function deployAddContract(): Promise<{
  chain: CanonicalBlockchain;
  contractAddress: string;
  adminAddress: string;
  txHash: string;
  verificationKey: VerificationKey;
  nonce: number;
}> {
  console.time("AddContract deployment");
  console.log("🚀 Starting AddContract deployment...");
  // Get chain from environment
  const chain: CanonicalBlockchain = process.env.MINA_CHAIN as
    | "mina:devnet"
    | "zeko:testnet"
    | "mina:mainnet";
  if (
    !chain ||
    !["mina:devnet", "zeko:testnet", "mina:mainnet"].includes(chain)
  ) {
    console.error(`Invalid or missing MINA_CHAIN: ${chain}`);
  }

  // Get keys from environment
  const MINA_PRIVATE_KEY = process.env.MINA_PRIVATE_KEY;
  const MINA_PUBLIC_KEY = process.env.MINA_PUBLIC_KEY;
  const MINA_APP_ADMIN = process.env.MINA_APP_ADMIN;

  if (!MINA_PRIVATE_KEY || !MINA_PUBLIC_KEY || !MINA_APP_ADMIN) {
    throw new Error(
      "Missing required environment variables: MINA_PRIVATE_KEY, MINA_PUBLIC_KEY, or MINA_APP_ADMIN"
    );
  }
  console.log(`Chain: ${chain}`);

  // Initialize blockchain connection
  await initBlockchain(chain);
  const { vkProgram, vkContract } = await compile({ compileContract: true });
  if (!vkProgram || !vkContract) {
    throw new Error("Failed to compile circuit for merging");
  }

  // Parse keys
  const deployerPrivateKey = PrivateKey.fromBase58(MINA_PRIVATE_KEY!);
  const deployerPublicKey = deployerPrivateKey.toPublicKey();
  const adminPublicKey = PublicKey.fromBase58(MINA_APP_ADMIN!);

  // Generate a new keypair for the contract
  const contractPrivateKey = PrivateKey.random();
  const contractPublicKey = contractPrivateKey.toPublicKey();

  console.log("\n📋 Deployment Details:");
  console.log(`  Deployer: ${deployerPublicKey.toBase58()}`);
  console.log(`  Admin: ${adminPublicKey.toBase58()}`);
  console.log(`  Contract Address: ${contractPublicKey.toBase58()}`);

  // Check deployer balance
  const balance = await accountBalanceMina(deployerPublicKey);
  console.log(`  Deployer Balance: ${balance} MINA`);

  if (balance < 2) {
    throw new Error(
      `Insufficient balance. Need at least 2 MINA, have ${balance} MINA`
    );
  }

  // Fetch account to get nonce
  await fetchMinaAccount({ publicKey: deployerPublicKey, force: true });

  // Get the account to retrieve the nonce before transaction
  const account = Mina.getAccount(deployerPublicKey);
  const currentNonce = Number(account.nonce.toBigint());

  // Create the contract instance
  const contract = new AddContract(contractPublicKey);

  // Create deployment transaction
  console.log("\n📝 Creating deployment transaction...");
  const tx = await Mina.transaction(
    {
      sender: deployerPublicKey,
      fee: 200_000_000,
      memo: "Deploy Silvana Contract",
    },
    async () => {
      // Fund the new account
      if (chain === "zeko:testnet") {
        // workaround to make zeko testnet work with o1js 2.4
        const au = AccountUpdate.createSigned(deployerPublicKey);
        au.balance.subInPlace(100_000_000); // 0.1 MINA Account Creation Fee on Zeko Testnet
      } else {
        AccountUpdate.fundNewAccount(deployerPublicKey);
      }

      // Deploy the contract
      await contract.deploy({
        admin: adminPublicKey,
        uri: "Silvana AddContract",
      });
    }
  );

  // Prove the transaction
  console.log("🔐 Proving transaction...");
  console.time("proved tx");
  await tx.prove();
  console.timeEnd("proved tx");

  // Sign and send the transaction
  console.log("📤 Sending transaction...");
  console.time("sent tx");
  const sentTx = await sendTx({
    tx: tx.sign([deployerPrivateKey, contractPrivateKey]),
    description: "Deploy Silvana Contract",
    wait: false,
    verbose: true,
  });
  console.timeEnd("sent tx");
  if (sentTx?.status !== expectedTxStatus) {
    console.error("Transaction failed:", sentTx);
    throw new Error(`Deploy AddContract failed: ${sentTx?.status}`);
  }

  const txHash = sentTx.hash;
  console.log(`✅ Transaction sent successfully!`);
  console.log(`  Transaction Hash: ${txHash}`);
  console.log(`  Contract Address: ${contractPublicKey.toBase58()}`);
  console.log(`  Admin Address: ${adminPublicKey.toBase58()}`);

  console.timeEnd("AddContract deployment");

  // The nonce after this transaction will be currentNonce + 1
  const nonce = currentNonce + 1;

  return {
    chain,
    contractAddress: contractPublicKey.toBase58(),
    adminAddress: adminPublicKey.toBase58(),
    txHash,
    verificationKey: vkContract,
    nonce,
  };
}

export async function checkAddContractDeployment(params: {
  contractAddress: string;
  adminAddress: string;
}): Promise<boolean> {
  const { contractAddress, adminAddress } = params;
  console.log("🔍 Checking AddContract deployment...");
  console.log(`  Contract Address: ${contractAddress}`);
  console.log(`  Admin Address: ${adminAddress}`);

  const contractPublicKey = PublicKey.fromBase58(contractAddress);
  const contract = new AddContract(contractPublicKey);

  // Try to fetch the contract account
  await fetchMinaAccount({ publicKey: contractPublicKey, force: false });

  if (!Mina.hasAccount(contractPublicKey)) {
    console.log("❌ Contract account not found on chain");
    return false;
  }

  // Check if admin matches expected
  const adminPublicKey = PublicKey.fromBase58(adminAddress);
  const onChainAdmin = contract.admin.get();

  if (onChainAdmin.toBase58() !== adminPublicKey.toBase58()) {
    console.error("❌ Admin address mismatch");
    console.error(`  Expected: ${adminPublicKey.toBase58()}`);
    console.error(`  On-chain: ${onChainAdmin.toBase58()}`);
    return false;
  }

  console.log("✅ Contract deployed successfully!");
  console.log(`  Admin: ${onChainAdmin.toBase58()}`);
  console.log(`  Block Number: ${contract.blockNumber.get().toString()}`);
  console.log(`  Sequence: ${contract.sequence.get().toString()}`);
  console.log(`  Sum: ${contract.sum.get().toString()}`);

  return true;
}
