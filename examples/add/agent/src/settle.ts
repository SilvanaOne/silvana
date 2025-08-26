import { JsonProof, verify } from "o1js";
import { AddProgramProof } from "./circuit.js";
import { compile } from "./state.js";

export async function settle(blockProofSerialized: string, blockNumber: bigint): Promise<boolean> {
  console.log("= Starting block proof settlement verification...");
  console.log(`Block number: ${blockNumber}`);
  console.log(`Block proof size: ${blockProofSerialized.length} chars`);

  // Ensure the circuit is compiled
  console.log("Compiling circuit...");
  const vk = await compile();
  if (!vk) {
    throw new Error("Failed to compile circuit for settlement");
  }
  console.log("Circuit compiled successfully");

  // Parse the proof data (it includes proof and serialized state)
  console.log("Parsing block proof data...");
  const proofData = JSON.parse(blockProofSerialized);
  
  // Extract just the proof part
  const proofJson = proofData.proof;
  
  // Deserialize the proof
  console.log("Deserializing block proof...");
  const blockProof: AddProgramProof = await AddProgramProof.fromJSON(
    JSON.parse(proofJson) as JsonProof
  );

  // Verify the proof
  console.log("Verifying block proof...");
  const verificationStartTime = Date.now();
  const isValid = await verify(blockProof, vk);
  const verificationTimeMs = Date.now() - verificationStartTime;
  
  if (!isValid) {
    console.error("L Block proof verification FAILED!");
    throw new Error(`Block proof verification failed for block ${blockNumber}`);
  }

  console.log(` Block proof verified successfully in ${verificationTimeMs}ms`);
  
  // Extract and display proof details
  console.log("=== BLOCK PROOF DETAILS ===");
  console.log(`Block Number (from proof): ${blockProof.publicOutput.blockNumber.toBigInt()}`);
  console.log(`Final Sequence: ${blockProof.publicOutput.sequence.toBigInt()}`);
  console.log(`Final Sum: ${blockProof.publicOutput.sum.toBigInt()}`);
  console.log(`Final Root: ${blockProof.publicOutput.root.toBigInt()}`);
  console.log(`Final Length: ${blockProof.publicOutput.length.toBigInt()}`);
  
  // Verify block number matches
  const proofBlockNumber = blockProof.publicOutput.blockNumber.toBigInt();
  if (proofBlockNumber !== blockNumber) {
    throw new Error(
      `Block number mismatch! Expected ${blockNumber}, got ${proofBlockNumber}`
    );
  }
  
  // Extract commitment hash for contract settlement
  const commitmentHash = blockProof.publicOutput.commitment.hash();
  console.log(`Commitment Hash: ${commitmentHash.toBigInt()}`);
  console.log("===========================");
  
  console.log("<� Block proof is valid and ready for on-chain settlement!");
  
  // TODO: In production, this would submit the proof to the smart contract
  // using the AddContract.settle() method
  console.log("=� TODO: Submit proof to AddContract for on-chain settlement");
  
  return true;
}