"use server";

import { generateEd25519 } from "@silvana-one/coordination";

export async function createSilvanaId(): Promise<string> {
  const coordinator = generateEd25519();
  return coordinator.address;
}
