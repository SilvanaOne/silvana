import { describe, it } from "node:test";
import assert from "node:assert";
import { action } from "./helpers/action.js";

describe("Batch", async () => {
  it("should batch add", async () => {
    while (true) {
      await action({
        action: "add",
        value: 1,
        index: 1,
      });
      const sleepTime = Math.floor(Math.random() * 120 * 1000) + 1000;
      console.log(`Sleeping for ${sleepTime / 1000}s`);
      await sleep(sleepTime);
    }
  });
});

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
