import { describe, it } from "node:test";
import assert from "node:assert";
import { action } from "./helpers/action.js";
import { AppInstanceManager } from "@silvana-one/coordination";

describe("Batch", async () => {
  it("should batch add", async () => {
    let batchIteration = 0;
    const testStartTime = Date.now();
    let totalTransactions = 0;

    const registryId = process.env.SILVANA_REGISTRY;
    const appInstanceId = process.env.APP_INSTANCE_ID;

    if (!registryId || !appInstanceId) {
      throw new Error(
        "SILVANA_REGISTRY and APP_INSTANCE_ID must be set in environment"
      );
    }

    const appInstanceManager = new AppInstanceManager({ registry: registryId });

    while (true) {
      batchIteration++;
      const maxDelay = Math.floor(Math.random() * 120 * 1000); //20 - 0.1 TPS

      // Calculate TPS
      const elapsedSeconds = (Date.now() - testStartTime) / 1000;
      const tps =
        elapsedSeconds > 0
          ? (totalTransactions / elapsedSeconds).toFixed(3)
          : 0;

      console.log(
        `[${new Date().toISOString()}] Batch iteration ${batchIteration} - Max delay: ${
          maxDelay / 1000
        }s - Total TX: ${totalTransactions} - TPS: ${tps}`
      );

      // Get app instance to check block numbers
      const appInstance = await appInstanceManager.getAppInstance(
        appInstanceId
      );
      if (
        !appInstance ||
        appInstance.blockNumber === undefined ||
        appInstance.lastSettledBlockNumber === undefined ||
        appInstance.blockNumber === null ||
        appInstance.lastSettledBlockNumber === null
      ) {
        console.error("❌ Failed to fetch app instance block numbers", {
          blockNumber: appInstance?.blockNumber,
          lastSettledBlockNumber: appInstance?.lastSettledBlockNumber,
        });
        await sleep(60000);
        continue;
      }

      const blockDiff =
        appInstance.blockNumber - appInstance.lastSettledBlockNumber;
      console.log(
        `Block status: current=${appInstance.blockNumber}, lastSettled=${appInstance.lastSettledBlockNumber}, diff=${blockDiff}`
      );

      if (blockDiff >= 5) {
        console.log(
          `⏸️ Skipping transactions: block difference (${blockDiff}) =${appInstance.blockNumber} - ${appInstance.lastSettledBlockNumber} >= 5`
        );
        await sleep(60000);
        continue;
      }

      for (let i = 0; i < 10; i++) {
        try {
          await action({
            action: "add",
            value: 1,
            index: 1,
          });
        } catch (error: any) {
          console.error("❌ Error sending action:", error?.message);
        }
        totalTransactions++;
        const sleepTime = Math.floor(Math.random() * maxDelay) + 2000;
        await sleep(sleepTime);
      }
    }
  });
});

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
