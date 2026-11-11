use super::*;

pub(super) fn normalize_active_tray(
    value: Option<&ScalarValue>,
    units: &[AmsUnitPatch],
) -> Option<ActiveTrayPatch> {
    let tray_now = parse_i64(value?)?;
    match tray_now {
        255 => return Some(ActiveTrayPatch::None),
        254 => return Some(external_active_tray_patch()),
        _ => {}
    }
    if let Some(active) = ams_3h_active_tray(units, tray_now) {
        return Some(active);
    }
    if let Some((ams_id, tray_id)) = active_ams_route(units, tray_now) {
        return Some(ams_active_tray_patch(tray_now, ams_id, tray_id));
    }
    match tray_now {
        0..=15 => Some(ams_active_tray_patch(
            tray_now,
            (tray_now / 4).to_string(),
            (tray_now % 4).to_string(),
        )),
        24..=27 => Some(mixed_ams_lite_global_active_tray_patch(tray_now)),
        128..=135 => Some(ams_ht_active_tray_patch(tray_now)),
        _ => None,
    }
}

// The H3C reports `tray_now` as the raw AMS 3H slot (0-8), not its global id.
fn ams_3h_active_tray(units: &[AmsUnitPatch], tray_now: i64) -> Option<ActiveTrayPatch> {
    let slot = u64::try_from(tray_now).ok()?;
    let unit = units
        .iter()
        .find(|unit| unit.unit_kind == Some(AmsUnitKind::Ams3h))?;
    let global_tray_id = AmsUnitKind::Ams3h.global_tray_id(0, slot)?;
    Some(ams_active_tray_patch(
        i64::try_from(global_tray_id).ok()?,
        unit.unit_id.clone(),
        slot.to_string(),
    ))
}

fn active_ams_route(units: &[AmsUnitPatch], tray_now: i64) -> Option<(String, String)> {
    let tray_now = u64::try_from(tray_now).ok()?;
    units.iter().find_map(|unit| {
        unit.trays.iter().find_map(|tray| match tray {
            MaterialTrayPatch::Present(tray) if tray.global_tray_id == Some(tray_now) => {
                Some((unit.unit_id.clone(), tray.tray_id.clone()))
            }
            MaterialTrayPatch::Present(_) | MaterialTrayPatch::EmptyClear(_) => None,
        })
    })
}
