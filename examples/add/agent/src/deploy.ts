import {
  PrivateKey,
  VerificationKey,
  Mina,
  AccountUpdate,
  PublicKey,
} from "o1js";
import { AddContract } from "./contract.js";
import { AddProgram } from "./circuit.js";
import {
  fetchMinaAccount,
  initBlockchain,
  accountBalanceMina,
  sendTx,
} from "@silvana-one/mina-utils";

// Get chain from environment
const chain = process.env.MINA_CHAIN as "devnet" | "zeko" | "mainnet";
if (!chain || !["devnet", "zeko", "mainnet"].includes(chain)) {
  throw new Error(`Invalid or missing MINA_CHAIN: ${chain}`);
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

const expectedTxStatus = "pending";

export async function deployAddContract(): Promise<{
  contractAddress: string;
  txHash: string;
  verificationKey: VerificationKey;
  nonce: number;
}> {
  console.time("AddContract deployment");
  console.log("🚀 Starting AddContract deployment...");
  console.log(`Chain: ${chain}`);

  // Initialize blockchain connection
  await initBlockchain(chain);

  // Compile the AddProgram circuit to get verification key
  console.log("📦 Compiling AddProgram circuit...");
  console.time("Circuit compilation");
  await AddProgram.compile();
  console.timeEnd("Circuit compilation");

  // Compile the AddContract
  console.log("📦 Compiling AddContract...");
  console.time("Contract compilation");
  const { verificationKey } = await AddContract.compile();
  console.timeEnd("Contract compilation");

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
      fee: 100_000_000,
      memo: "Deploy AddContract",
    },
    async () => {
      // Fund the new account
      AccountUpdate.fundNewAccount(deployerPublicKey);

      // Deploy the contract
      await contract.deploy({
        admin: adminPublicKey,
        uri: "AddContract for Silvana",
        verificationKey,
      });
    }
  );

  // Prove the transaction
  console.log("🔐 Proving transaction...");
  console.time("Transaction proof");
  await tx.prove();
  console.timeEnd("Transaction proof");

  // Sign and send the transaction
  console.log("📤 Sending transaction...");
  const sentTx = await sendTx({
    tx: tx.sign([deployerPrivateKey, contractPrivateKey]),
    description: "Deploy AddContract",
    wait: false,
    verbose: true,
  });

  if (sentTx?.status !== expectedTxStatus) {
    console.error("Transaction failed:", sentTx);
    throw new Error(`Deploy AddContract failed: ${sentTx?.status}`);
  }

  const txHash = sentTx.hash;
  console.log(`✅ Transaction sent successfully!`);
  console.log(`  Transaction Hash: ${txHash}`);
  console.log(`  Contract Address: ${contractPublicKey.toBase58()}`);

  console.timeEnd("AddContract deployment");

  // The nonce after this transaction will be currentNonce + 1
  const nonce = currentNonce + 1;

  return {
    contractAddress: contractPublicKey.toBase58(),
    txHash,
    verificationKey,
    nonce,
  };
}

export async function checkAddContractDeployment(
  contractAddress: string
): Promise<boolean> {
  console.log("🔍 Checking AddContract deployment...");
  console.log(`  Chain: ${chain}`);
  console.log(`  Contract Address: ${contractAddress}`);

  await initBlockchain(chain);

  const contractPublicKey = PublicKey.fromBase58(contractAddress);
  const contract = new AddContract(contractPublicKey);

  // Try to fetch the contract account
  await fetchMinaAccount({ publicKey: contractPublicKey, force: false });

  if (!Mina.hasAccount(contractPublicKey)) {
    console.log("❌ Contract account not found on chain");
    return false;
  }

  // Check if admin matches expected
  const adminPublicKey = PublicKey.fromBase58(MINA_APP_ADMIN!);
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

// Main execution if run directly
if (import.meta.url === `file://${process.argv[1]}`) {
  console.log("=====================================");
  console.log("    AddContract Deployment Script    ");
  console.log("=====================================\n");

  deployAddContract()
    .then(async (result) => {
      console.log("\n🎉 Deployment completed successfully!");
      console.log("\nSave these values:");
      console.log(`CONTRACT_ADDRESS=${result.contractAddress}`);
      console.log(`DEPLOYMENT_TX_HASH=${result.txHash}`);
      console.log(
        `VERIFICATION_KEY_HASH=${result.verificationKey.hash.toString()}`
      );
      console.log(`NEXT_NONCE=${result.nonce}`);

      // Wait a bit for the transaction to be included
      console.log("\n⏳ Waiting 30 seconds for transaction to be included...");
      await new Promise((resolve) => setTimeout(resolve, 30000));

      // Check deployment
      const isDeployed = await checkAddContractDeployment(
        result.contractAddress
      );
      if (isDeployed) {
        console.log("\n✅ Contract verified on-chain!");
      } else {
        console.log(
          "\n⚠️ Contract not yet visible on-chain. It may take a few more minutes."
        );
      }
    })
    .catch((error) => {
      console.error("\n❌ Deployment failed:");
      console.error(error);
      process.exit(1);
    });
}
