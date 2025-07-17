import { CoinBalance } from "@mysten/sui/client";
import { suiClient } from "@silvana-one/coordination";
import { MIST_PER_SUI } from "@mysten/sui/utils";
import { Ed25519Keypair } from "@mysten/sui/keypairs/ed25519";

export function suiBalance(balance: CoinBalance): number {
  return Number.parseInt(balance.totalBalance) / Number(MIST_PER_SUI);
}

export async function getSuiBalance(address: string): Promise<number> {
  try {
    const balance = await suiClient.getBalance({
      owner: address,
      coinType: "0x2::sui::SUI",
    });
    return suiBalance(balance);
  } catch (error: any) {
    console.error("getSuiBalance error:", error?.message);
    return 0;
  }
}

export async function getSuiAddress(params: {
  secretKey: string;
}): Promise<string> {
  return Ed25519Keypair.fromSecretKey(params.secretKey)
    .getPublicKey()
    .toSuiAddress();
}
