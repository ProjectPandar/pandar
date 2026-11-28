"use client";

import { useQueries } from "@tanstack/react-query";
import { useSearchParams } from "next/navigation";
import { useEffect, useState } from "react";
import { useTranslations } from "next-intl";

import { parseSettingsSection } from "../../dashboard-shell";
import { QueryErrorBoundary } from "../../query-error-boundary";
import {
  settingsAdminRouteQueries,
  settingsRouteQueries,
} from "../../route-data";
import { SettingsDashboard } from "../../settings-dashboard";
import type { AuthMetadata, Tenant } from "../../dashboard-types";

export function SettingsPageClient({
  auth,
  selectedTenant,
  membership,
}: {
  auth: AuthMetadata;
  selectedTenant: Tenant;
  membership: { role: string | null; error: string | null };
}) {
  const t = useTranslations("settingsPage");
  const searchParams = useSearchParams();
  const activeSection = parseSettingsSection(searchParams.get("section"));
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    const interval = setInterval(() => setNowMs(Date.now()), 60_000);
    return () => clearInterval(interval);
  }, []);
  const canAdmin =
    auth.provider === "none" ||
    (membership.role === "tenant_admin" && membership.error === null);
  const [agentsQuery, printersQuery] = useQueries({
    queries: settingsRouteQueries(selectedTenant.id),
  });
  const [tokenOptions, auditOptions] = settingsAdminRouteQueries(
    selectedTenant.id,
  );
  const [tokensQuery, auditQuery] = useQueries({
    queries: [
      { ...tokenOptions, enabled: canAdmin },
      { ...auditOptions, enabled: canAdmin },
    ] as const,
  });
  const workspaceLoading = agentsQuery.isLoading || printersQuery.isLoading;
  const workspaceError = agentsQuery.error ?? printersQuery.error;

  if (workspaceLoading) {
    return <SettingsLoading />;
  }

  if (workspaceError) {
    return (
      <div
        className="rounded-xl border border-destructive/30 bg-destructive/10 p-4 text-sm text-destructive"
        role="alert"
      >
        <div className="font-medium">{t("loadErrorTitle")}</div>
        <p className="mt-1 text-destructive/90">{t("loadErrorDescription")}</p>
        <details className="mt-2 text-xs">
          <summary className="cursor-pointer font-medium">
            {t("errorDetails")}
          </summary>
          <div className="mt-1 break-all">
            {workspaceError instanceof Error
              ? workspaceError.message
              : String(workspaceError)}
          </div>
        </details>
      </div>
    );
  }

  const adminUnavailable =
    auth.provider !== "none" &&
    membership.error === null &&
    membership.role !== "tenant_admin";
  const adminLoadError =
    (auth.provider !== "none" && membership.error !== null) ||
    tokensQuery.error !== null ||
    auditQuery.error !== null;

  return (
    <QueryErrorBoundary>
      <SettingsDashboard
        activeSection={activeSection}
        adminLoadError={adminLoadError}
        adminLoading={
          canAdmin && (tokensQuery.isLoading || auditQuery.isLoading)
        }
        adminUnavailable={adminUnavailable}
        agents={agentsQuery.data ?? []}
        auditEvents={auditQuery.data ?? []}
        auth={auth}
        canAdmin={canAdmin}
        membershipRole={membership.role}
        nowMs={nowMs}
        printers={printersQuery.data ?? []}
        selectedTenant={selectedTenant}
        tenantTokens={tokensQuery.data ?? []}
      />
    </QueryErrorBoundary>
  );
}

function SettingsLoading() {
  return (
    <div className="mx-auto max-w-3xl animate-pulse pb-10">
      <div className="space-y-2 border-b border-border pb-6">
        <div className="h-8 w-36 rounded-lg bg-muted" />
        <div className="h-4 w-72 rounded-lg bg-muted/70" />
      </div>
      <div className="divide-y divide-border">
        <div className="py-5">
          <div className="h-14 rounded-lg bg-muted/70" />
        </div>
        <div className="py-5">
          <div className="h-24 rounded-lg bg-muted/70" />
        </div>
        <div className="py-5">
          <div className="h-16 rounded-lg bg-muted/70" />
        </div>
      </div>
    </div>
  );
}
