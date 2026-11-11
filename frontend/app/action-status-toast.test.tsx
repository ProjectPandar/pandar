import { StrictMode } from "react";
import { NextIntlClientProvider } from "next-intl";
import { render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import en from "../messages/en.json";
import { ActionStatusToast } from "./action-status-toast";
import { toast } from "sonner";

const { pushMock, refreshMock } = vi.hoisted(() => ({
  pushMock: vi.fn(),
  refreshMock: vi.fn(),
}));

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: pushMock, refresh: refreshMock }),
  usePathname: () => window.location.pathname,
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    warning: vi.fn(),
    error: vi.fn(),
  },
}));

function renderWithMessages(children: React.ReactNode) {
  return render(
    <NextIntlClientProvider locale="en" messages={en}>
      {children}
    </NextIntlClientProvider>,
  );
}

function setUrl(path: string) {
  window.history.pushState({}, "", path);
}

afterEach(() => {
  vi.clearAllMocks();
  vi.unstubAllGlobals();
  window.history.replaceState({}, "", "/");
  document.cookie = "pandar.tenant=; path=/; max-age=0";
});

describe("ActionStatusToast", () => {
  it("shows a success toast and clears only the status query parameter", async () => {
    setUrl("/devices?tenant=t1&status=refresh_queued");

    renderWithMessages(<ActionStatusToast status="refresh_queued" />);

    await waitFor(() => expect(toast.success).toHaveBeenCalledWith("Refresh queued"));
    expect(window.location.pathname + window.location.search).toBe("/devices?tenant=t1");
  });

  it("shows a warning toast and preserves tenant plus command query parameters", async () => {
    setUrl("/devices?tenant=t1&command=c1&status=refresh_partial");

    renderWithMessages(<ActionStatusToast status="refresh_partial" />);

    await waitFor(() =>
      expect(toast.warning).toHaveBeenCalledWith(
        "Some refreshes could not be queued — review the list",
      ),
    );
    expect(window.location.pathname + window.location.search).toBe("/devices?tenant=t1&command=c1");
  });

  it("shows an error toast for unexpected backend error codes", async () => {
    setUrl("/devices?tenant=t1&status=artifact_too_large");

    renderWithMessages(<ActionStatusToast status="artifact_too_large" />);

    await waitFor(() => expect(toast.error).toHaveBeenCalledWith("Artifact Too Large"));
  });

  it("does not duplicate toasts under Strict Mode effect replay", async () => {
    setUrl("/devices?tenant=t1&status=refresh_queued");

    renderWithMessages(
      <StrictMode>
        <ActionStatusToast status="refresh_queued" />
      </StrictMode>,
    );

    await waitFor(() => expect(toast.success).toHaveBeenCalledTimes(1));
  });
});
