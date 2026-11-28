import { act } from "react";
import { NextIntlClientProvider } from "next-intl";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import en from "../messages/en.json";
import type { MutationActionState, SecretActionState } from "./action-state";
import { TenantTokensTable } from "./admin-settings-token-list";
import type { Tenant, TenantToken } from "./dashboard-types";

const actionMocks = vi.hoisted(() => ({
  createAgentPairing: vi.fn(),
  createTenantToken: vi.fn(),
  revokeTenantToken: vi.fn(),
  rotateTenantToken: vi.fn(),
}));

vi.mock("./admin-actions", () => actionMocks);

const tenant: Tenant = {
  id: "tenant-1",
  slug: "factory",
  display_name: "Factory",
  created_at: "2026-07-01T00:00:00Z",
};

const nowMs = Date.parse("2026-07-17T00:00:00Z");
const activeToken: TenantToken = {
  id: "token-old",
  tenant_id: tenant.id,
  name: "Studio token",
  scopes: ["plugin:studio"],
  created_by_user_id: null,
  created_at: "2026-07-16T00:00:00Z",
  last_used_at: null,
  expires_at: "2027-01-01T00:00:00Z",
  revoked_at: null,
};
const rotatedToken: TenantToken = {
  ...activeToken,
  id: "token-new",
  created_at: "2026-07-17T00:00:00Z",
};

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, reject, resolve };
}

function TokenPanel({
  tokens,
  queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  }),
}: {
  tokens: TenantToken[];
  queryClient?: QueryClient;
}) {
  return (
    <QueryClientProvider client={queryClient}>
      <NextIntlClientProvider locale="en" messages={en}>
        <TenantTokensTable tenantId={tenant.id} tokens={tokens} nowMs={nowMs} />
      </NextIntlClientProvider>
    </QueryClientProvider>
  );
}

describe("tenant token dialogs", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("hides expired and revoked tokens from the UI and shows empty state when none are active", () => {
    const expiredToken: TenantToken = {
      ...activeToken,
      id: "token-expired",
      name: "Expired token",
      expires_at: "2026-07-16T23:59:59Z",
      revoked_at: null,
    };
    const revokedToken: TenantToken = {
      ...activeToken,
      id: "token-revoked",
      name: "Revoked token",
      expires_at: null,
      revoked_at: "2026-07-16T12:00:00Z",
    };

    const view = render(
      <TokenPanel tokens={[activeToken, expiredToken, revokedToken]} />,
    );

    expect(screen.getByText("Studio token")).toBeVisible();
    expect(screen.queryByText("Expired token")).not.toBeInTheDocument();
    expect(screen.queryByText("Revoked token")).not.toBeInTheDocument();
    expect(screen.getByText(/1 active/)).toBeVisible();

    view.rerender(<TokenPanel tokens={[expiredToken, revokedToken]} />);

    expect(screen.getByText("No tenant tokens")).toBeVisible();
    expect(screen.getByText(/0 active/)).toBeVisible();
  });

  it("locks creation, uses expiration presets dropdown, invalidates token siblings, and clears the one-time secret after closing", async () => {
    const user = userEvent.setup();
    const request = deferred<SecretActionState>();
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    actionMocks.createTenantToken.mockImplementation(() => request.promise);
    render(<TokenPanel queryClient={queryClient} tokens={[activeToken]} />);

    await user.click(screen.getByRole("button", { name: "Create token" }));
    const dialog = screen.getByRole("dialog", {
      name: "Create tenant token",
    });
    const expirationSelect = within(dialog).getByLabelText("Expiration");
    expect(expirationSelect.tagName).toBe("SELECT");
    expect(
      Array.from(expirationSelect.querySelectorAll("option")).map((option) => ({
        value: option.value,
        label: option.textContent,
      })),
    ).toEqual([
      { value: "7", label: "7 days" },
      { value: "30", label: "30 days" },
      { value: "365", label: "365 days" },
      { value: "never", label: "Never expires" },
    ]);
    expect(expirationSelect).toHaveValue("30");

    await user.type(within(dialog).getByLabelText("Name"), "CI token");
    await user.selectOptions(expirationSelect, "365");
    await user.click(
      within(dialog).getByRole("button", { name: "Create token" }),
    );

    await waitFor(() => {
      expect(actionMocks.createTenantToken).toHaveBeenCalledTimes(1);
      expect(
        within(dialog).getByRole("button", { name: "Creating..." }),
      ).toBeDisabled();
    });
    const submittedFormData = actionMocks.createTenantToken.mock
      .calls[0]?.[1] as FormData;
    expect(submittedFormData.get("expires_in_days")).toBe("365");
    expect(
      within(dialog).getByRole("button", { name: "Cancel" }),
    ).toBeDisabled();
    expect(
      within(dialog).queryByRole("button", { name: "Close" }),
    ).not.toBeInTheDocument();

    await user.keyboard("{Escape}");
    expect(dialog).toBeVisible();

    const createdToken = { ...rotatedToken, id: "token-created" };
    await act(async () => {
      request.resolve({
        ok: true,
        kind: "tenant_token",
        operation: "created",
        tenantToken: createdToken,
        token: "pandar_tenant_create-once",
      });
      await request.promise;
    });
    expect(screen.getByText("pandar_tenant_create-once")).toBeVisible();
    await waitFor(() => expect(invalidate).toHaveBeenCalledTimes(2));
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["tenant", tenant.id, "tenant-tokens"],
    });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["tenant", tenant.id, "audit-events"],
    });
    expect(
      screen.getByText("pandar_tenant_create-once").closest('[data-motion="secret-result"]'),
    ).toBeVisible();
    expect(within(dialog).getByLabelText("Name")).toBeDisabled();
    expect(within(dialog).getByLabelText("Scopes")).toBeDisabled();
    expect(within(dialog).getByLabelText("Expiration")).toBeDisabled();
    expect(
      within(dialog).queryByRole("button", { name: "Create token" }),
    ).not.toBeInTheDocument();
    expect(
      within(dialog).queryByRole("button", { name: "Cancel" }),
    ).not.toBeInTheDocument();
    expect(dialog.querySelector('button[type="submit"]')).not.toBeInTheDocument();

    await user.keyboard("{Enter}");
    fireEvent.submit(dialog.querySelector("form")!);
    expect(actionMocks.createTenantToken).toHaveBeenCalledTimes(1);

    const createFooter = dialog.querySelector(
      '[data-slot="dialog-footer"]',
    ) as HTMLElement;
    expect(
      within(createFooter).getByRole("button", { name: "Close" }),
    ).toBeVisible();
    await user.click(
      within(createFooter).getByRole("button", { name: "Close" }),
    );
    await waitFor(() => expect(dialog).not.toBeInTheDocument());
    await user.click(screen.getByRole("button", { name: "Create token" }));

    expect(
      screen.getByRole("dialog", { name: "Create tenant token" }),
    ).toBeVisible();
    expect(
      screen.queryByText("pandar_tenant_create-once"),
    ).not.toBeInTheDocument();
  });

  it("invalidates token siblings and keeps the rotated secret visible when refreshed props revoke and hide the old token", async () => {
    const user = userEvent.setup();
    const request = deferred<SecretActionState>();
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    actionMocks.rotateTenantToken.mockImplementation(() => request.promise);
    const view = render(
      <TokenPanel queryClient={queryClient} tokens={[activeToken]} />,
    );
    const row = screen.getByText("Studio token").closest("article")!;

    await user.click(within(row).getByRole("button", { name: "Rotate token Studio token" }));
    const dialog = screen.getByRole("dialog", { name: "Rotate tenant token" });
    const expirationSelect = within(dialog).getByLabelText("Expiration");
    expect(expirationSelect.tagName).toBe("SELECT");
    expect(expirationSelect).toHaveValue("keep");
    await user.click(
      within(dialog).getByRole("button", { name: "Rotate token" }),
    );

    await waitFor(() => {
      expect(actionMocks.rotateTenantToken).toHaveBeenCalledTimes(1);
      expect(
        within(dialog).getByRole("button", { name: "Rotating..." }),
      ).toBeDisabled();
    });
    expect(
      within(dialog).getByRole("button", { name: "Cancel" }),
    ).toBeDisabled();
    expect(
      within(dialog).queryByRole("button", { name: "Close" }),
    ).not.toBeInTheDocument();

    await user.keyboard("{Escape}");
    expect(dialog).toBeVisible();

    await act(async () => {
      request.resolve({
        ok: true,
        kind: "tenant_token",
        operation: "rotated",
        tenantToken: rotatedToken,
        token: "pandar_tenant_rotate-once",
      });
      await request.promise;
    });
    expect(screen.getByText("pandar_tenant_rotate-once")).toBeVisible();
    await waitFor(() => expect(invalidate).toHaveBeenCalledTimes(2));
    expect(
      screen.getByText("pandar_tenant_rotate-once").closest('[data-motion="secret-result"]'),
    ).toBeVisible();
    expect(within(dialog).getByLabelText("Expiration")).toBeDisabled();
    expect(
      within(dialog).queryByRole("button", { name: "Rotate token" }),
    ).not.toBeInTheDocument();
    expect(
      within(dialog).queryByRole("button", { name: "Cancel" }),
    ).not.toBeInTheDocument();
    expect(dialog.querySelector('button[type="submit"]')).not.toBeInTheDocument();
    const rotateFooter = dialog.querySelector(
      '[data-slot="dialog-footer"]',
    ) as HTMLElement;
    expect(
      within(rotateFooter).getByRole("button", { name: "Close" }),
    ).toBeVisible();

    await user.keyboard("{Enter}");
    fireEvent.submit(dialog.querySelector("form")!);
    expect(actionMocks.rotateTenantToken).toHaveBeenCalledTimes(1);

    view.rerender(
      <TokenPanel
        queryClient={queryClient}
        tokens={[
          rotatedToken,
          { ...activeToken, revoked_at: "2026-07-17T00:00:00Z" },
        ]}
      />,
    );

    expect(screen.getByText("pandar_tenant_rotate-once")).toBeVisible();
    expect(
      screen.getByRole("dialog", { name: "Rotate tenant token" }),
    ).toBeVisible();
    expect(screen.getByTitle("token-new").closest("article")).toHaveAttribute(
      "data-token-status",
      "active",
    );
    expect(screen.queryByTitle("token-old")).not.toBeInTheDocument();
  });

  it("locks revoke confirmation and invalidates token siblings on completion", async () => {
    const user = userEvent.setup();
    const request = deferred<MutationActionState>();
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    actionMocks.revokeTenantToken.mockImplementation(() => request.promise);
    render(<TokenPanel queryClient={queryClient} tokens={[activeToken]} />);
    const row = screen.getByText("Studio token").closest("article")!;

    await user.click(within(row).getByRole("button", { name: "Revoke token Studio token" }));
    const dialog = screen.getByRole("dialog", { name: "Revoke tenant token" });
    await user.click(
      within(dialog).getByRole("button", { name: "Revoke token" }),
    );

    await waitFor(() => {
      expect(actionMocks.revokeTenantToken).toHaveBeenCalledTimes(1);
      expect(
        within(dialog).getByRole("button", { name: "Revoking..." }),
      ).toBeDisabled();
    });
    expect(
      within(dialog).getByRole("button", { name: "Cancel" }),
    ).toBeDisabled();
    expect(
      within(dialog).queryByRole("button", { name: "Close" }),
    ).not.toBeInTheDocument();

    await user.keyboard("{Escape}");
    expect(dialog).toBeVisible();

    await act(async () => {
      request.resolve({ ok: true });
      await request.promise;
    });
    await waitFor(() => expect(dialog).not.toBeInTheDocument());
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["tenant", tenant.id, "tenant-tokens"],
    });
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: ["tenant", tenant.id, "audit-events"],
    });
  });

  it("renders friendly labels for operator, viewer, and session scopes and submits chosen preset scope", async () => {
    const user = userEvent.setup();
    actionMocks.createTenantToken.mockResolvedValue({
      ok: true,
      kind: "tenant_token",
      operation: "created",
      tenantToken: { ...rotatedToken, id: "token-op", scopes: ["operator"] },
      token: "pandar_tenant_op_secret",
    });
    render(
      <TokenPanel
        tokens={[
          { ...activeToken, id: "t-op", name: "Op key", scopes: ["operator"] },
          { ...activeToken, id: "t-view", name: "Viewer key", scopes: ["viewer"] },
          { ...activeToken, id: "t-cli", name: "Pandar CLI", scopes: ["cli:session"] },
        ]}
      />,
    );

    expect(screen.getByText("Operator")).toBeVisible();
    expect(screen.getByText("Viewer")).toBeVisible();
    expect(screen.getByText("CLI session")).toBeVisible();

    await user.click(screen.getByRole("button", { name: "Create token" }));
    const dialog = screen.getByRole("dialog", { name: "Create tenant token" });
    await user.type(within(dialog).getByLabelText("Name"), "Automation key");
    await user.click(within(dialog).getByRole("button", { name: "Operator" }));
    expect(within(dialog).getByLabelText("Scopes")).toHaveValue("operator");
    await user.click(
      within(dialog).getByRole("button", { name: "Create token" }),
    );

    await waitFor(() => {
      expect(actionMocks.createTenantToken).toHaveBeenCalledTimes(1);
    });
    const submittedFormData = actionMocks.createTenantToken.mock
      .calls[0][1] as FormData;
    expect(submittedFormData.get("scopes")).toBe("operator");
  });
});
