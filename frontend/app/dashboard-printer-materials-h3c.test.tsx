import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NextIntlClientProvider } from "next-intl";

import { QueryClientTestProvider } from "./query-client.test-utils";
import { describe, expect, it, vi } from "vitest";

import en from "../messages/en.json";
import { PrinterMaterialsPanel } from "./dashboard-printer-materials";
import type { Printer } from "./dashboard-types";
import { printerCompatibility } from "./printer-compatibility.test-utils";

vi.mock("./actions", () => ({ controlPrinter: vi.fn() }));

function renderPanel(printer: Printer) {
  render(
    <QueryClientTestProvider>
      <NextIntlClientProvider locale="en" messages={en}>
        <PrinterMaterialsPanel printer={printer} />
      </NextIntlClientProvider>
    </QueryClientTestProvider>,
  );
}

describe("H3C AMS 3H and per-hotend external inputs", () => {
  it("loads an AMS 3H slot by its Studio rack tray id", async () => {
    const user = userEvent.setup();
    renderPanel(h3cPrinter());

    await user.click(screen.getByRole("button", { name: "AMS 3H slot 9, PLA" }));

    const loadForm = screen.getByRole("button", { name: "Load" }).closest("form");
    expect(loadForm?.querySelector('input[name="ams_id"]')).toHaveValue("32");
    expect(loadForm?.querySelector('input[name="slot_id"]')).toHaveValue("8");
    expect(loadForm?.querySelector('input[name="global_tray_id"]')).toHaveValue("36");
  });
});

function h3cPrinter(): Printer {
  return {
    id: "printer-h3c",
    tenant_id: "tenant-1",
    agent_id: "agent-1",
    serial_number: "32X0000",
    name: "Office H3C",
    model: "N10",
    printer_family: "bambu_lab",
    compatibility: { ...printerCompatibility("unknown"), normalized_model: "H3C", nozzle_layout: "single" },
    status: "idle",
    last_seen_at: "2026-11-11T00:00:00Z",
    created_at: "2026-11-11T00:00:00Z",
    materials: {
      observed_at: "2026-11-11T00:00:00Z",
      active_tray: { kind: "ams", global_tray_id: 34, ams_id: "32", tray_id: "6" },
      external_spools: Array.from({ length: 9 }, (_, index) => ({
        external_id: String(247 + index),
        tray_id: String(index),
        exists: true,
        type: 247 + index === 250 ? "PETG" : null,
      })),
      ams_units: [{
        unit_id: "32",
        unit_kind: "ams_3h",
        trays: Array.from({ length: 9 }, (_, slot) => ({
          tray_id: String(slot),
          type: slot === 7 ? null : "PLA",
          color: "DE4343",
          exists: slot !== 7,
        })),
      }],
    },
  };
}
