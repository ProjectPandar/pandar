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

  it("preserves the current expiration for an explicit token rotation", async () => {
    const user = userEvent.setup();
    renderWithMessages(
      <SettingsDashboard
        {...settingsProps}
        nowMs={nowMs}
        tenantTokens={[tenantTokens[0]]}
      />,
    );

    const row = screen.getByText("Future token").closest("article")!;
    await user.click(within(row).getByRole("button", { name: "Rotate token Future token" }));

    const dialog = screen.getByRole("dialog", { name: "Rotate tenant token" });
    expect(within(dialog).getByLabelText("Expires at")).toHaveValue(
      "2026-07-17T00:05:00Z",
    );
    expect(within(dialog).getByText(/keeps this expiration/)).toBeVisible();
  });

  it("clears an expired token expiration by default when rotating", async () => {
    const user = userEvent.setup();
    renderWithMessages(
      <SettingsDashboard
        {...settingsProps}
        nowMs={nowMs}
        tenantTokens={[tenantTokens[1]]}
      />,
    );

    const row = screen.getByText("Past token").closest("article")!;
    expect(within(row).getByRole("button", { name: "Revoke token Past token" })).toBeVisible();

    await user.click(within(row).getByRole("button", { name: "Rotate token Past token" }));

    const dialog = screen.getByRole("dialog", { name: "Rotate tenant token" });
    expect(within(dialog).getByLabelText("Expires at")).toHaveValue("");
    expect(
      within(dialog).getByText(/expired.*new future expiration/i),
    ).toBeVisible();
  });

  it("treats the exact expiration instant as expired and gives revoked precedence", () => {
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
        tenantTokens={[exactExpiration, expiredAndRevoked]}
      />,
    );

    const exactExpirationRow = screen
      .getByText("Exact expiration token")
      .closest("article")!;
    const expiredAndRevokedRow = screen
      .getByText("Expired and revoked token")
      .closest("article")!;
    expect(exactExpirationRow).toHaveAttribute("data-token-status", "expired");
    expect(within(exactExpirationRow).getByText("Expired")).toBeVisible();
    expect(expiredAndRevokedRow).toHaveAttribute(
      "data-token-status",
      "revoked",
    );
    expect(within(expiredAndRevokedRow).getByText("Revoked")).toBeVisible();
  });
});
