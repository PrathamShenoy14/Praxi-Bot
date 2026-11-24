import type { Bot, Chat, Run } from "../api/types.ts";
import { botLanding, type BotLanding } from "./botLanding.ts";

export type DeepLink = { kind: "bot" | "chat" | "run"; id: string } | { kind: "invalid" };
export type DeepLinkFailure = "invalid" | "missing" | "offline" | "blocked";
type Destination = BotLanding | { kind: "run"; run: Run };

type Refresh<T> = (current: () => boolean) => Promise<readonly T[] | null>;
type Session = {
  key: object;
  refreshBots: Refresh<Bot>;
  refreshChats: Refresh<Chat>;
  refreshRuns: Refresh<Run>;
};
type Scope = {
  owner: object;
  computerId: string | null;
  selected: () => boolean;
  connection: Session | "waiting" | "offline";
  open: (destination: Destination) => boolean;
  feedback: (failure: DeepLinkFailure) => void;
};

async function resolveLink(link: Exclude<DeepLink, { kind: "invalid" }>, session: Session, current: () => boolean): Promise<Destination | null> {
  if (link.kind === "run") {
    const run = (await session.refreshRuns(current))?.find((item) => item.id === link.id);
    return run ? { kind: "run", run } : null;
  }
  if (link.kind === "bot") {
    const bots = await session.refreshBots(current);
    if (!current() || !bots?.some((bot) => bot.id === link.id)) return null;
    const chats = await session.refreshChats(current);
    return chats ? botLanding(link.id, chats) : null;
  }
  const chat = (await session.refreshChats(current))?.find((item) => item.id === link.id);
  if (!current() || !chat) return null;
  const bots = await session.refreshBots(current);
  return bots?.some((bot) => bot.id === chat.bot_id)
    ? { kind: "chat", chatId: chat.id, botId: chat.bot_id }
    : null;
}

/** One mailbox and one lookup across the lifetimes of the selected computer views. */
export function createDeepLinkDispatcher() {
  let scope: Scope | null = null;
  let pending: { link: DeepLink; computerId: string | null } | null = null;
  let lookingUp = false;
  let wake = false;
  let draining: Promise<void> | null = null;

  async function dispatch() {
    if (draining || lookingUp || !pending || !scope || scope.connection === "waiting" || !scope.selected()) return;
    const request = pending;
    const { owner, connection } = scope;
    const current = () =>
      pending === request && scope?.owner === owner && scope.selected() &&
      (typeof connection === "string" ? scope.connection === connection
        : typeof scope.connection !== "string" && scope.connection.key === connection.key);
    lookingUp = true;
    let outcome: Destination | DeepLinkFailure = "missing";
    try {
      if (request.link.kind === "invalid") outcome = "invalid";
      else if (connection === "offline") outcome = "offline";
      else outcome = (await resolveLink(request.link, connection, current)) ?? "missing";
    } catch {
      outcome = "offline";
    }
    // A wake during the lookup can replace the request before the UI applies its result.
    while (draining) await draining;
    const latest = scope;
    if (current() && latest) {
      pending = null;
      if (typeof outcome === "string") latest.feedback(outcome);
      else if (!latest.open(outcome)) latest.feedback("blocked");
    }
    // Otherwise a stale result is dropped but the link stays pending: a reconnect retries it
    // on the new session below, and a computer change already discarded it in `update()`.
    lookingUp = false;
    void dispatch();
  }

  function drain(take: () => Promise<DeepLink | null>) {
    wake = true;
    if (draining) return draining;
    draining = (async () => {
      let newest: DeepLink | null = null;
      let computerId = scope?.computerId ?? null;
      while (wake) {
        wake = false;
        const target = scope?.computerId ?? null;
        try {
          const link = await take();
          if (link) {
            newest = link;
            computerId = target;
          }
        } catch {
          // A failed IPC read leaves any already-resolved intent intact.
        }
      }
      if (newest && (computerId === null || !scope || (scope.computerId === computerId && scope.selected()))) {
        pending = { link: newest, computerId: computerId ?? scope?.computerId ?? null };
      }
    })().finally(() => {
      draining = null;
      // The next wake may already be queued when the last take settles.
      if (wake) void drain(take);
      else void dispatch();
    });
    return draining;
  }

  return {
    update(next: Scope) {
      scope = next;
      if (pending && next.computerId !== null) {
        if (pending.computerId === null) pending.computerId = next.computerId;
        else if (pending.computerId !== next.computerId) pending = null;
      }
      void dispatch();
    },
    detach(owner: object) {
      if (scope?.owner === owner) scope = null;
    },
    drain
  };
}

/** Subscribe before taking the startup link. An IPC take that began before cleanup may still complete. */
export function subscribeDeepLinks(
  dispatcher: ReturnType<typeof createDeepLinkDispatcher>,
  listen: (wake: () => void) => Promise<() => void>,
  take: () => Promise<DeepLink | null>
): () => void {
  let active = true;
  let unlisten: (() => void) | undefined;
  const wake = () => {
    if (active) void dispatcher.drain(take);
  };
  void listen(wake).then((release) => {
    if (!active) {
      release();
      return;
    }
    unlisten = release;
    wake();
  }).catch(() => {});
  return () => {
    active = false;
    unlisten?.();
  };
}
