import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { PrinterInventory } from "./dashboard-inventory";
import {
  agent,
  printerWithMaterials,
  renderWithMessages,
  tenant,
} from "./dashboard-inventory.test.context";

describe("PrinterInventory", () => {
  it("opens an AMS slot popover on click with RFID, load, and unload operations", async () => {
    const user = userEvent.setup();
    renderWithMessages(
      <PrinterInventory selectedTenant={tenant} printers={[printerWithMaterials]} agents={[agent]} nowMs={0} />,
    );

    await user.click(
      screen.getByRole("button", { name: "AMS-A slot 2, PETG, Active, Remaining: 42%" }),
    );

    expect(screen.getByText("#FFA726")).toBeVisible();
    expect(screen.getByText("42%")).toBeVisible();
    expect(screen.getByRole("button", { name: "Re-read RFID" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Load" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Unload" })).toBeVisible();

    const rereadForm = screen.getByRole("button", { name: "Re-read RFID" }).closest("form");
    const loadForm = screen.getByRole("button", { name: "Load" }).closest("form");
    expect(rereadForm?.querySelector('input[name="global_tray_id"]')).toBeNull();
    expect(loadForm?.querySelector('input[name="global_tray_id"]')).toHaveValue("1");
    expect(loadForm?.querySelector('input[name="extruder_id"]')).toHaveValue("0");
  });
});
