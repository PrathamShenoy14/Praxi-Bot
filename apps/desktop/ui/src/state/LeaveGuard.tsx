import { createContext, type ReactNode, useContext, useLayoutEffect, useMemo, useRef } from "react";
import { createLeaveRegistry, type LeaveGuard, type LeaveRegistry } from "../lib/leaveGuard";

const LeaveContext = createContext<LeaveRegistry | null>(null);

/** Owns the guard registry of the app shell. */
export function LeaveGuardProvider({
  registry,
  children
}: {
  registry?: LeaveRegistry;
  children: ReactNode;
}) {
  const value = useMemo(() => registry ?? createLeaveRegistry(), [registry]);
  return <LeaveContext.Provider value={value}>{children}</LeaveContext.Provider>;
}

/** The registry an app shell action goes through (`attempt`). */
export function useLeaveRegistry(): LeaveRegistry {
  const reg = useContext(LeaveContext);
  const fallback = useMemo(() => createLeaveRegistry(), []);
  return reg ?? fallback;
}

/** Register a dialog's unsaved-changes guard for as long as it is mounted. */
export function useLeaveGuard(guard: LeaveGuard, silentOnly = false) {
  const registry = useLeaveRegistry();
  const latest = useRef(guard);
  latest.current = guard;
  useLayoutEffect(
    () =>
      registry.register({
        state: () => latest.current.state(),
        confirm: (proceed) => latest.current.confirm(proceed)
      }, silentOnly),
    [registry, silentOnly]
  );
}
