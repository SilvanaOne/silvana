import { describe, it } from "node:test";
import assert from "node:assert";
import { getSum } from "./helpers/sum.js";
import { getState } from "./helpers/state.js";

describe("Sum", async () => {
  it("should get sum", async () => {
    const sum = await getSum();
    assert.ok(sum !== undefined, "Sum is not defined");
    console.log("Sum:", sum);
  });
  it("should get state", async () => {
    const state = await getState();
    assert.ok(state !== undefined, "State is not defined");
    console.log("State:", state);
  });
});
