import { beforeEach, describe, expect, it, vi } from "vitest";

const apiHeadersMock = vi.hoisted(() =>
  vi.fn(async () => ({ authorization: "Bearer server-secret" })),
);

vi.mock("./api-auth", () => ({
  apiHeaders: apiHeadersMock,
}));

const hubBase = "https://hub.internal.example/base";

async function loadRoutes() {
  vi.resetModules();
  vi.stubEnv("APP_API_URL", hubBase);
  const [reprint, printerJobs, camera, auditEvents] = await Promise.all([
    import("./api/tenants/[tenantId]/jobs/[jobId]/reprint/route"),
    import("./api/tenants/[tenantId]/printers/[printerId]/jobs/route"),
    import("./api/tenants/[tenantId]/printers/[printerId]/camera.mp4/route"),
    import("./api/tenants/[tenantId]/audit-events/route"),
  ]);
  return { reprint, printerJobs, camera, auditEvents };
}

type FetchMock = ReturnType<
  typeof vi.fn<
    (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>
  >
>;

function stubUpstream(): FetchMock {
  const fetchMock = vi.fn<
    (input: RequestInfo | URL, init?: RequestInit) => Promise<Response>
  >(
    async () =>
      new Response("{}", {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
  );
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

function firstCall(fetchMock: FetchMock): [RequestInfo | URL, RequestInit] {
  const call = fetchMock.mock.calls[0];
  if (!call) throw new Error("expected an upstream fetch call");
  return [call[0], call[1] ?? {}];
}

function mutationRequest(url: string, init?: RequestInit) {
  const headers = new Headers(init?.headers);
  headers.set("origin", "https://web.example");
  headers.set("sec-fetch-site", "same-origin");
  return new Request(url, { ...init, headers });
}

function expectUpstreamCall(fetchMock: FetchMock, url: string, method: string) {
  expect(fetchMock).toHaveBeenCalledOnce();
  const [calledUrl, init] = firstCall(fetchMock);
  expect(calledUrl).toBe(url);
  expect(init.method).toBe(method);
}

describe("hub proxy route wiring", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
  });

  it("proxies a job reprint with a forced JSON content-type", async () => {
    const routes = await loadRoutes();
    const fetchMock = stubUpstream();

    await routes.reprint.POST(
      mutationRequest(
        "https://web.example/api/tenants/tenant-1/jobs/job-1/reprint",
        { method: "POST", body: "{}" },
      ),
      { params: Promise.resolve({ tenantId: "tenant-1", jobId: "job-1" }) },
    );

    expectUpstreamCall(
      fetchMock,
      `${hubBase}/api/v1/tenants/tenant-1/jobs/job-1/reprint`,
      "POST",
    );
    const [, init] = firstCall(fetchMock);
    expect(new Headers(init.headers).get("content-type")).toBe(
      "application/json",
    );
  });

  it("proxies a printer job dispatch forwarding the request content-type", async () => {
    const routes = await loadRoutes();
    const fetchMock = stubUpstream();

    await routes.printerJobs.POST(
      mutationRequest(
        "https://web.example/api/tenants/tenant-1/printers/printer-1/jobs",
        {
          method: "POST",
          body: "plate",
          headers: { "content-type": "text/csv" },
        },
      ),
      {
        params: Promise.resolve({
          tenantId: "tenant-1",
          printerId: "printer-1",
        }),
      },
    );

    expectUpstreamCall(
      fetchMock,
      `${hubBase}/api/v1/tenants/tenant-1/printers/printer-1/jobs`,
      "POST",
    );
    const [, init] = firstCall(fetchMock);
    expect(new Headers(init.headers).get("content-type")).toBe("text/csv");
  });

  it("proxies the audit events list with the declared limit", async () => {
    const routes = await loadRoutes();
    const fetchMock = stubUpstream();

    await routes.auditEvents.GET(
      new Request("https://web.example/api/tenants/tenant-1/audit-events"),
      { params: Promise.resolve({ tenantId: "tenant-1" }) },
    );

    expectUpstreamCall(
      fetchMock,
      `${hubBase}/api/v1/tenants/tenant-1/audit-events?limit=20`,
      "GET",
    );
  });

  it("proxies the camera stream", async () => {
    const routes = await loadRoutes();
    const fetchMock = stubUpstream();

    await routes.camera.GET(
      new Request(
        "https://web.example/api/tenants/tenant-1/printers/printer-1/camera.mp4",
      ),
      {
        params: Promise.resolve({
          tenantId: "tenant-1",
          printerId: "printer-1",
        }),
      },
    );

    expectUpstreamCall(
      fetchMock,
      `${hubBase}/api/v1/tenants/tenant-1/printers/printer-1/camera.mp4`,
      "GET",
    );
  });
});
