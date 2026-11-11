import { NextIntlClientProvider } from "next-intl";

import { QueryClientTestProvider } from "./query-client.test-utils";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import en from "../messages/en.json";
import { AppSidebar } from "../components/app-sidebar";
import { SidebarProvider } from "../components/ui/sidebar";
import {
  agentSettingsHref,
  dashboardRootRedirectTarget,
  dashboardSidebarHref,
} from "./dashboard-shell";
import type { AuthMetadata, Tenant } from "./dashboard-types";

const { pushMock, refreshMock } = vi.hoisted(() => ({
  pushMock: vi.fn(),
  refreshMock: vi.fn(),
}));

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: pushMock, refresh: refreshMock }),
  usePathname: () => window.location.pathname,
  useSearchParams: () => new URLSearchParams(window.location.search),
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    warning: vi.fn(),
    error: vi.fn(),
  },
}));

vi.mock("../components/ui/sidebar", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../components/ui/sidebar")>();
  return {
    ...actual,
    SidebarTrigger: ({ className }: { className?: string }) => (
      <button aria-label="Toggle sidebar" className={className} type="button" />
    ),
  };
});

function renderWithMessages(children: React.ReactNode) {
  return render(
    <QueryClientTestProvider>
      <NextIntlClientProvider locale="en" messages={en}>
        {children}
      </NextIntlClientProvider>
    </QueryClientTestProvider>,
  );
}

const tenants: Tenant[] = [
  {
    id: "t1",
    slug: "tenant-one",
    display_name: "Tenant One",
    created_at: "2026-06-30T00:00:00Z",
  },
  {
    id: "t2",
    slug: "tenant-two",
    display_name: "Tenant Two",
    created_at: "2026-06-30T00:00:00Z",
  },
];

const auth: AuthMetadata = {
  source: "none",
  cookieName: "pandar_auth",
  provider: "none",
  signInUrl: null,
  signOutUrl: null,
};

describe("dashboard shell helpers", () => {
  it("builds dashboard redirect and navigation URLs", () => {
    expect(dashboardRootRedirectTarget({})).toBe("/devices");
    expect(
      dashboardRootRedirectTarget({
        status: "job_created",
      }),
    ).toBe("/devices?status=job_created");
    expect(
      dashboardRootRedirectTarget({
        command: "cmd1",
        status: "refresh_queued",
      }),
    ).toBe("/agents?command=cmd1&status=refresh_queued");

    expect(dashboardSidebarHref("agents")).toBe("/agents");
    expect(dashboardSidebarHref("jobs")).toBe("/jobs");
    expect(dashboardSidebarHref("users")).toBe("/users");
    expect(agentSettingsHref("agent/1")).toBe(
      "/agents/agent%2F1/settings",
    );
    expect(agentSettingsHref("a1")).toBe(
      "/agents/a1/settings",
    );
  });
});

describe("AppSidebar", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    window.history.replaceState({}, "", "/");
    document.cookie = "pandar.tenant=; path=/; max-age=0";
  });

  it("keeps tenant access available inside the mobile sidebar", async () => {
    const user = userEvent.setup();
    vi.stubGlobal("innerWidth", 500);
    window.history.replaceState({}, "", "/devices");
    renderWithMessages(
      <SidebarProvider defaultOpen>
        <AppSidebar
          activeView="devices"
          auth={auth}
          selectedTenant={tenants[0]}
          tenants={tenants}
        />
      </SidebarProvider>,
    );

    fireEvent.keyDown(window, { key: "b", ctrlKey: true });
    const sidebar = await screen.findByRole("dialog", { name: "Dashboard" });
    await user.click(
      within(sidebar).getByRole("button", { name: "Select tenant access" }),
    );

    const menu = screen.getByRole("dialog", { name: "Tenant access" });
    await user.click(within(menu).getByRole("button", { name: "Tenant Two" }));

    expect(document.cookie).toContain("pandar.tenant=t2");
    expect(refreshMock).toHaveBeenCalled();
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Tenant access" })).not.toBeInTheDocument();
      expect(screen.queryByRole("dialog", { name: "Dashboard" })).not.toBeInTheDocument();
    });
  });

  it("switches tenant via cookie and drops transient query context", async () => {
    const user = userEvent.setup();
    window.history.replaceState({}, "", "/agents?command=cmd1&status=done");

    renderWithMessages(
      <SidebarProvider>
        <AppSidebar
          activeView="agents"
          auth={auth}
          selectedTenant={tenants[0]}
          tenants={tenants}
        />
      </SidebarProvider>,
    );

    const trigger = screen.getByRole("button", { name: "Select tenant access" });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    await user.click(trigger);

    const menu = screen.getByRole("dialog", { name: "Tenant access" });
    expect(within(menu).getByText("Tenant access")).toBeVisible();
    expect(screen.getByRole("button", { name: "Tenant One" })).toHaveAttribute(
      "aria-current",
      "true",
    );
    await user.click(screen.getByRole("button", { name: "Tenant Two" }));

    expect(document.cookie).toContain("pandar.tenant=t2");
    expect(pushMock).toHaveBeenCalledWith("/agents");
    await waitFor(() => {
      expect(trigger).toHaveAttribute("aria-expanded", "false");
      expect(screen.queryByRole("dialog", { name: "Tenant access" })).not.toBeInTheDocument();
    });
  });

  it("opens and closes tenant access from the collapsed desktop sidebar", async () => {
    const user = userEvent.setup();
    const { container } = renderWithMessages(
      <SidebarProvider defaultOpen={false}>
        <AppSidebar
          activeView="devices"
          auth={auth}
          selectedTenant={tenants[0]}
          tenants={tenants}
        />
      </SidebarProvider>,
    );

    expect(container.querySelector('[data-slot="sidebar"][data-state]')).toHaveAttribute(
      "data-state",
      "collapsed",
    );
    await user.click(screen.getByRole("button", { name: "Select tenant access" }));
    await user.click(screen.getByRole("button", { name: "Tenant Two" }));

    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "Tenant access" })).not.toBeInTheDocument();
    });
  });
});
