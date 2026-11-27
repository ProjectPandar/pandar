import type { useTranslations } from "next-intl";

import type { Printer } from "./dashboard-types";

type NozzleTemperature = NonNullable<Printer["nozzle_temperatures"]>[number];

/** A nozzle with a reading, labelled by the printer (Bambu `L`/`R`, Klipper `T<n>`) or by its position. */
export type PresentedNozzle = Omit<NozzleTemperature, "label"> & {
  label: string;
};

export function presentNozzles(
  nozzles: NonNullable<Printer["nozzle_temperatures"]>,
): PresentedNozzle[] {
  const present: PresentedNozzle[] = [];
  for (const nozzle of nozzles) {
    if (nozzle.current_celsius) {
      present.push({
        ...nozzle,
        label: nozzle.label ?? String(present.length + 1),
      });
    }
  }
  return present;
}

export function formatTemperatureValue(value?: string | null, suffix = true) {
  if (!value) {
    return suffix ? "-°C" : "-°";
  }
  const parsed = Number(value);
  const text = Number.isFinite(parsed) ? `${Math.round(parsed)}` : value;
  return suffix ? `${text}°C` : `${text}°`;
}

export function hasActiveTargetTemperature(value?: string | null) {
  if (!value) {
    return false;
  }
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0;
}

/** The label of the nozzle the printer reports as active, if any. */
export function activeNozzleLabel(printer: Printer) {
  const activeNozzle = normalizeToolhead(printer.active_nozzle);
  if (activeNozzle) {
    return activeNozzle;
  }

  const activeTray = printer.materials?.active_tray;
  if (!activeTray) {
    return null;
  }

  if (activeTray.kind === "external") {
    return normalizeToolhead(
      printer.materials?.external_spools.find(
        (spool) =>
          spool.external_id === activeTray.external_id &&
          (!activeTray.tray_id || spool.tray_id === activeTray.tray_id),
      )?.toolhead,
    );
  }

  const unit = printer.materials?.ams_units.find(
    (ams) => ams.unit_id === activeTray.ams_id,
  );
  const tray = unit?.trays?.find(
    (tray) =>
      tray.tray_id === activeTray.tray_id ||
      (activeTray.global_tray_id !== null &&
        activeTray.global_tray_id !== undefined &&
        tray.global_tray_id === activeTray.global_tray_id),
  );
  return normalizeToolhead(tray?.toolhead ?? unit?.toolhead);
}

/** Maps a nozzle label onto the extruder id the Hub expects: Bambu `L`/`R` or Klipper `T<n>`. */
export function normalizeToolhead(value?: string | null) {
  const normalized = value?.trim().toUpperCase();
  if (normalized === "L" || normalized === "R") return normalized;
  return normalized && /^T\d+$/.test(normalized) ? normalized : null;
}

export function extruderIdForNozzle(label?: string | null) {
  const toolhead = normalizeToolhead(label);
  if (toolhead === "L") return 1;
  if (toolhead === "R") return 0;
  const tool = toolhead ? Number(toolhead.slice(1)) : NaN;
  return Number.isInteger(tool) ? tool : null;
}

export function nozzleTitle(
  label: string,
  t: ReturnType<typeof useTranslations>,
) {
  switch (label) {
    case "L":
      return t("leftNozzleTemp");
    case "R":
      return t("rightNozzleTemp");
    default:
      return label;
  }
}
