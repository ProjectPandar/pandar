use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value};

use super::client::KlipperObjects;

const FIXED_OBJECTS: &[&str] = &[
    "webhooks",
    "print_stats",
    "virtual_sdcard",
    "display_status",
    "toolhead",
    "gcode_move",
    "heater_bed",
    "fan",
    "print_task_config",
];

/// The Klipper objects this printer exposes that Pandar projects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ObjectSelection {
    pub(super) subscribed: Vec<String>,
    pub(super) extruders: Vec<String>,
    pub(super) chamber: Option<String>,
    pub(super) light: Option<String>,
    pub(super) filament_channels: bool,
}

impl ObjectSelection {
    pub(super) fn from_available(available: &[String]) -> Self {
        let has = |name: &str| available.iter().any(|object| object == name);
        let mut extruders: Vec<String> = available
            .iter()
            .filter(|name| extruder_index(name).is_some())
            .cloned()
            .collect();
        extruders.sort_by_key(|name| extruder_index(name));
        let chamber = available
            .iter()
            .find(|name| name.starts_with("heater_generic ") && is_chamber_name(name))
            .or_else(|| {
                available
                    .iter()
                    .find(|name| name.starts_with("temperature_sensor ") && is_chamber_name(name))
            })
            .cloned();
        // Prefer an enclosure light; otherwise use the first LED Klipper exposes.
        let is_light = |name: &&String| name.starts_with("led ") || name.starts_with("neopixel ");
        let light = available
            .iter()
            .filter(is_light)
            .find(|name| is_chamber_name(name))
            .or_else(|| available.iter().find(is_light))
            .cloned();
        let filament_channels = has("print_task_config");

        let mut subscribed: Vec<String> = FIXED_OBJECTS
            .iter()
            .filter(|name| has(name))
            .map(|name| (*name).to_owned())
            .collect();
        subscribed.extend(extruders.iter().cloned());
        subscribed.extend(chamber.iter().cloned());
        subscribed.extend(light.iter().cloned());

        Self {
            subscribed,
            extruders,
            chamber,
            light,
            filament_channels,
        }
    }
}

/// `extruder` is tool 0; `extruderN` is tool N.
pub(super) fn extruder_index(name: &str) -> Option<u32> {
    let suffix = name.strip_prefix("extruder")?;
    if suffix.is_empty() {
        return Some(0);
    }
    suffix.parse().ok()
}

fn is_chamber_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["chamber", "cavity", "enclosure"]
        .iter()
        .any(|keyword| name.contains(keyword))
}

/// Merged Klipper object state. Moonraker status notifications carry only the
/// top-level fields of each object that changed, so a shallow per-field merge
/// reproduces the full object.
#[derive(Debug, Clone, Default)]
pub(super) struct KlipperStatus {
    objects: KlipperObjects,
}

impl KlipperStatus {
    pub(super) fn apply(&mut self, update: KlipperObjects) {
        for (object, fields) in update {
            self.objects.entry(object).or_default().extend(fields);
        }
    }

    pub(super) fn object<T: DeserializeOwned + Default>(&self, name: &str) -> T {
        self.objects
            .get(name)
            .and_then(|fields| decode(fields).ok())
            .unwrap_or_default()
    }

    pub(super) fn has(&self, name: &str) -> bool {
        self.objects.contains_key(name)
    }
}

fn decode<T: DeserializeOwned>(fields: &Map<String, Value>) -> serde_json::Result<T> {
    T::deserialize(Value::Object(fields.clone()))
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct Webhooks {
    #[serde(default)]
    pub(super) state: String,
    #[serde(default)]
    pub(super) state_message: String,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct PrintStats {
    #[serde(default)]
    pub(super) state: String,
    #[serde(default)]
    pub(super) filename: String,
    #[serde(default)]
    pub(super) print_duration: f64,
    #[serde(default)]
    pub(super) message: String,
    #[serde(default)]
    pub(super) info: PrintStatsInfo,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct PrintStatsInfo {
    pub(super) current_layer: Option<u32>,
    pub(super) total_layer: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct VirtualSdcard {
    #[serde(default)]
    pub(super) progress: f64,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct DisplayStatus {
    pub(super) progress: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct Toolhead {
    #[serde(default)]
    pub(super) extruder: String,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct GcodeMove {
    pub(super) speed_factor: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct Heater {
    pub(super) temperature: Option<f64>,
    pub(super) target: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct Led {
    #[serde(default)]
    pub(super) color_data: Vec<Vec<f64>>,
}

/// Per-channel filament configuration published by firmware that exposes `print_task_config`.
#[derive(Debug, Default, Deserialize)]
pub(super) struct PrintTaskConfig {
    #[serde(default)]
    pub(super) filament_exist: Vec<bool>,
    #[serde(default)]
    pub(super) filament_vendor: Vec<String>,
    #[serde(default)]
    pub(super) filament_type: Vec<String>,
    #[serde(default)]
    pub(super) filament_sub_type: Vec<String>,
    #[serde(default)]
    pub(super) filament_color_rgba: Vec<String>,
}
