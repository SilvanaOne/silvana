import { describe, it } from "node:test";
import assert from "node:assert";
import { action } from "./helpers/action.js";

describe("Multiply", async () => {
  it("should multiply", async () => {
    await action({
      action: "multiply",
      value: 2,
      index: 1,
    });
  });
});
