import { describe, it } from "node:test";
import assert from "node:assert";

import { writeFile } from "node:fs/promises";
import { createApp } from "./helpers/create.js";

let appID: string | undefined = undefined;

describe("Create App", async () => {
  it("should create app", async () => {
    appID = await createApp();
    assert.ok(appID !== undefined, "appID is not set");
  });
  it("should save circuit address to .env.circuit", async () => {
    if (!appID) {
      throw new Error("appId is not set");
    }
    const envContent = `# App ID
APP_OBJECT_ID=${appID}
`;
    await writeFile(".env.app", envContent);
  });
});
