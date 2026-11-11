import { describe, expect, it } from "vitest";

import { reprintRequestBody } from "./dispatch-form-submission";

function formData() {
  const data = new FormData();
  data.set("printer_id", "printer-1");
  data.set("use_ams", "true");
  data.set("material_mapping_valid", "true");
  data.set("material_mapping_uses_ams", "false");
  data.set("external_material_mismatch", "false");
  return data;
}

describe("reprintRequestBody", () => {
  it("explicitly clears inherited mappings when the selected plate has none", () => {
    const data = formData();
    data.set("plate_id", "1");
    data.set("bed_leveling", "true");
    data.set("auto_bed_leveling", "2");
    data.set("flow_cali", "false");
    data.set("auto_flow_cali", "2");
    data.set("auto_offset_cali", "0");
    data.set("timelapse", "false");

    expect(reprintRequestBody(data)).toMatchObject({
      ams_mapping: [],
      ams_mapping2: [],
      ams_mapping_info: [],
    });
  });
});
