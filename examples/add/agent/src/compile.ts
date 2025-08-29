import { Cache, VerificationKey } from "o1js";
import { AddContract } from "./contract.js";
import { AddProgram } from "./circuit.js";

let vkProgram: VerificationKey | undefined = undefined;
let vkContract: VerificationKey | undefined = undefined;

export async function compile(params?: { compileContract: boolean }): Promise<{
  vkProgram: VerificationKey;
  vkContract: VerificationKey | undefined;
}> {
  const cache = Cache.FileSystem("./cache");

  // Ensure the circuit is compiled
  if (vkProgram === undefined) {
    console.log("📦 Compiling circuit...");

    console.time("compiled AddProgram");
    vkProgram = (await AddProgram.compile({ cache })).verificationKey;
    console.timeEnd("compiled AddProgram");
    console.log("vk AddProgram", vkProgram.hash.toJSON());
  }

  // Compile the contract
  if (params?.compileContract === true && vkContract === undefined) {
    console.log("📦 Compiling contract...");
    console.time("compiled AddContract");
    vkContract = (await AddContract.compile({ cache })).verificationKey;
    console.timeEnd("compiled AddContract");
    console.log("vk AddContract", vkContract.hash.toJSON());
  }

  return { vkProgram, vkContract };
}
