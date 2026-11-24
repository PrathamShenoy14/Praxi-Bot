import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import type { Bot, Chat, Run } from "../api/types.ts";
import { createDeepLinkDispatcher, subscribeDeepLinks, type DeepLink } from "./deepLinks.ts";
import { createLeaveRegistry } from "./leaveGuard.ts";
import { buildRunReportFromFacts } from "./runReport.ts";

type Scope = Parameters<ReturnType<typeof createDeepLinkDispatcher>["update"]>[0];
type Session = Exclude<Scope["connection"], string>;

const bot = { id: "bot" } as Bot;
const chat = (id: string, created_at_ms: number, attrs: Partial<Chat> = {}): Chat => ({
  id, chat_id: id, bot_id: bot.id, created_at_ms, title: id, ...attrs
});
const childRun: Run = {
  id: "child", bot_id: bot.id, parent_run_id: "parent", chat_id: null,
  kind: "turn", state: "failed", created_at_ms: 1000, updated_at_ms: 1200, error: "private failure text"
};
const tick = () => new Promise<void>((resolve) => setImmediate(resolve));

function setup() {
  const dispatcher = createDeepLinkDispatcher();
  const reads: string[] = [];
  const opened: Parameters<Scope["open"]>[0][] = [];
  const feedback: Parameters<Scope["feedback"]>[0][] = [];
  const mutations: string[] = [];
  const client = {
    refreshBots: async () => { reads.push("bots"); return [bot]; },
    refreshChats: async () => {
      reads.push("chats");
      return [chat("pinned", 1, { pinned: true }), chat("newest", 2), chat("archived", 3, { archived: true })];
    },
    refreshRuns: async () => {
      reads.push("runs");
      return [{ ...childRun, id: "parent", parent_run_id: null }, childRun];
    },
    sendMessage: () => mutations.push("send"),
    createChat: () => mutations.push("create"),
    cancelRun: () => mutations.push("cancel"),
    approve: () => mutations.push("approve"),
    export: () => mutations.push("export"),
    selectComputer: () => mutations.push("select computer")
  };
  const connection: Session = { key: {}, ...client };
  const scope: Scope = {
    owner: {}, computerId: "local", connection,
    selected: () => true,
    open: (destination) => { opened.push(destination); return true; },
    feedback: (failure) => feedback.push(failure)
  };
  const take = async (link: DeepLink) => {
    await dispatcher.drain(async () => link);
    await tick();
  };
  return { dispatcher, scope, connection, take, reads, opened, feedback, mutations };
}

test("fresh bot landing and exact chat reference only the selected session lists", async () => {
  const { dispatcher, scope, take, reads, opened, mutations } = setup();
  dispatcher.update(scope);
  await take({ kind: "bot", id: bot.id });
  await take({ kind: "chat", id: "archived" });
  assert.deepEqual(reads, ["bots", "chats", "chats", "bots"]);
  assert.deepEqual(opened, [
    { kind: "chat", chatId: "pinned", botId: bot.id },
    { kind: "chat", chatId: "archived", botId: bot.id }
  ]);
  dispatcher.update({
    ...scope,
    connection: { ...scope.connection as Session, refreshChats: async () => [] }
  });
  await take({ kind: "bot", id: bot.id });
  assert.deepEqual(opened.at(-1), { kind: "bot", botId: bot.id });
  assert.deepEqual(mutations, []);
});

test("run link opens the exact child without a chat, and DTO fallback exposes only report facts", async () => {
  const { dispatcher, scope, take, reads, opened, mutations } = setup();
  dispatcher.update(scope);
  await take({ kind: "run", id: childRun.id });
  assert.deepEqual(reads, ["runs"]);
  assert.deepEqual(opened, [{ kind: "run", run: childRun }]);
  for (const [state, outcome, finishedMs] of [
    ["queued", "queued", null],
    ["running", "running", null],
    ["waiting_approval", "waitingApproval", null],
    ["paused", "paused", null],
    ["succeeded", "done", 1200],
    ["failed", "failed", 1200],
    ["cancelled", "stopped", 1200]
  ] as const) {
    assert.deepEqual(buildRunReportFromFacts({ ...childRun, state }, "Bot"), {
      botName: "Bot", startedMs: 1000, finishedMs, outcome, error: null, steps: []
    });
  }
  assert.deepEqual(mutations, []);
});

test("startup keeps the newest link until settings and the initial selected session are ready", async () => {
  const { dispatcher, scope, take, reads, opened } = setup();
  dispatcher.update({ ...scope, computerId: null, connection: "waiting" });
  await take({ kind: "chat", id: "archived" });
  await take({ kind: "run", id: childRun.id });
  assert.deepEqual(reads, []);
  dispatcher.detach(scope.owner);
  const host = { ...scope, owner: {}, computerId: "host" };
  dispatcher.update({ ...host, connection: "waiting" });
  assert.deepEqual(reads, []);
  dispatcher.update(host);
  await tick();
  assert.deepEqual(opened, [{ kind: "run", run: childRun }]);
  assert.deepEqual(reads, ["runs"]);
});

test("new wake during a lookup replaces it before opening and never starts a concurrent lookup", async () => {
  const { dispatcher, scope, connection, take, reads, opened, feedback } = setup();
  let returnBots: (value: Bot[]) => void = () => assert.fail("no bot read");
  const slowBots = new Promise<Bot[]>((resolve) => { returnBots = resolve; });
  dispatcher.update({
    ...scope,
    connection: { ...connection, refreshBots: () => { reads.push("slow bots"); return slowBots; } }
  });
  await take({ kind: "bot", id: bot.id });
  let takeLatest: (link: DeepLink) => void = () => assert.fail("no IPC take");
  const next = new Promise<DeepLink>((resolve) => { takeLatest = resolve; });
  void dispatcher.drain(() => next);
  returnBots([]);
  await tick();
  assert.deepEqual(opened, []);
  assert.deepEqual(feedback, []);
  assert.deepEqual(reads, ["slow bots"]);
  takeLatest({ kind: "run", id: childRun.id });
  await tick();
  assert.deepEqual(reads, ["slow bots", "runs"]);
  assert.deepEqual(opened, [{ kind: "run", run: childRun }]);
  assert.deepEqual(feedback, []);
});

test("a reconnect during a lookup keeps the link and retries it on the new session", async () => {
  const { dispatcher, scope, connection, take, reads, opened, feedback } = setup();
  let resolveOld: (runs: Run[]) => void = () => assert.fail("no old lookup");
  const old = new Promise<Run[]>((resolve) => { resolveOld = resolve; });
  dispatcher.update({
    ...scope,
    connection: { ...connection, refreshRuns: () => { reads.push("old runs"); return old; } }
  });
  await take({ kind: "run", id: childRun.id });
  dispatcher.update({ ...scope, connection: "waiting" });
  resolveOld([childRun]);
  await tick();
  assert.deepEqual(opened, []);
  assert.deepEqual(feedback, []);
  dispatcher.update({ ...scope, connection: { ...connection, key: {} } });
  await tick();
  assert.deepEqual(reads, ["old runs", "runs"]);
  assert.deepEqual(opened, [{ kind: "run", run: childRun }]);
  assert.deepEqual(feedback, []);
});

test("switching computers discards the old destination and never looks it up on the new host", async () => {
  const { dispatcher, scope, connection, take, reads, opened, feedback, mutations } = setup();
  let resolveOld: (chats: Chat[]) => void = () => assert.fail("no old lookup");
  const old = new Promise<Chat[]>((resolve) => { resolveOld = resolve; });
  let current: () => boolean = () => true;
  let selected = true;
  dispatcher.update({
    ...scope,
    selected: () => selected,
    connection: { ...connection, refreshChats: (valid) => { current = valid; return old; } }
  });
  await take({ kind: "chat", id: "old" });
  selected = false;
  assert.equal(current(), false);
  dispatcher.detach(scope.owner);
  dispatcher.update({ ...scope, owner: {}, computerId: "host" });
  assert.equal(current(), false);
  resolveOld([chat("old", 1)]);
  await tick();
  assert.deepEqual(opened, []);
  assert.deepEqual(feedback, []);
  assert.deepEqual(reads, []);
  assert.deepEqual(mutations, []);
  await take({ kind: "run", id: childRun.id });
  assert.deepEqual(opened, [{ kind: "run", run: childRun }]);
});

test("invalid, missing, offline and dirty destinations use generic feedback and never prompt", async () => {
  const { dispatcher, scope, connection, take, reads, opened, feedback, mutations } = setup();
  dispatcher.update(scope);
  await take({ kind: "invalid" });
  await take({ kind: "chat", id: "deleted" });
  dispatcher.update({ ...scope, connection: "offline" });
  await take({ kind: "run", id: childRun.id });
  const leave = createLeaveRegistry();
  leave.register({
    state: () => ({ dirty: true, busy: false }),
    confirm: () => assert.fail("an external link prompted")
  });
  dispatcher.update({
    ...scope, connection, open: (destination) => leave.attemptSilently(() => scope.open(destination)) === "went"
  });
  await take({ kind: "run", id: childRun.id });
  assert.deepEqual(feedback, ["invalid", "missing", "offline", "blocked"]);
  assert.deepEqual(reads, ["chats", "runs"]);
  assert.deepEqual(opened, []);
  assert.deepEqual(mutations, []);
});

test("listener startup and cleanup handle StrictMode and re-drain a wake during the take", async () => {
  const { dispatcher, scope, opened, reads } = setup();
  dispatcher.update(scope);
  const subscriptions: { wake: () => void; ready: (release: () => void) => void }[] = [];
  const listen = (wake: () => void) => new Promise<() => void>((ready) => subscriptions.push({ wake, ready }));
  const takes: ((link: DeepLink | null) => void)[] = [];
  const take = () => new Promise<DeepLink | null>((resolve) => takes.push(resolve));
  const removed: number[] = [];
  subscribeDeepLinks(dispatcher, listen, take)();
  const unsubscribe = subscribeDeepLinks(dispatcher, listen, take);
  assert.equal(takes.length, 0);
  subscriptions[0]!.ready(() => removed.push(0));
  subscriptions[1]!.ready(() => removed.push(1));
  await tick();
  assert.deepEqual(removed, [0]);
  assert.equal(takes.length, 1);
  subscriptions[1]!.wake();
  takes[0]!({ kind: "bot", id: bot.id });
  await tick();
  assert.equal(takes.length, 2);
  assert.deepEqual(reads, []);
  takes[1]!({ kind: "run", id: childRun.id });
  await tick();
  assert.deepEqual(opened, [{ kind: "run", run: childRun }]);
  unsubscribe();
  subscriptions[1]!.wake();
  assert.equal(takes.length, 2);
  assert.deepEqual(removed, [0, 1]);
});

test("desktop wiring keeps exact event history and forwards chat suppression without altering notification routes", () => {
  const read = (file: string) => readFileSync(new URL(file, import.meta.url), "utf8");
  const app = read("../App.tsx");
  const chatView = read("../views/ChatView.tsx");
  const shell = read("./shell.ts");
  assert.match(shell, /window\.listen\("shell:deep_link_pending", wake\)/);
  assert.match(shell, /invoke<DeepLink \| null>\("deep_link_take"\)/);
  assert.match(app, /events\.runs\[linkedRun\.id\]/);
  assert.match(app, /external=\{route\.external\}/);
  assert.match(app, /<ShellBridgeHost[\s\S]*?onOpenChat=\{openChat\}/);
  assert.match(chatView, /useAutoSpeak\(chatId, chatRuns, external\)/);
  assert.match(chatView, /useChatRead\([\s\S]*?messages,\s*external/);
  assert.match(chatView, /followNewSince\.current = external \? null/);
  // An explicit `/new` sent after a link opened the chat still follows the new chat.
  assert.match(chatView, /followNewSince\.current === null && parseSlashInput\(body\)\?\.name === "new"/);
  // At run end a linked chat delivers only messages queued in this view, never ones queued before.
  assert.match(chatView, /queuedHere\.current\.add\(id\);\s*updateQueue\(\(q\) => enqueue\(q, body, id,/);
  assert.match(chatView, /takeForRunEnd\(queue, was, active, \(m\) => !external \|\| queuedHere\.current\.has\(m\.id\)\)/);
});
