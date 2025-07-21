import { describe, it } from "node:test";
import assert from "node:assert";
import { action } from "./helpers/action.js";

describe("Add", async () => {
  it("should add", async () => {
    await action({
      action: "add",
      value: 1,
      index: 1,
    });
  });
});
