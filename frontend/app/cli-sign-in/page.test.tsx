import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const fetchMock = vi.hoisted(() => vi.fn());
const authSourceMock = vi.hoisted(() =>
  vi.fn(async () => ({
    source: "request_cookie",
    cookieName: "pandar_auth_token",
    provider: "betterauth",
  })),
);
const authProviderConfigMock = vi.hoisted(() =>
  vi.fn(() => ({
    provider: "betterauth",
    signInUrl: "https://auth.example/sign-in",
  })),
);

vi.mock("next-intl/server", () => ({
  getTranslations: vi.fn(async () => (key: string) => key),
}));

vi.mock("../actions", () => ({
  createCliTicket: vi.fn(),
}));

vi.mock("../api-auth", () => ({
  apiHeaders: vi.fn(async () => ({ authorization: "Bearer external-jwt" })),
  authSource: authSourceMock,
}));

vi.mock("../auth-provider", () => ({
  authProviderConfig: authProviderConfigMock,
}));

vi.mock("../../components/language-switcher", () => ({
  LanguageSwitcher: () => <div />,
}));

vi.mock("./cli-ticket-form", () => ({
  CliTicketForm: (props: {
    selectedTenant: { display_name: string };
    redirectUrl: string;
    codeChallenge: string;
    state: string;
  }) => (
    <div>
      cli ticket {props.selectedTenant.display_name} {props.redirectUrl}{" "}
      {props.codeChallenge} {props.state}
    </div>
  ),
}));

const callback = {
  redirect_url: "http://127.0.0.1:54321/callback",
  code_challenge: "ZtNPunH49FD35FWYhT5Tv8I7vRKQJ8uxMaL0_9eHjNA",
  state: "cli-opaque-state",
};

function identity(tenants: { id: string; name: string }[]) {
  return new Response(
    JSON.stringify({
      identity: {
        provider: "betterauth",
        subject: "user-1",
        email: "user@example.com",
        email_verified: true,
        display_name: "User",
      },
      tenants: tenants.map((tenant) => ({
        tenant_id: tenant.id,
        tenant_slug: tenant.id,
        display_name: tenant.name,
        role: "viewer",
      })),
      can_self_create_tenant: true,
    }),
    { status: 200 },
  );
}

describe("CliSignInPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    authSourceMock.mockResolvedValue({
      source: "request_cookie",
      cookieName: "pandar_auth_token",
      provider: "betterauth",
    });
    authProviderConfigMock.mockReturnValue({
      provider: "betterauth",
      signInUrl: "https://auth.example/sign-in",
    });
    vi.stubGlobal("fetch", fetchMock);
    fetchMock.mockImplementation(async (input: string | URL | Request) => {
      const url = input.toString();
      if (url.endsWith("/api/v1/me")) {
        return identity([{ id: "tenant-1", name: "Acme Labs" }]);
      }
      throw new Error(`Unexpected fetch: ${url}`);
    });
  });

  it("shows an error state without calling the Hub when redirect_url is missing", async () => {
    const { default: CliSignInPage } = await import("./page");

    render(await CliSignInPage({ searchParams: Promise.resolve({}) }));

    expect(screen.getByText("cliMissingRedirectTitle")).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("directs local no-auth operators to API key login without calling the Hub", async () => {
    authSourceMock.mockResolvedValueOnce({
      source: "none",
      cookieName: "",
      provider: "none",
    });
    authProviderConfigMock.mockReturnValueOnce({
      provider: "none",
      signInUrl: "",
    });
    const { default: CliSignInPage } = await import("./page");

    render(await CliSignInPage({ searchParams: Promise.resolve(callback) }));

    expect(screen.getByText("cliNoAuthTitle")).toBeInTheDocument();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("hands the callback, PKCE challenge, and state to the only tenant's form", async () => {
    const { default: CliSignInPage } = await import("./page");

    render(await CliSignInPage({ searchParams: Promise.resolve(callback) }));

    expect(
      screen.getByText(
        `cli ticket Acme Labs ${callback.redirect_url} ${callback.code_challenge} ${callback.state}`,
      ),
    ).toBeInTheDocument();
  });

  it("renders a form that posts the callback, PKCE challenge, and state", async () => {
    const { CliTicketForm } =
      await vi.importActual<typeof import("./cli-ticket-form")>(
        "./cli-ticket-form",
      );

    const view = render(
      await CliTicketForm({
        action: vi.fn(async () => undefined),
        autoSelectedTenant: true,
        codeChallenge: callback.code_challenge,
        redirectUrl: callback.redirect_url,
        selectedTenant: {
          id: "tenant-1",
          slug: "acme",
          display_name: "Acme Labs",
          created_at: "2026-08-30T00:00:00Z",
        },
        state: callback.state,
      }),
    );

    const field = (name: string) =>
      view.container.querySelector<HTMLInputElement>(`input[name="${name}"]`);
    expect(field("tenant_id")).toHaveValue("tenant-1");
    expect(field("redirect_url")).toHaveValue(callback.redirect_url);
    expect(field("code_challenge")).toHaveValue(callback.code_challenge);
    expect(field("state")).toHaveValue(callback.state);
    expect(screen.getByText(callback.redirect_url)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "cliSignInSubmit" }),
    ).toBeInTheDocument();
  });

  it("preserves the callback while selecting among tenants", async () => {
    fetchMock.mockImplementation(async () =>
      identity([
        { id: "tenant-1", name: "Acme Labs" },
        { id: "tenant-2", name: "Workshop" },
      ]),
    );
    const { default: CliSignInPage } = await import("./page");

    const view = render(
      await CliSignInPage({ searchParams: Promise.resolve(callback) }),
    );

    expect(
      screen.getByRole("option", { name: "Workshop" }),
    ).toBeInTheDocument();
    for (const [name, value] of Object.entries(callback)) {
      expect(
        view.container.querySelector<HTMLInputElement>(`input[name="${name}"]`),
      ).toHaveValue(value);
    }
  });
});
