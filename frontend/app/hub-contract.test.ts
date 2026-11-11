import { describe, expect, it } from "vitest";

import {
  decodeHubPayload,
  decodeHubResponse,
  HubContractError,
} from "./hub-contract";

describe("decodeHubResponse", () => {
  it("returns a typed response and permits additive fields", () => {
    const decoded = decodeHubResponse("AgentList", {
      agents: [
        {
          id: "agent-1",
          tenant_id: "tenant-1",
          name: "Shop",
          status: "online",
          created_at: "created",
          future_field: true,
        },
      ],
      future_envelope: "kept",
    });

    expect(decoded.agents[0].id).toBe("agent-1");
  });

  it("rejects malformed known fields with their wire path", () => {
    expect(() =>
      decodeHubResponse("Command", {
        id: "command-1",
        tenant_id: "tenant-1",
        agent_id: "agent-1",
        printer_id: "printer-1",
        kind: "printer_operation",
        status: "completed",
        payload_json: "{}",
        error: null,
        result_json: null,
        created_at: "created",
        updated_at: "updated",
      }),
    ).toThrowError(
      new HubContractError(
        "$.status",
        "queued | sent | acknowledged | succeeded | failed | cancelled",
      ),
    );
  });

  it("matches the closed printer-control request boundary", () => {
    expect(
      decodeHubPayload("PrinterControlRequest", {
        action: "handle_print_error",
        error_action: "resume",
        error_generation: 9,
      }),
    ).toEqual({
      action: "handle_print_error",
      error_action: "resume",
      error_generation: 9,
    });
    expect(
      decodeHubPayload("PrinterControlRequest", {
        action: "home",
        axes: [],
        required_device_features: ["bambu_mqtt_homing"],
      }),
    ).toEqual({
      action: "home",
      axes: [],
      required_device_features: ["bambu_mqtt_homing"],
    });
    expect(() =>
      decodeHubPayload("PrinterControlRequest", {
        action: "pause",
        unexpected: true,
      }),
    ).toThrow("Invalid Hub payload at $.unexpected: expected declared field");
  });

  it("rejects missing required fields instead of casting response json", () => {
    expect(() => decodeHubResponse("PrinterList", {})).toThrow(
      "Invalid Hub payload at $.printers: expected present field",
    );
  });
});
