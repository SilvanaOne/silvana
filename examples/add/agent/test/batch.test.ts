import { describe, it } from "node:test";
import assert from "node:assert";
import { action } from "./helpers/action.js";

describe("Batch", async () => {
  it("should batch add", async () => {
    let batchIteration = 0;
    const testStartTime = Date.now();
    let totalTransactions = 0;

    while (true) {
      batchIteration++;
      const maxDelay = Math.floor(Math.random() * 60 * 1000); //20 - 0.1 TPS

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
      for (let i = 0; i < 10; i++) {
        // console.log(
        //   `[${new Date().toISOString()}] Batch ${batchIteration}, action ${
        //     i + 1
        //   }/10 - Sending add action`
        // );
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
        // console.log(
        //   `[${new Date().toISOString()}] Batch ${batchIteration}, action ${
        //     i + 1
        //   }/10 - Sleeping for ${sleepTime / 1000}s`
        // );
        await sleep(sleepTime);
      }
      // console.log(
      //   `[${new Date().toISOString()}] Batch ${batchIteration} complete - Sleeping for 1 minute before next batch`
      // );
      // await sleep(1 * 60 * 1000);
    }
  });
});

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
