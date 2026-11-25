import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const LOCALES = ["en", "tr", "de", "es", "fr", "pt", "id"] as const;
const PROSE = ["explainDoes", "explainDoesNot", "explainRevoke", "empty", "confirmTitle", "confirmBody", "confirmLastBody"];

type Grants = Record<string, string>;
const grants = (lang: string): Grants => {
  const json = JSON.parse(readFileSync(new URL(`../i18n/locales/${lang}.json`, import.meta.url), "utf8"));
  return json.vault.grants as Grants;
};
const placeholders = (s: string): string[] => [...s.matchAll(/\{\{\s*(\w+)\s*\}\}/g)].map((m) => m[1]!).sort();

const en = grants("en");

test("english has the full key set", () => {
  assert.ok(Object.keys(en).length >= 16);
});

for (const lang of LOCALES) {
  test(`${lang} has every vault.grants key with the same placeholders`, () => {
    const loc = grants(lang);
    for (const [key, value] of Object.entries(en)) {
      assert.equal(typeof loc[key], "string", `${lang}: missing vault.grants.${key}`);
      assert.ok(loc[key]!.trim().length > 0, `${lang}: empty vault.grants.${key}`);
      assert.deepEqual(placeholders(loc[key]!), placeholders(value), `${lang}: placeholders of ${key}`);
    }
  });
}

for (const lang of ["tr", "de", "es"] as const) {
  test(`${lang} translates the prose keys`, () => {
    const loc = grants(lang);
    for (const key of PROSE) assert.notEqual(loc[key], en[key], `${lang}: ${key} is still English`);
  });
}

test("english wording states the exact effect of a revoke", () => {
  assert.match(en.confirmBody!, /immediately/);
  assert.match(en.confirmLastBody!, /immediately/);
  assert.match(en.confirmLastBody!, /Ask before use/);
  assert.match(en.explainRevoke!, /immediately/);
});
