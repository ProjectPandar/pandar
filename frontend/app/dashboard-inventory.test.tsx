import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { PrinterInventory } from "./dashboard-inventory";
import {
  FilterRouteHarness,
  agent,
  renderWithMessages,
  tenant,
} from "./dashboard-inventory.test.context";

describe("PrinterInventory", () => {
  it("resets its typed filters after navigating away and back", async () => {
    const user = userEvent.setup();
    renderWithMessages(<FilterRouteHarness />);

    await user.type(
      screen.getByRole("searchbox", { name: "Search name or serial" }),
      "Office",
    );
    expect(
      screen.getByRole("article", { name: "Office A1" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("article", { name: "Workshop X1" }),
    ).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Switch route" }));
    await user.click(screen.getByRole("button", { name: "Switch route" }));

    expect(
      screen.getByRole("article", { name: "Office A1" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("article", { name: "Workshop X1" }),
    ).toBeInTheDocument();
  });

  it("opens the machine form from the empty printer state", async () => {
    const user = userEvent.setup();
    renderWithMessages(
      <PrinterInventory selectedTenant={tenant} printers={[]} agents={[agent]} nowMs={0} />,
    );

    expect(screen.getByText("No printers reported")).toBeVisible();

    const trigger = screen.getByRole("button", { name: "Link printer" });
    expect(trigger).toHaveAttribute("data-slot", "dialog-trigger");

    await user.click(trigger);

    const dialog = screen.getByRole("dialog", { name: "Link printer to agent" });
    const form = within(dialog)
      .getByRole("button", { name: "Link printer" })
      .closest("form")!;
    expect(form).toHaveFormValues({
      tenant_id: tenant.id,
      agent_id: agent.id,
      type: "BambuLab",
      host: "",
      name: "",
      access_code: "",
    });

    await user.type(within(dialog).getByLabelText("Access code"), "SECRET-LINK-CODE");
    await user.click(within(dialog).getByRole("button", { name: "Close" }));
    await user.click(trigger);

    expect(screen.getByLabelText("Access code")).toHaveValue("");
  });
});
