import { describe, it } from "node:test";
import assert from "node:assert";
import { action } from "./helpers/action.js";

describe("Batch", async () => {
  it("should batch add", async () => {
    let batchIteration = 0;
    while (true) {
      batchIteration++;
      const maxDelay = Math.floor(Math.random() * 60 * 1000) + 5000;
      console.log(`[${new Date().toISOString()}] Batch iteration ${batchIteration} - Max delay: ${maxDelay / 1000}s`);
      for (let i = 0; i < 10; i++) {
        console.log(`[${new Date().toISOString()}] Batch ${batchIteration}, action ${i + 1}/10 - Sending add action`);
        await action({
          action: "add",
          value: 1,
          index: 1,
        });
        const sleepTime = Math.floor(Math.random() * maxDelay) + 1000;
        console.log(`[${new Date().toISOString()}] Batch ${batchIteration}, action ${i + 1}/10 - Sleeping for ${sleepTime / 1000}s`);
        await sleep(sleepTime);
      }
      console.log(`[${new Date().toISOString()}] Batch ${batchIteration} complete - Sleeping for 20 minutes before next batch`);
      await sleep(20 * 60 * 1000);
    }
  });
});

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
