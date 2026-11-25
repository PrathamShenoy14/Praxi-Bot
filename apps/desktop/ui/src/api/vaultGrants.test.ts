import assert from "node:assert/strict";
import { test } from "node:test";
import { ApiError } from "./client.ts";
import { MockApiClient } from "./mock.ts";

async function askOf(mock: MockApiClient, name: string): Promise<boolean | undefined> {
  return (await mock.listFillSecrets()).find((s) => s.name === name)?.ask_before_use;
}

test("seed grants match entries with Ask before use off", async () => {
  const mock = new MockApiClient();
  const items = await mock.listFillSecrets();
  const grants = await mock.listFillGrants();
  assert.ok(grants.length > 0);
  for (const it of items) {
    const mine = grants.filter((g) => g.secret === it.name);
    if (it.ask_before_use) {
      assert.equal(mine.length, 0, it.name);
    } else {
      assert.deepEqual(mine.map((g) => g.origin).sort(), [...it.origins].sort(), it.name);
    }
  }
});

test("revoking the last grant turns Ask before use back on", async () => {
  const mock = new MockApiClient();
  const [grant] = await mock.listFillGrants();
  assert.equal(await askOf(mock, grant.secret), false);
  await mock.revokeFillGrant(grant.id);
  assert.equal((await mock.listFillGrants()).some((g) => g.id === grant.id), false);
  assert.equal(await askOf(mock, grant.secret), true);
});

test("revoking a non-last grant keeps Ask before use off", async () => {
  const mock = new MockApiClient();
  const [seed] = await mock.listFillGrants();
  await mock.updateFillSecret(seed.secret, {
    origins: [seed.origin, "https://second.example"],
  });
  const grants = (await mock.listFillGrants()).filter((g) => g.secret === seed.secret);
  assert.equal(grants.length, 2);
  await mock.revokeFillGrant(grants[0].id);
  assert.equal(await askOf(mock, seed.secret), false);
  assert.equal((await mock.listFillGrants()).filter((g) => g.secret === seed.secret).length, 1);
});

test("revoking an unknown grant is a 404", async () => {
  const mock = new MockApiClient();
  await assert.rejects(
    () => mock.revokeFillGrant("nope"),
    (err: unknown) => err instanceof ApiError && err.status === 404 && err.code === "not_found",
  );
});

test("toggling Ask before use creates one grant per origin and removes them", async () => {
  const mock = new MockApiClient();
  const name = "login/github";
  await mock.updateFillSecret(name, { origins: ["https://github.com", "https://gist.github.com"] });
  assert.equal((await mock.listFillGrants()).filter((g) => g.secret === name).length, 0);
  await mock.updateFillSecret(name, { ask_before_use: false });
  const grants = (await mock.listFillGrants()).filter((g) => g.secret === name);
  assert.deepEqual(grants.map((g) => g.origin).sort(), ["https://gist.github.com", "https://github.com"]);
  await mock.updateFillSecret(name, { ask_before_use: true });
  assert.equal((await mock.listFillGrants()).filter((g) => g.secret === name).length, 0);
});

test("changing origins prunes and adds grants while Ask before use is off", async () => {
  const mock = new MockApiClient();
  const name = "login/linear";
  await mock.updateFillSecret(name, { origins: ["https://linear.app", "https://b.example"] });
  let origins = (await mock.listFillGrants()).filter((g) => g.secret === name).map((g) => g.origin);
  assert.deepEqual(origins.sort(), ["https://b.example", "https://linear.app"]);
  await mock.updateFillSecret(name, { origins: ["https://b.example"] });
  origins = (await mock.listFillGrants()).filter((g) => g.secret === name).map((g) => g.origin);
  assert.deepEqual(origins, ["https://b.example"]);
});

test("deleting an entry removes its grants", async () => {
  const mock = new MockApiClient();
  const [grant] = await mock.listFillGrants();
  await mock.deleteFillSecret(grant.secret);
  assert.equal((await mock.listFillGrants()).some((g) => g.secret === grant.secret), false);
});

test("grants never carry a value", async () => {
  const mock = new MockApiClient();
  for (const grant of await mock.listFillGrants()) {
    assert.equal("value" in grant, false);
  }
});
