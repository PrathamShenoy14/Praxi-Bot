import assert from "node:assert/strict";
import { test } from "node:test";
import {
  EMPTY_QUEUE,
  enqueue,
  putBack,
  removeQueued,
  takeForNow,
  takeForRunEnd
} from "./messageQueue.ts";
import { sendButtonDisabled, sendButtonMode } from "./composerRun.ts";

test("a queued message stacks in order, can be removed, sent now, and is delivered when the run ends", () => {
  let q = enqueue(EMPTY_QUEUE, "  first ", "a", 1);
  q = enqueue(q, "second", "b", 2);
  q = enqueue(q, "third", "c", 3);
  assert.equal(enqueue(q, "   ", "x", 4), q, "blank text is never queued");
  assert.deepEqual(q.map((m) => m.text), ["first", "second", "third"]);

  // the small x
  q = removeQueued(q, "b");
  assert.deepEqual(q.map((m) => m.id), ["a", "c"]);

  // "Simdi gonder" takes exactly that one out
  const now = takeForNow(q, "c");
  assert.equal(now.message?.text, "third");
  assert.deepEqual(now.rest.map((m) => m.id), ["a"]);
  assert.equal(takeForNow(q, "nope").message, null);
  // a failed send goes back to the front
  assert.deepEqual(putBack(now.rest, now.message!).map((m) => m.id), ["c", "a"]);

  // the run ends: only the oldest message goes, the rest wait for the run it starts
  assert.equal(takeForRunEnd(q, true, true).message, null, "still running");
  assert.equal(takeForRunEnd(q, false, false).message, null, "was never running");
  const end = takeForRunEnd(q, true, false);
  assert.equal(end.message?.id, "a");
  assert.deepEqual(end.rest.map((m) => m.id), ["c"]);
  assert.equal(takeForRunEnd(EMPTY_QUEUE, true, false).message, null);

  // held messages (queued before a link opened the chat) stay in place; later ones still go in order
  const queuedHere = (m: { id: string }) => m.id === "c";
  const held = takeForRunEnd(q, true, false, queuedHere);
  assert.equal(held.message?.id, "c");
  assert.deepEqual(held.rest.map((m) => m.id), ["a"]);
  assert.equal(takeForRunEnd(held.rest, true, false, queuedHere).message, null, "nothing deliverable");
  assert.equal(takeForRunEnd(q, true, true, queuedHere).message, null, "still running");
});

test("the round button is a stop while the bot works and the box is empty, an arrow otherwise", () => {
  assert.equal(sendButtonMode({ runActive: true, hasText: false }), "stop");
  assert.equal(sendButtonMode({ runActive: true, hasText: true }), "send");
  assert.equal(sendButtonMode({ runActive: false, hasText: false }), "send");
  // a stop already under way cannot be pressed again; a send in flight cannot either
  assert.equal(sendButtonDisabled({ mode: "stop", sending: false, stopping: true }), true);
  assert.equal(sendButtonDisabled({ mode: "stop", sending: true, stopping: false }), false);
  assert.equal(sendButtonDisabled({ mode: "send", sending: true, stopping: false }), true);
});
