import { describe, it } from "node:test";
import assert from "node:assert";
import { action } from "./helpers/action.js";

describe("Batch", async () => {
  it("should batch add", async () => {
    while (true) {
      const maxDelay = Math.floor(Math.random() * 60 * 1000) + 5000;
      console.log(`Max delay for batch: ${maxDelay / 1000}s`);
      for (let i = 0; i < 10; i++) {
        await action({
          action: "add",
          value: 1,
          index: 1,
        });
        const sleepTime = Math.floor(Math.random() * maxDelay) + 1000;
        console.log(`Sleeping for ${sleepTime / 1000}s`);
        await sleep(sleepTime);
      }
      await sleep(20 * 60 * 1000);
    }
  });
});

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
