import { describe, it } from "node:test";
import assert from "node:assert";
import { purge } from "./helpers/purge.js";

describe("Purge", async () => {
  it("should purge", async () => {
    await purge({
      proved_sequence: 10,
    });
  });
});
