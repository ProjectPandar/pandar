import { NextIntlClientProvider } from "next-intl";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import en from "../messages/en.json";
import { SettingsDashboard } from "./settings-dashboard";
import type { AuthMetadata, Tenant, TenantToken } from "./dashboard-types";

vi.mock("next/navigation", () => ({
  useRouter: () => ({ refresh: vi.fn() }),
}));

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
}

function renderWithMessages(children: React.ReactNode) {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <NextIntlClientProvider locale="en" messages={en}>
        {children}
      </NextIntlClientProvider>
    </QueryClientProvider>,
  );
}

const auth: AuthMetadata = {
  source: "none",
  cookieName: "pandar_auth",
  provider: "none",
  signInUrl: null,
  signOutUrl: null,
};

const tenant: Tenant = {
  id: "t1",
  slug: "tenant-one",
  display_name: "Tenant One",
  created_at: "2026-06-30T00:00:00Z",
};

const nowMs = Date.parse("2026-07-17T00:00:00Z");

const tenantTokens: TenantToken[] = [
  {
    id: "future-token",
    tenant_id: tenant.id,
    name: "Future token",
    scopes: ["plugin:studio"],
    created_by_user_id: null,
    created_at: "2026-07-16T23:00:00Z",
    last_used_at: "2026-07-16T23:50:00Z",
    expires_at: "2026-07-17T00:05:00Z",
    revoked_at: null,
  },
  {
    id: "past-token",
    tenant_id: tenant.id,
    name: "Past token",
    scopes: ["*"],
    created_by_user_id: null,
    created_at: "2026-07-16T21:00:00Z",
    last_used_at: null,
    expires_at: "2026-07-16T23:55:00Z",
    revoked_at: null,
  },
];

describe("Dashboard views", () => {
  const settingsProps = {
    activeSection: "access" as const,
    auth,
    selectedTenant: tenant,
    membershipRole: "tenant_admin",
    canAdmin: true,
    agents: [],
    printers: [],
    tenantTokens: [],
    auditEvents: [],
    adminUnavailable: false,
    adminLoadError: false,
    adminLoading: false,
    nowMs: 0,
  };

  it("defaults token rotation to keeping current expiration for expiring tokens and never for non-expiring tokens", async () => {
    const user = userEvent.setup();
    const neverExpiring: TenantToken = {
      ...tenantTokens[0],
      id: "never-token",
      name: "Never token",
      expires_at: null,
    };
    renderWithMessages(
      <SettingsDashboard
        {...settingsProps}
        nowMs={nowMs}
        tenantTokens={[tenantTokens[0], neverExpiring]}
      />,
    );

    const expiringRow = screen.getByText("Future token").closest("article")!;
    await user.click(
      within(expiringRow).getByRole("button", {
        name: "Rotate token Future token",
      }),
    );

    const dialog = screen.getByRole("dialog", { name: "Rotate tenant token" });
    expect(within(dialog).getByLabelText("Expiration")).toHaveValue("keep");
    expect(
      dialog.querySelector<HTMLInputElement>('input[name="current_expires_at"]'),
    ).toHaveValue("2026-07-17T00:05:00Z");

    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));

    const neverRow = screen.getByText("Never token").closest("article")!;
    await user.click(
      within(neverRow).getByRole("button", {
        name: "Rotate token Never token",
      }),
    );

    const neverDialog = screen.getByRole("dialog", {
      name: "Rotate tenant token",
    });
    const neverSelect = within(neverDialog).getByLabelText("Expiration");
    expect(neverSelect).toHaveValue("never");
    expect(
      Array.from(neverSelect.querySelectorAll("option")).map((o) => o.value),
    ).toEqual(["7", "30", "365", "never"]);
  });

  it("hides expired tokens (including at exact expiration instant) and revoked tokens from Access & security", () => {
    const exactExpiration: TenantToken = {
      ...tenantTokens[0],
      id: "exact-expiration-token",
      name: "Exact expiration token",
      expires_at: new Date(nowMs).toISOString(),
    };
    const expiredAndRevoked: TenantToken = {
      ...tenantTokens[1],
      id: "expired-and-revoked-token",
      name: "Expired and revoked token",
      revoked_at: "2026-07-16T23:59:00Z",
    };

    renderWithMessages(
      <SettingsDashboard
        {...settingsProps}
        nowMs={nowMs}
        tenantTokens={[
          tenantTokens[0],
          tenantTokens[1],
          exactExpiration,
          expiredAndRevoked,
        ]}
      />,
    );

    expect(screen.getByText("Future token")).toBeVisible();
    expect(screen.queryByText("Past token")).not.toBeInTheDocument();
    expect(
      screen.queryByText("Exact expiration token"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("Expired and revoked token"),
    ).not.toBeInTheDocument();
  });
});
