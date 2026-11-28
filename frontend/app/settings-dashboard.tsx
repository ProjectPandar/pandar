"use client";

import Link from "next/link";
import { useTranslations } from "next-intl";
import {
  ArrowUpRightIcon,
  MonitorCogIcon,
  ServerCogIcon,
  ShieldCheckIcon,
  SunIcon,
} from "lucide-react";

import { FormattedDate } from "../components/formatted-date";
import { LanguageSwitcher } from "../components/language-switcher";
import { ThemeSwitcher } from "../components/theme-switcher";
import { OFFLINE_PRINTER_STATUSES } from "./dashboard-attention";
import { CreateAgentPairingForm } from "./create-agent-pairing-form";
import { TenantAuditPanel } from "./tenant-audit-panel";
import { TenantTokensTable } from "./admin-settings-token-list";
import type {
  Agent,
  AuditEvent,
  AuthMetadata,
  Printer,
  Tenant,
  TenantToken,
} from "./dashboard-types";
import {
  dashboardSidebarHref,
  SETTINGS_SECTION_META,
  type SettingsSectionId,
} from "./dashboard-shell";
import { SettingsAccountSection } from "./settings-account-section";
import { WorkspaceRenameForm } from "./settings-rename-form";

type SettingsDashboardProps = {
  activeSection?: SettingsSectionId;
  auth: AuthMetadata;
  selectedTenant: Tenant;
  membershipRole: string | null;
  canAdmin: boolean;
  agents: Agent[];
  printers: Printer[];
  tenantTokens: TenantToken[];
  auditEvents: AuditEvent[];
  adminUnavailable: boolean;
  adminLoadError: boolean;
  adminLoading: boolean;
  nowMs: number;
};

export function SettingsDashboard(props: SettingsDashboardProps) {
  const t = useTranslations("settingsPage");
  const tRoles = useTranslations("tokens");
  const activeSection = props.activeSection ?? "workspace";
  const sectionMeta = SETTINGS_SECTION_META[activeSection];
  const roleLabel = props.membershipRole
    ? tRoles(props.membershipRole)
    : props.auth.provider === "none"
      ? t("localAdministrator")
      : t("roleUnavailable");

  return (
    <div className="mx-auto max-w-3xl pb-10">
      <header className="flex flex-wrap items-center justify-between gap-3 border-b border-border pb-4 sm:pb-6">
        <div>
          <h2 className="text-2xl font-semibold tracking-tight text-foreground">
            {t(sectionMeta.titleKey)}
          </h2>
          <p className="mt-1 text-sm text-muted-foreground">
            {t(sectionMeta.descriptionKey)}
          </p>
        </div>
        <span className="inline-flex items-center gap-1.5 rounded-full border border-border bg-muted/40 px-3 py-1 text-xs font-medium text-muted-foreground">
          <ShieldCheckIcon aria-hidden="true" className="size-3.5 text-success" />
          {roleLabel}
        </span>
      </header>

      <div className="min-w-0">
        {activeSection === "workspace" ? <WorkspaceSection {...props} /> : null}
        {activeSection === "appearance" ? <AppearanceSection /> : null}
        {activeSection === "access" ? <AccessSection {...props} /> : null}
        {activeSection === "account" ? (
          <SettingsAccountSection auth={props.auth} />
        ) : null}
      </div>
    </div>
  );
}

function WorkspaceSection(props: SettingsDashboardProps) {
  const t = useTranslations("settingsPage");
  const connectedAgents = props.agents.filter(
    (agent) => agent.status.toLowerCase() === "online",
  ).length;
  const onlinePrinters = props.printers.filter(
    (printer) => !OFFLINE_PRINTER_STATUSES.has(printer.status.toLowerCase()),
  ).length;

  return (
    <section className="divide-y divide-border" id="workspace">
      {props.canAdmin ? (
        <WorkspaceRenameForm tenant={props.selectedTenant} />
      ) : null}
      <dl className="grid gap-4 py-5 sm:grid-cols-2 sm:gap-5">
        {props.canAdmin ? null : (
          <WorkspaceFact
            label={t("workspaceName")}
            value={props.selectedTenant.display_name}
          />
        )}
        <WorkspaceFact
          label={t("workspaceSlug")}
          value={props.selectedTenant.slug}
          mono
        />
        <WorkspaceFact
          label={t("workspaceCreated")}
          value={<FormattedDate value={props.selectedTenant.created_at} />}
        />
        <WorkspaceFact
          label={t("workspaceId")}
          value={props.selectedTenant.id}
          mono
        />
        <WorkspaceFact label={t("authProvider")} value={props.auth.provider} />
      </dl>
      <div className="divide-y divide-border">
        <WorkspaceLink
          description={t("agentsDescription", {
            connected: connectedAgents,
            total: props.agents.length,
          })}
          href={dashboardSidebarHref("agents")}
          icon={ServerCogIcon}
          title={t("manageAgents")}
        />
        <WorkspaceLink
          description={t("printersDescription", {
            online: onlinePrinters,
            total: props.printers.length,
          })}
          href={dashboardSidebarHref("devices")}
          icon={MonitorCogIcon}
          title={t("managePrinters")}
        />
      </div>
    </section>
  );
}

function AppearanceSection() {
  const t = useTranslations("settingsPage");

  return (
    <section className="divide-y divide-border" id="appearance">
      <PreferenceRow
        description={t("languageDescription")}
        icon={MonitorCogIcon}
        title={t("languageTitle")}
      >
        <LanguageSwitcher />
      </PreferenceRow>
      <PreferenceRow
        description={t("themeDescription")}
        icon={SunIcon}
        title={t("themeTitle")}
      >
        <ThemeSwitcher />
      </PreferenceRow>
    </section>
  );
}

function AccessSection(props: SettingsDashboardProps) {
  const t = useTranslations("settingsPage");

  if (props.adminLoading) {
    return (
      <section className="space-y-3 py-5" id="access" aria-label={t("loadingAccess")}>
        <div className="h-16 animate-pulse rounded-lg bg-muted" />
        <div className="h-16 animate-pulse rounded-lg bg-muted" />
      </section>
    );
  }

  if (props.adminUnavailable || props.adminLoadError) {
    return (
      <section className="py-5" id="access">
        <div
          className="flex gap-3"
          role={props.adminLoadError ? "alert" : undefined}
        >
          <ShieldCheckIcon
            aria-hidden="true"
            className="mt-0.5 size-5 shrink-0 text-muted-foreground"
          />
          <div>
            <div className="text-sm font-medium text-foreground">
              {props.adminLoadError
                ? t("accessLoadErrorTitle")
                : t("accessRestrictedTitle")}
            </div>
            <p className="mt-1 text-sm leading-6 text-muted-foreground">
              {props.adminLoadError
                ? t("accessLoadErrorDescription")
                : t("accessRestrictedDescription")}
            </p>
          </div>
        </div>
      </section>
    );
  }

  return (
    <section className="divide-y divide-border" id="access">
      <div className="py-5">
        <TenantTokensTable
          nowMs={props.nowMs}
          tenantId={props.selectedTenant.id}
          tokens={props.tenantTokens}
        />
      </div>
      <div className="py-5">
        <h3 className="text-sm font-semibold text-foreground">
          {t("pairAgentTitle")}
        </h3>
        <p className="mt-0.5 text-sm text-muted-foreground">
          {t("pairAgentDescription")}
        </p>
        <div className="mt-4">
          <CreateAgentPairingForm inline tenantId={props.selectedTenant.id} />
        </div>
      </div>
      <div className="py-5">
        <TenantAuditPanel auditEvents={props.auditEvents} />
      </div>
    </section>
  );
}

function PreferenceRow({
  icon: Icon,
  title,
  description,
  children,
}: {
  icon: typeof SunIcon;
  title: string;
  description: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-3 py-5 sm:flex-row sm:items-center sm:justify-between">
      <div className="flex items-start gap-3">
        <Icon
          aria-hidden="true"
          className="mt-0.5 size-4 shrink-0 text-muted-foreground"
        />
        <div>
          <div className="text-sm font-medium text-foreground">{title}</div>
          <div className="mt-0.5 text-sm text-muted-foreground">
            {description}
          </div>
        </div>
      </div>
      <div className="shrink-0 sm:pl-4">{children}</div>
    </div>
  );
}

function WorkspaceFact({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: React.ReactNode;
  mono?: boolean;
}) {
  return (
    <div>
      <dt className="text-xs font-medium text-muted-foreground">{label}</dt>
      <dd
        className={`mt-1 break-all text-foreground ${
          mono ? "font-mono text-xs" : "text-sm"
        }`}
      >
        {value}
      </dd>
    </div>
  );
}

function WorkspaceLink({
  icon: Icon,
  title,
  description,
  href,
}: {
  icon: typeof ServerCogIcon;
  title: string;
  description: string;
  href: string;
}) {
  return (
    <Link
      className="group flex items-center justify-between gap-4 py-4 transition-colors hover:text-foreground"
      href={href}
    >
      <div className="flex items-start gap-3">
        <Icon
          aria-hidden="true"
          className="mt-0.5 size-4 shrink-0 text-muted-foreground"
        />
        <div>
          <div className="text-sm font-medium text-foreground">{title}</div>
          <p className="mt-0.5 text-xs text-muted-foreground">{description}</p>
        </div>
      </div>
      <ArrowUpRightIcon
        aria-hidden="true"
        className="size-4 shrink-0 text-muted-foreground transition-transform group-hover:-translate-y-0.5 group-hover:translate-x-0.5"
      />
    </Link>
  );
}
