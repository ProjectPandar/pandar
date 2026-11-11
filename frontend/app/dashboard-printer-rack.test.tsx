import { NextIntlClientProvider } from "next-intl";

import { QueryClientTestProvider } from "./query-client.test-utils";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import en from "../messages/en.json";
import { PrinterRackPanel } from "./dashboard-printer-rack";
import type { Printer } from "./dashboard-types";
import { printerCompatibility } from "./printer-compatibility.test-utils";

vi.mock("./actions", () => ({
  controlPrinter: vi.fn(),
}));

function renderWithMessages(children: React.ReactNode) {
  return render(
    <QueryClientTestProvider>
      <NextIntlClientProvider locale="en" messages={en}>
        {children}
      </NextIntlClientProvider>
    </QueryClientTestProvider>,
  );
}

const printer: Printer = {
  id: "printer-1",
  tenant_id: "tenant-1",
  agent_id: "agent-1",
  serial_number: "H2CSERIAL",
  printer_family: "bambu_lab",
  name: "Lab H2C",
  model: "H2C",
  compatibility: printerCompatibility("unknown"),
  status: "idle",
  last_seen_at: "2026-08-04T00:00:00Z",
  created_at: "2026-08-04T00:00:00Z",
  materials: null,
};

const rackPrinter: Printer = {
  ...printer,
  nozzle_system: {
    nozzle: {
      exist: 1,
      state: 0,
      src_id: 16,
      tar_id: 18,
      info: [
        { id: 0, diameter: 0.4000000059604645, type: "HS01", wear: 0.12 },
        { id: 1, diameter: 0.6, type: "XH05" },
        { id: 16, diameter: 0.4, type: "XS01", wear: 0.5 },
        { id: 17, diameter: 0.6, type: "HH05" },
        { id: 20, diameter: 0.2, type: "AB99" },
      ],
    },
    holder: { stat: 0, pos: 1, info: 1 },
  },
};

describe("PrinterRackPanel", () => {
  it("wires rack move and global operation forms", () => {
    renderWithMessages(<PrinterRackPanel printer={rackPrinter} />);

    const centreForm = screen.getByRole("button", { name: "Centre" }).closest("form");
    expect(centreForm?.querySelector('input[name="action"]')).toHaveValue("nozzle_holder_ctrl");
    expect(centreForm?.querySelector('input[name="holder_action"]')).toHaveValue("0");
    const moveAForm = screen.getByRole("button", { name: "A top" }).closest("form");
    expect(moveAForm?.querySelector('input[name="holder_action"]')).toHaveValue("1");
    const moveBForm = screen.getByRole("button", { name: "B top" }).closest("form");
    expect(moveBForm?.querySelector('input[name="holder_action"]')).toHaveValue("2");

    const rereadAllForm = screen.getByRole("button", { name: "Re-read all" }).closest("form");
    expect(rereadAllForm?.querySelector('input[name="action"]')).toHaveValue("holder_nozzle_refresh");
    expect(rereadAllForm?.querySelector('input[name="nozzle_id"]')).toHaveValue("255");
    const confirmAllForm = screen.getByRole("button", { name: "Confirm all" }).closest("form");
    expect(confirmAllForm?.querySelector('input[name="action"]')).toHaveValue("nozzle_info_confirm");
    expect(confirmAllForm?.querySelector('input[name="nozzle_id"]')).toHaveValue("255");
  });

  it("offers per-slot re-read and confirm forms for occupied slots", async () => {
    const user = userEvent.setup();
    renderWithMessages(<PrinterRackPanel printer={rackPrinter} />);

    await user.click(screen.getByRole("button", { name: "Rack slot 2, 0.6 mm Tungsten Carbide" }));

    expect(await screen.findByText("High Flow")).toBeVisible();
    const rereadForm = (await screen.findByRole("button", { name: "Re-read hotend" })).closest("form");
    expect(rereadForm?.querySelector('input[name="action"]')).toHaveValue("holder_nozzle_refresh");
    expect(rereadForm?.querySelector('input[name="nozzle_id"]')).toHaveValue("17");
    const confirmForm = screen.getByRole("button", { name: "Confirm hotend" }).closest("form");
    expect(confirmForm?.querySelector('input[name="action"]')).toHaveValue("nozzle_info_confirm");
    expect(confirmForm?.querySelector('input[name="nozzle_id"]')).toHaveValue("17");
  });

  it("disables rack operations while a print is running", () => {
    const printing: Printer = {
      ...rackPrinter,
      status: "RUNNING",
      print: {
        task_generation: 1,
        error_generation: 0,
        hms: [],
        job_state: null,
        gcode_state: "RUNNING",
        task_id: null,
        subtask_id: null,
        subtask_name: null,
        gcode_file: null,
        progress_percent: null,
        speed_level: null,
        remaining_time_minutes: null,
        current_layer: null,
        total_layers: null,
        print_error: null,
        printer_job_id: null,
      },
    };
    renderWithMessages(<PrinterRackPanel printer={printing} />);

    expect(screen.getByRole("button", { name: "Centre" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Re-read all" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Confirm all" })).toBeDisabled();
  });
});

describe("PrinterRackPanel on the H3C", () => {
  const h3cPrinter: Printer = {
    ...printer,
    serial_number: "32X0000",
    name: "Lab H3C",
    model: "N10",
    compatibility: { ...printerCompatibility("unknown"), normalized_model: "H3C", nozzle_layout: "single" },
    nozzle_system: {
      nozzle: {
        info: [
          { id: 0, addr_id: 6, diameter: 0.4, type: "HS01" },
          { id: 16, addr_id: 0, diameter: 0.4, type: "HS01" },
          { id: 24, addr_id: 8, diameter: 0.6, type: "HS01" },
        ],
      },
      holder: { stat: 0, pos: 1, info: 1 },
    },
  };

  it("moves the direct rack up, to the centre and down", () => {
    renderWithMessages(<PrinterRackPanel printer={h3cPrinter} />);

    const holderAction = (name: string) =>
      screen.getByRole("button", { name }).closest("form")?.querySelector('input[name="holder_action"]');
    expect(holderAction("Centre")).toHaveValue("0");
    expect(holderAction("Raise")).toHaveValue("1");
    expect(holderAction("Lower")).toHaveValue("2");
    expect(screen.queryByRole("button", { name: "A top" })).toBeNull();
  });

  it("addresses the mounted hotend by its rack position", async () => {
    const user = userEvent.setup();
    renderWithMessages(<PrinterRackPanel printer={h3cPrinter} />);

    await user.click(screen.getByRole("button", { name: "Rack slot 7, 0.4 mm Hardened Steel, Mounted" }));

    const rereadForm = (await screen.findByRole("button", { name: "Re-read hotend" })).closest("form");
    expect(rereadForm?.querySelector('input[name="nozzle_id"]')).toHaveValue("22");
  });
});
