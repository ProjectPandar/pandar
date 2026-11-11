import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const root = dirname(dirname(fileURLToPath(import.meta.url)));

async function readSource(path: string): Promise<string> {
  return readFile(join(root, path), "utf8");
}

describe("passkey sign-in UI", () => {
  it("hands the auth token to the dashboard through a POST form, never the URL", async () => {
    const tokenFlow = await readSource("lib/token.ts");

    expect(tokenFlow).toMatch(/form\.method = "POST"/);
    expect(tokenFlow).not.toMatch(/searchParams\.set\("token"/);
  });

  it("returns to the dashboard through a POST navigation after issuer sign-out", async () => {
    const signOutClient = await readSource("app/sign-out/sign-out-client.tsx");
    const env = await readSource("lib/env.ts");

    expect(env).toMatch(/DEFAULT_DASHBOARD_URL}\/auth\/betterauth\/session/);
    expect(signOutClient).not.toMatch(/fetch\(dashboardSignOutUrl/);
    expect(signOutClient).toMatch(/document\.createElement\("form"\)/);
    expect(signOutClient).toMatch(/form\.method = "post"/);
    expect(signOutClient).toMatch(/form\.action = dashboardSignOutUrl/);
    expect(signOutClient).toMatch(/form\.submit\(\)/);
    expect(signOutClient).toMatch(
      /<form action=\{dashboardSignOutUrl\} method="post">/,
    );
  });
});
