/**
 * One gate for everything that leaves the bot the user is editing: switching bot, opening a chat,
 * closing the panel, the command palette, global navigation (review of PR #95, finding 4).  A
 * dialog with unsaved data registers a guard; the app routes every such action through `attempt`.
 */

export interface LeaveGuard {
  /** Read at the moment of the attempt. */
  state: () => { dirty: boolean; busy: boolean };
  /** Ask the user; call `proceed` only when they agree to discard. */
  confirm: (proceed: () => void) => void;
}

export type LeaveResult = "went" | "pending" | "blocked";

export interface LeaveRegistry {
  register: (guard: LeaveGuard, silentOnly?: boolean) => () => void;
  /** Run `action` now, after the user agreed, or never (a save is in flight). */
  attempt: (action: () => void) => LeaveResult;
  /** External navigation can proceed only when no guard is dirty or busy. */
  attemptSilently: (action: () => void) => LeaveResult;
}

export function createLeaveRegistry(): LeaveRegistry {
  const guards: { guard: LeaveGuard; silentOnly: boolean }[] = [];
  return {
    register(guard, silentOnly = false) {
      const entry = { guard, silentOnly };
      guards.push(entry);
      return () => {
        const i = guards.indexOf(entry);
        if (i >= 0) guards.splice(i, 1);
      };
    },
    attempt(action) {
      const live = guards.filter((entry) => !entry.silentOnly).map((entry) => entry.guard).reverse();
      const states = live.map((g) => g.state());
      if (states.some((s) => s.busy)) return "blocked";
      const dirty = live.filter((_, i) => states[i]?.dirty);
      if (dirty.length === 0) {
        action();
        return "went";
      }
      const ask = (rest: LeaveGuard[]) => {
        const [first, ...others] = rest;
        if (!first) {
          action();
          return;
        }
        first.confirm(() => ask(others));
      };
      ask(dirty);
      return "pending";
    },
    attemptSilently(action) {
      if (guards.some(({ guard }) => {
        const { dirty, busy } = guard.state();
        return dirty || busy;
      })) return "blocked";
      action();
      return "went";
    }
  };
}
