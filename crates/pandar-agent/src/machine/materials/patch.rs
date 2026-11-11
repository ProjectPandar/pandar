use pandar_core::AmsUnitKind;
use serde::{Serialize, Serializer};
use serde_json::Number;

#[derive(Serialize)]
pub(crate) struct MaterialPatchDocument<'a> {
    #[serde(rename = "type")]
    pub(crate) document_type: &'static str,
    pub(crate) observed_at: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cfg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) aux: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stat: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) filament_switch_installed: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) ams_units: Vec<AmsUnitPatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) external_spools: Option<Vec<ExternalSpoolPatch>>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) replace_external_spools: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) active_tray: Option<ActiveTrayPatch>,
}

#[derive(Serialize)]
pub(crate) struct AmsUnitPatch {
    pub(crate) unit_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) unit_kind: Option<AmsUnitKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) info: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) humidity: Option<Number>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) humidity_level: Option<Number>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) temperature_celsius: Option<Number>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) dry_status: Option<Number>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) dry_time_minutes: Option<Number>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) toolhead: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) trays: Vec<MaterialTrayPatch>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) replace_trays: bool,
}

#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum MaterialTrayPatch {
    Present(MaterialTrayEntryPatch),
    EmptyClear(EmptyTrayClear),
}

#[derive(Serialize)]
pub(crate) struct MaterialTrayEntryPatch {
    pub(crate) tray_id: String,
    pub(crate) exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) unit_kind: Option<AmsUnitKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) global_tray_id: Option<u64>,
    #[serde(flatten)]
    pub(crate) fields: MaterialFieldsPatch,
}

#[derive(Serialize)]
pub(crate) struct ExternalSpoolPatch {
    pub(crate) external_id: String,
    pub(crate) exists: bool,
    pub(crate) tray_id: String,
    #[serde(flatten)]
    pub(crate) fields: MaterialFieldsPatch,
}

#[derive(Default, Serialize)]
pub(crate) struct MaterialFieldsPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) filament_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) setting_id: Option<String>,
    #[serde(rename = "type")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) filament_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) multi_color: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tag_uid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tray_uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) remaining_estimate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) k_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) toolhead: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct EmptyTrayClear {
    tray_id: String,
    exists: bool,
    unit_kind: AmsUnitKind,
    global_tray_id: Option<u64>,
    state: &'static str,
    filament_id: Option<&'static str>,
    setting_id: Option<&'static str>,
    #[serde(rename = "type")]
    filament_type: Option<&'static str>,
    color: Option<&'static str>,
    multi_color: Option<Vec<String>>,
    tag_uid: Option<&'static str>,
    tray_uuid: Option<&'static str>,
    name: Option<&'static str>,
    remaining_estimate: Option<&'static str>,
}

pub(crate) enum ActiveTrayPatch {
    None,
    External(ExternalActiveTray),
    Ams(AmsActiveTray),
    AmsHt(AmsHtActiveTray),
}

#[derive(Serialize)]
pub(crate) struct ExternalActiveTray {
    kind: &'static str,
    external_id: &'static str,
    tray_id: &'static str,
    global_tray_id: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct AmsActiveTray {
    kind: &'static str,
    global_tray_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    ams_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tray_id: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct AmsHtActiveTray {
    kind: &'static str,
    global_tray_id: Option<u64>,
    ams_id: String,
    tray_id: &'static str,
}

pub(crate) fn empty_tray_clear_patch(
    tray_id: String,
    unit_kind: AmsUnitKind,
    global_tray_id: Option<u64>,
) -> MaterialTrayPatch {
    MaterialTrayPatch::EmptyClear(EmptyTrayClear {
        tray_id,
        exists: false,
        unit_kind,
        global_tray_id,
        state: "9",
        filament_id: None,
        setting_id: None,
        filament_type: None,
        color: None,
        multi_color: None,
        tag_uid: None,
        tray_uuid: None,
        name: None,
        remaining_estimate: None,
    })
}

pub(crate) fn external_active_tray_patch() -> ActiveTrayPatch {
    ActiveTrayPatch::External(ExternalActiveTray {
        kind: "external",
        external_id: "254",
        tray_id: "0",
        global_tray_id: None,
    })
}

pub(crate) fn ams_active_tray_patch(
    tray_now: i64,
    ams_id: String,
    tray_id: String,
) -> ActiveTrayPatch {
    ActiveTrayPatch::Ams(AmsActiveTray {
        kind: "ams",
        global_tray_id: tray_now,
        ams_id: Some(ams_id),
        tray_id: Some(tray_id),
    })
}

pub(crate) fn mixed_ams_lite_global_active_tray_patch(tray_now: i64) -> ActiveTrayPatch {
    ActiveTrayPatch::Ams(AmsActiveTray {
        kind: "ams",
        global_tray_id: tray_now,
        ams_id: None,
        tray_id: None,
    })
}

pub(crate) fn ams_ht_active_tray_patch(tray_now: i64) -> ActiveTrayPatch {
    ActiveTrayPatch::AmsHt(AmsHtActiveTray {
        kind: "ams_ht",
        global_tray_id: None,
        ams_id: tray_now.to_string(),
        tray_id: "0",
    })
}

impl Serialize for ActiveTrayPatch {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::None => serializer.serialize_none(),
            Self::External(value) => value.serialize(serializer),
            Self::Ams(value) => value.serialize(serializer),
            Self::AmsHt(value) => value.serialize(serializer),
        }
    }
}
