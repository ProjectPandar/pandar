import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  createTenantToken,
  revokeTenantToken,
  rotateTenantToken,
} from "./admin-actions";
import { refreshPrinters } from "./actions";

const redirectMock = vi.hoisted(() =>
  vi.fn((url: string) => {
    throw new Error(`NEXT_REDIRECT:${url}`);
  }),
);
const refreshMock = vi.hoisted(() => vi.fn());

vi.mock("next/cache", () => ({
  refresh: refreshMock,
}));

vi.mock("next/navigation", () => ({
  redirect: redirectMock,
}));

vi.mock("./api-auth", () => ({
  requireAuth: vi.fn(async () => undefined),
  apiHeaders: vi.fn(async () => ({ "content-type": "application/json" })),
}));

describe("job action redirects", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(JSON.stringify({ id: "command-1" }), {
            status: 200,
            headers: { "content-type": "application/json" },
          }),
      ),
    );
  });

  it("redirects agent refresh back to Agents", async () => {
    const formData = new FormData();
    formData.set("tenant_id", "tenant-1");
    formData.set("agent_id", "agent-1");
    formData.set("return_to", "agents");

    await expect(refreshPrinters(formData)).rejects.toThrow(
      "NEXT_REDIRECT:/agents?status=refresh_queued",
    );
  });
});

describe("revokeTenantToken", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(JSON.stringify({ tenant_token: { id: "token-1" } }), {
            status: 200,
            headers: { "content-type": "application/json" },
          }),
      ),
    );
  });

  it("returns mutation completion after revoking a token", async () => {
    const formData = new FormData();
    formData.set("tenant_id", "tenant-1");
    formData.set("token_id", "token-1");

    await expect(revokeTenantToken(null, formData)).resolves.toEqual({
      ok: true,
    });
    expect(fetch).toHaveBeenCalledWith(
      "http://localhost:8080/api/v1/tenants/tenant-1/tenant-tokens/token-1",
      {
        method: "DELETE",
        headers: { "content-type": "application/json" },
      },
    );
  });
});

describe("createTenantToken", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-17T01:00:00.000Z"));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("posts normalized token fields, computes expiration from preset days, returns the one-time secret, and refreshes token management", async () => {
    const tenantToken = {
      id: "token-created",
      tenant_id: "tenant-1",
      name: "Studio automation",
      scopes: ["plugin:studio", "agent:register"],
      created_by_user_id: null,
      created_at: "2026-07-17T01:00:00Z",
      last_used_at: null,
      expires_at: "2026-08-16T01:00:00.000Z",
      revoked_at: null,
    };
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(
            JSON.stringify({
              tenant_token: tenantToken,
              token: "pandar_tenant_created-secret",
            }),
            {
              status: 201,
              headers: { "content-type": "application/json" },
            },
          ),
      ),
    );
    const formData = new FormData();
    formData.set("tenant_id", "tenant-1");
    formData.set("name", "Studio automation");
    formData.set("scopes", " plugin:studio, agent:register, agent:register, ");
    formData.set("expires_in_days", "30");

    await expect(createTenantToken(null, formData)).resolves.toEqual({
      ok: true,
      kind: "tenant_token",
      operation: "created",
      tenantToken,
      token: "pandar_tenant_created-secret",
    });
    expect(fetch).toHaveBeenCalledWith(
      "http://localhost:8080/api/v1/tenants/tenant-1/tenant-tokens",
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          name: "Studio automation",
          scopes: ["agent:register", "plugin:studio"],
          expires_at: "2026-08-16T01:00:00.000Z",
        }),
      },
    );
    expect(refreshMock).toHaveBeenCalledTimes(1);
  });

  it("sends null expires_at when never expires is selected and rejects unknown presets", async () => {
    const tenantToken = {
      id: "token-never",
      tenant_id: "tenant-1",
      name: "Never expires",
      scopes: ["*"],
      created_by_user_id: null,
      created_at: "2026-07-17T01:00:00Z",
      last_used_at: null,
      expires_at: null,
      revoked_at: null,
    };
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(
            JSON.stringify({
              tenant_token: tenantToken,
              token: "pandar_tenant_never-secret",
            }),
            {
              status: 201,
              headers: { "content-type": "application/json" },
            },
          ),
      ),
    );

    const neverForm = new FormData();
    neverForm.set("tenant_id", "tenant-1");
    neverForm.set("name", "Never expires");
    neverForm.set("scopes", "*");
    neverForm.set("expires_in_days", "never");

    await expect(createTenantToken(null, neverForm)).resolves.toEqual({
      ok: true,
      kind: "tenant_token",
      operation: "created",
      tenantToken,
      token: "pandar_tenant_never-secret",
    });
    expect(fetch).toHaveBeenCalledWith(
      "http://localhost:8080/api/v1/tenants/tenant-1/tenant-tokens",
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          name: "Never expires",
          scopes: ["*"],
          expires_at: null,
        }),
      },
    );

    const badForm = new FormData();
    badForm.set("tenant_id", "tenant-1");
    badForm.set("name", "Bad expiry");
    badForm.set("scopes", "*");
    badForm.set("expires_in_days", "999");

    await expect(createTenantToken(null, badForm)).resolves.toEqual({
      ok: false,
      error: "invalid_expires_at",
    });
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("returns the API error without refreshing token management", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(JSON.stringify({ error: "invalid_scope" }), {
            status: 400,
            headers: { "content-type": "application/json" },
          }),
      ),
    );
    const formData = new FormData();
    formData.set("tenant_id", "tenant-1");
    formData.set("name", "Broken token");
    formData.set("scopes", "unknown:scope");
    formData.set("expires_in_days", "7");

    await expect(createTenantToken(null, formData)).resolves.toEqual({
      ok: false,
      error: "invalid_scope",
    });
    expect(refreshMock).not.toHaveBeenCalled();
  });
});

describe("rotateTenantToken", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-17T02:00:00.000Z"));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("preserves current expiration when keep is selected, returns the one-time secret, and refreshes token management", async () => {
    const tenantToken = {
      id: "token-rotated",
      tenant_id: "tenant-1",
      name: "Studio automation",
      scopes: ["plugin:studio"],
      created_by_user_id: null,
      created_at: "2026-07-17T02:00:00Z",
      last_used_at: null,
      expires_at: "2027-01-01T00:00:00Z",
      revoked_at: null,
    };
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(
            JSON.stringify({
              tenant_token: tenantToken,
              token: "pandar_tenant_rotated-secret",
              rotated_from_token_id: "token-old",
            }),
            {
              status: 201,
              headers: { "content-type": "application/json" },
            },
          ),
      ),
    );
    const formData = new FormData();
    formData.set("tenant_id", "tenant-1");
    formData.set("token_id", "token-old");
    formData.set("expires_in_days", "keep");
    formData.set("current_expires_at", "2027-01-01T00:00:00Z");

    await expect(rotateTenantToken(null, formData)).resolves.toEqual({
      ok: true,
      kind: "tenant_token",
      operation: "rotated",
      tenantToken,
      token: "pandar_tenant_rotated-secret",
    });
    expect(fetch).toHaveBeenCalledWith(
      "http://localhost:8080/api/v1/tenants/tenant-1/tenant-tokens/token-old/rotate",
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ expires_at: "2027-01-01T00:00:00Z" }),
      },
    );
    expect(refreshMock).toHaveBeenCalledTimes(1);
  });

  it("computes new expiration when a preset duration is selected on rotate", async () => {
    const tenantToken = {
      id: "token-rotated-7d",
      tenant_id: "tenant-1",
      name: "Studio automation",
      scopes: ["plugin:studio"],
      created_by_user_id: null,
      created_at: "2026-07-17T02:00:00Z",
      last_used_at: null,
      expires_at: "2026-07-24T02:00:00.000Z",
      revoked_at: null,
    };
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(
            JSON.stringify({
              tenant_token: tenantToken,
              token: "pandar_tenant_rotated-7d",
              rotated_from_token_id: "token-old",
            }),
            {
              status: 201,
              headers: { "content-type": "application/json" },
            },
          ),
      ),
    );
    const formData = new FormData();
    formData.set("tenant_id", "tenant-1");
    formData.set("token_id", "token-old");
    formData.set("expires_in_days", "7");

    await expect(rotateTenantToken(null, formData)).resolves.toEqual({
      ok: true,
      kind: "tenant_token",
      operation: "rotated",
      tenantToken,
      token: "pandar_tenant_rotated-7d",
    });
    expect(fetch).toHaveBeenCalledWith(
      "http://localhost:8080/api/v1/tenants/tenant-1/tenant-tokens/token-old/rotate",
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ expires_at: "2026-07-24T02:00:00.000Z" }),
      },
    );
  });
});
