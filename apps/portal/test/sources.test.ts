import assert from "node:assert/strict";
import { test } from "node:test";

import { fieldNameFor, idFor, keptSecret } from "../src/modules/config/lib/sources.lib.ts";

test("source ids and field names do not clash", () => {
  assert.equal(idFor("Bitcoin price", ["bitcoin-price"]), "bitcoin-price-2");
  assert.equal(fieldNameFor("json.current.temperature_2m", []), "temperature_2m");
  assert.equal(fieldNameFor("json.ok", []), "ok_value");
});

test("a key uses its own kept secret, else its source's", () => {
  const secrets = ["ha//token", "ha/w1/token"];
  assert.equal(keptSecret(secrets, "ha", "w1", "token"), "own");
  assert.equal(keptSecret(secrets, "ha", "w2", "token"), "shared");
  assert.equal(keptSecret(secrets, "ha", null, "token"), "shared");
  assert.equal(keptSecret(secrets, "fx", "w1", "token"), null);
});
