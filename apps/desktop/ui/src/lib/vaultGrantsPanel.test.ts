import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { after, before, test } from "node:test";
import { createElement, type ComponentType } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import type { i18n as I18n } from "i18next";
import { createServer, type ViteDevServer } from "vite";
import type { FillGrantInfo } from "../api/types.ts";
import type { GrantGroup } from "./vault.ts";
import type { VaultGrantsPanelProps } from "../views/VaultGrantsPanel.tsx";

let server: ViteDevServer;
let i18n: I18n;
let VaultGrantsPanel: ComponentType<VaultGrantsPanelProps>;

before(async () => {
  server = await createServer({
    root: fileURLToPath(new URL("../..", import.meta.url)),
    logLevel: "error",
    optimizeDeps: { noDiscovery: true, entries: [] },
    server: { middlewareMode: true, watch: null, hmr: false },
    ssr: { noExternal: ["ui"] }
  });
  ({ i18n } = await server.ssrLoadModule("/src/i18n/index.ts"));
  ({ VaultGrantsPanel } = await server.ssrLoadModule("/src/views/VaultGrantsPanel.tsx"));
  await i18n.changeLanguage("en");
});
after(async () => {
  await server?.close();
});

const grant = (id: string, secret: string, origin: string): FillGrantInfo =>
  ({ id, secret, origin, created_at_ms: Date.UTC(2026, 5, 3, 12) });

const groups: GrantGroup[] = [
  { name: "login/linear", item: null, label: "Linear", grants: [grant("g1", "login/linear", "https://linear.app")] },
  {
    name: "login/github", item: null, label: "login/github",
    grants: [grant("g2", "login/github", "https://github.com"), grant("g3", "login/github", "https://gist.github.com")]
  }
];

const noop = () => {};
const render = (over: Partial<VaultGrantsPanelProps> = {}): string =>
  renderToStaticMarkup(createElement(VaultGrantsPanel, {
    groups, loading: false, error: null, busyId: null, onRetry: noop, onRevoke: noop, onBack: noop, ...over
  }));

test("lists grants in a labelled list with named Revoke buttons", () => {
  const html = render();
  assert.match(html, /data-testid="vault-grants-panel"/);
  assert.match(html, /<ul[^>]*aria-label="Standing grants by vault item"/);
  assert.match(html, /data-testid="vault-grant-revoke-g1"[^>]*aria-label="Revoke the standing grant for Linear on https:\/\/linear\.app"/);
  assert.match(html, /aria-label="Revoke the standing grant for login\/github on https:\/\/gist\.github\.com"/);
  assert.equal((html.match(/data-testid="vault-grant-revoke-/g) ?? []).length, 3);
  assert.match(html, /Since /);
  assert.match(html, /data-testid="vault-grants-back"/);
  assert.doesNotMatch(html, /vault-grants-empty/);
});

test("always shows the three explanation paragraphs", () => {
  for (const html of [render(), render({ groups: [] }), render({ error: "boom" })]) {
    assert.match(html, /fill it on one exact website/);
    assert.match(html, /never covers other websites/);
    assert.match(html, /Revoking takes effect immediately/);
  }
});

test("empty state has no list", () => {
  const html = render({ groups: [] });
  assert.match(html, /data-testid="vault-grants-empty"/);
  assert.match(html, /role="status"/);
  assert.match(html, /No standing grants\./);
  assert.doesNotMatch(html, /vault-grants-list/);
});

test("error shows an alert and a retry button", () => {
  const html = render({ groups: [], error: "Could not load" });
  assert.match(html, /<p role="alert">Could not load<\/p>/);
  assert.match(html, /Try again/);
  assert.doesNotMatch(html, /vault-grants-list|vault-grants-empty/);
});

test("the grant being revoked has a disabled button", () => {
  const html = render({ busyId: "g2" });
  assert.match(html, /<button[^>]*data-testid="vault-grant-revoke-g2"[^>]*disabled/);
  assert.doesNotMatch(html, /<button[^>]*data-testid="vault-grant-revoke-g1"[^>]*disabled/);
});
