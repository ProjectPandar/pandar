export const DASHBOARD_VIEWS = [
  "devices",
  "jobs",
  "agents",
  "users",
  "settings",
] as const;

export type DashboardView = (typeof DASHBOARD_VIEWS)[number];

export const SETTINGS_SECTIONS = [
  "workspace",
  "appearance",
  "access",
  "account",
] as const;

export type SettingsSectionId = (typeof SETTINGS_SECTIONS)[number];

export const SETTINGS_SECTION_META: Record<
  SettingsSectionId,
  {
    titleKey:
      | "workspaceTitle"
      | "appearanceTitle"
      | "accessTitle"
      | "accountTitle";
    descriptionKey:
      | "workspaceDescription"
      | "appearanceDescription"
      | "accessDescription"
      | "accountDescription";
  }
> = {
  workspace: {
    titleKey: "workspaceTitle",
    descriptionKey: "workspaceDescription",
  },
  appearance: {
    titleKey: "appearanceTitle",
    descriptionKey: "appearanceDescription",
  },
  access: {
    titleKey: "accessTitle",
    descriptionKey: "accessDescription",
  },
  account: {
    titleKey: "accountTitle",
    descriptionKey: "accountDescription",
  },
};

export function parseSettingsSection(
  value: string | null | undefined,
): SettingsSectionId {
  if (value && (SETTINGS_SECTIONS as readonly string[]).includes(value)) {
    return value as SettingsSectionId;
  }
  return "workspace";
}

export function settingsSectionHref(section: SettingsSectionId): string {
  return section === "workspace" ? "/settings" : `/settings?section=${section}`;
}

export type DashboardQuery = {
  command?: string;
  status?: string;
};

export function dashboardViewTitleKey(view: DashboardView) {
  return view;
}

export function dashboardRootRedirectTarget(query: DashboardQuery) {
  return dashboardPath(query.command ? "agents" : "devices", query);
}

export function dashboardSidebarHref(view: DashboardView) {
  return `/${view}`;
}

export function agentSettingsHref(agentId: string) {
  return `/agents/${encodeURIComponent(agentId)}/settings`;
}

function dashboardPath(view: DashboardView, query: DashboardQuery = {}) {
  const params = new URLSearchParams();
  if (query.command) params.set("command", query.command);
  if (query.status) params.set("status", query.status);
  const suffix = params.toString();
  return suffix ? `/${view}?${suffix}` : `/${view}`;
}

export function logoutHref({ signOutUrl }: { signOutUrl: string | null }) {
  return signOutUrl;
}
