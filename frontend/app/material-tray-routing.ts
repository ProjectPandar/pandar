// Units whose trays use a fixed Studio global tray range instead of
// `unit_id * 4 + tray`: the mixed AMS Lite (24-27) and the H3C's AMS 3H (28-36).
export function fixedRangeGlobalTrayId(
  unitKind: string | null | undefined,
  trayId: number,
): number | null {
  if (unitKind === "ams_lite_mixed")
    return trayId >= 0 && trayId < 4 ? 24 + trayId : null;
  if (unitKind === "ams_3h")
    return trayId >= 0 && trayId < 9 ? 28 + trayId : null;
  return null;
}
