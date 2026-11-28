import type { ReactNode } from "react";
import { NextIntlClientProvider } from "next-intl";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import en from "../messages/en.json";
import { createJoinLink } from "./admin-actions";
import type { JoinLink, Tenant } from "./dashboard-types";
import { InvitesSection } from "./users-invites";

vi.mock("./admin-actions", () => ({
  createJoinLink: vi.fn(),
  removeTenantUser: vi.fn(),
  revokeJoinLink: vi.fn(),
  updateTenantUserRole: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
    warning: vi.fn(),
  },
}));

const tenant: Tenant = {
  id: "tenant-1",
  slug: "factory",
  display_name: "Factory Floor",
  created_at: "2026-06-01T00:00:00Z",
};

function joinLink(overrides: Partial<JoinLink>): JoinLink {
  return {
    id: "link-1",
    tenant_id: tenant.id,
    role: "viewer",
    email_constraint: null,
    expires_at: new Date(Date.now() + 3 * 24 * 60 * 60 * 1000).toISOString(),
    max_uses: 5,
    used_count: 1,
    created_by_user_id: null,
    revoked_at: null,
    created_at: "2026-06-30T00:00:00Z",
    ...overrides,
  };
}

function renderSection(joinLinks: JoinLink[]) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const content: ReactNode = (
    <InvitesSection joinLinks={joinLinks} tenant={tenant} />
  );
  return render(
    <NextIntlClientProvider locale="en" messages={en}>
      <QueryClientProvider client={queryClient}>{content}</QueryClientProvider>
    </NextIntlClientProvider>,
  );
}

describe("InvitesSection", () => {
  it("hides expired and revoked links while keeping active and exhausted links visible", () => {
    const expired = joinLink({
      id: "link-expired",
      email_constraint: "expired@example.test",
      expires_at: "2020-01-01T00:00:00Z",
    });
    const revoked = joinLink({
      id: "link-revoked",
      email_constraint: "revoked@example.test",
      revoked_at: "2026-06-30T00:00:00Z",
    });
    const view = renderSection([
      expired,
      joinLink({ id: "link-active", email_constraint: "vip@example.test" }),
      revoked,
      joinLink({ id: "link-exhausted", used_count: 5 }),
    ]);

    expect(screen.getByText("Active")).toBeVisible();
    expect(screen.getByText("Used up")).toBeVisible();
    expect(screen.queryByText("expired@example.test")).not.toBeInTheDocument();
    expect(screen.queryByText("revoked@example.test")).not.toBeInTheDocument();
    expect(screen.getByText(/vip@example\.test/)).toBeVisible();
    expect(screen.getByText("1 active · 2 total")).toBeVisible();

    const revokeButtons = screen.getAllByRole("button", { name: /Revoke invite/ });
    expect(revokeButtons).toHaveLength(1);

    view.rerender(
      <NextIntlClientProvider locale="en" messages={en}>
        <QueryClientProvider
          client={
            new QueryClient({
              defaultOptions: { queries: { retry: false } },
            })
          }
        >
          <InvitesSection joinLinks={[expired, revoked]} tenant={tenant} />
        </QueryClientProvider>
      </NextIntlClientProvider>,
    );
    expect(screen.getByText("No invite links")).toBeVisible();
    expect(screen.getByText("0 active · 0 total")).toBeVisible();
  });

  it("keeps the selected role checked after a successful create", async () => {
    const event = userEvent.setup();
    vi.mocked(createJoinLink).mockResolvedValue({
      ok: true,
      kind: "join_link",
      joinLink: joinLink({ role: "tenant_admin" }),
      token: "secret-token",
      message: "Join link created",
    });
    renderSection([]);

    await event.click(screen.getByRole("button", { name: "Create invite" }));
    const adminRadio = await screen.findByRole("radio", {
      name: /Tenant admin/,
    });
    await event.click(adminRadio);
    expect(adminRadio).toBeChecked();

    await event.click(screen.getByRole("button", { name: "Create link" }));

    expect(await screen.findByText("Join link created")).toBeVisible();
    expect(
      screen.getByRole("radio", { name: /Tenant admin/ }),
    ).toBeChecked();
    expect(screen.getByRole("radio", { name: /Viewer/ })).not.toBeChecked();
  });
});
