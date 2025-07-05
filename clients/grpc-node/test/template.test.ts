import { describe, it } from "node:test";
import assert from "node:assert";

describe("Test template", async () => {
  it("should test", async () => {
    const a = true;
    const b = true;
    assert.strictEqual(a, b);
    console.log("✅ Test template completed successfully!");
  });
});
