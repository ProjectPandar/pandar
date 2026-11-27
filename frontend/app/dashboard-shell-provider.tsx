"use client";

import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { usePathname } from "next/navigation";

import type { Tenant } from "./dashboard-types";
import type { DashboardView } from "./dashboard-shell";

/** Replaces the brand/title pair in the shell header, e.g. a mobile detail view with a back action. */
export type ShellHeaderOverride = {
  backLabel: string;
  title: string;
  meta: string;
  onBack: () => void;
};

export type DashboardShellContextValue = {
  shellView: DashboardView;
  shellTenant: Tenant | null;
  headerOverride: ShellHeaderOverride | null;
  setHeaderOverride: (override: ShellHeaderOverride | null) => void;
};

const DashboardShellContext = createContext<DashboardShellContextValue | null>(
  null,
);

export function useDashboardShell() {
  const context = useOptionalDashboardShell();
  if (!context) {
    throw new Error("useDashboardShell must be used within DashboardShellProvider");
  }
  return context;
}

/** Pages rendered outside the shell (tests, standalone panels) get null instead of throwing. */
export function useOptionalDashboardShell() {
  return useContext(DashboardShellContext);
}

export function DashboardShellProvider({
  children,
  selectedTenant,
}: {
  children: ReactNode;
  selectedTenant: Tenant | null;
}) {
  const pathname = usePathname();
  const view = (pathname.split("/")[1] || "devices") as DashboardView;
  const [headerOverride, setHeaderOverride] = useState<ShellHeaderOverride | null>(
    null,
  );

  const value = useMemo<DashboardShellContextValue>(
    () => ({
      shellView: view,
      shellTenant: selectedTenant,
      headerOverride,
      setHeaderOverride,
    }),
    [view, selectedTenant, headerOverride],
  );

  return (
    <DashboardShellContext.Provider value={value}>
      {children}
    </DashboardShellContext.Provider>
  );
}

/** Keeps the shell header in sync with `override`, clearing it when the page unmounts. */
export function useShellHeaderOverride(override: ShellHeaderOverride | null) {
  const setHeaderOverride = useOptionalDashboardShell()?.setHeaderOverride;
  const backLabel = override?.backLabel;
  const title = override?.title;
  const meta = override?.meta;
  const onBack = override?.onBack;

  useEffect(() => {
    if (!setHeaderOverride || !override) {
      return;
    }
    setHeaderOverride(override);
    return () => setHeaderOverride(null);
    // Keyed on the override's fields so callers may build the object inline.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [setHeaderOverride, backLabel, title, meta, onBack]);
}
