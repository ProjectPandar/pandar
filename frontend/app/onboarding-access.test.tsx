import { NextIntlClientProvider } from "next-intl";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import en from "../messages/en.json";
import { JoinTokenForm } from "./join/token-form";

function renderWithMessages(children: React.ReactNode) {
  return render(
    <NextIntlClientProvider locale="en" messages={en}>
      {children}
    </NextIntlClientProvider>,
  );
}

describe("tenant onboarding access", () => {
  it("pre-fills join token input from the URL hash", () => {
    window.location.hash = "invite-token-123";

    renderWithMessages(<JoinTokenForm action={vi.fn()} />);

    expect(screen.getByLabelText("Join token")).toHaveValue(
      "invite-token-123",
    );
    expect(screen.getByRole("button", { name: /join tenant/i })).toHaveAttribute(
      "type",
      "submit",
    );
  });
});
