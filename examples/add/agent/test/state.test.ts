import { describe, it } from "node:test";
import assert from "node:assert";
import { getState } from "@silvana-one/coordination";

describe("State", async () => {
  it("should get state", async () => {
    const state = await getState();
    assert.ok(state !== undefined, "State is not defined");
    console.log("State:", state);
  });
});
