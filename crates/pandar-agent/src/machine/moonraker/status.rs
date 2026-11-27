use pandar_core::PrinterCoolingFanKind;
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ObjectSelection {
    /// Every object Klipper reported, projected or not.
    pub(super) available: Vec<String>,
    pub(super) subscribed: Vec<String>,
    pub(super) extruders: Vec<String>,
    pub(super) chamber: Option<String>,
    pub(super) light: Option<String>,
    pub(super) fans: Vec<CoolingFan>,
    pub(super) filament_channels: bool,
}

impl ObjectSelection {
    pub(super) fn has_object(&self, name: &str) -> bool {
        self.available.iter().any(|object| object == name)
    }
}

/// A Klipper fan object projected onto one of Pandar's cooling-fan kinds. The
/// dashboard keys fans by kind, so each kind is assigned at most once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CoolingFan {
    /// Full Klipper object name, e.g. `fan` or `fan_generic exhaust_fan`.
    pub(super) object: String,
    pub(super) kind: PrinterCoolingFanKind,
}

impl CoolingFan {
    /// Pandar's 1-based `SetFanSpeed` index, matching the Bambu airduct fan ids.
    pub(super) fn control_index(&self) -> Option<u8> {
        match self.kind {
            PrinterCoolingFanKind::PartCooling => Some(1),
            PrinterCoolingFanKind::Auxiliary => Some(2),
            PrinterCoolingFanKind::Chamber => Some(3),
            _ => None,
        }
    }

    /// G-code that drives this fan at `speed_percent`. `[fan]` only exposes
    /// `M106`; every other fan object answers the `SET_FAN_SPEED FAN=<name>` mux
    /// that `fan_generic` and `temperature_fan` share.
    pub(super) fn speed_script(&self, speed_percent: u8) -> String {
        let speed_percent = speed_percent.min(100);
        if self.object != "fan" {
            let name = self
                .object
                .as_str()
                .split_once(' ')
                .map_or(self.object.as_str(), |(_, name)| name);
            let speed = f64::from(speed_percent) / 100.0;
            return format!("SET_FAN_SPEED FAN={name} SPEED={speed}");
        }
        format!("M106 S{}", (u32::from(speed_percent) * 255 + 50) / 100)
    }
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
        let fans = select_fans(available);
        let filament_channels = has("print_task_config");

        let mut subscribed: Vec<String> = FIXED_OBJECTS
            .iter()
            .filter(|name| has(name))
            .map(|name| (*name).to_owned())
            .collect();
        subscribed.extend(extruders.iter().cloned());
        subscribed.extend(chamber.iter().cloned());
        subscribed.extend(light.iter().cloned());
        subscribed.extend(fans.iter().map(|fan| fan.object.clone()));

        Self {
            available: available.to_vec(),
            subscribed,
            extruders,
            chamber,
            light,
            fans,
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

/// Maps Klipper fan objects onto Pandar's cooling-fan kinds. `[fan]` is the
/// part-cooling fan; `heater_fan` and `controller_fan` are read-only; other
/// fans (`fan_generic`, `temperature_fan`, or firmware-specific `*_fan`
/// sections) are classified by name so per-tool part-cooling fans and other
/// unmatched fans are not mislabelled. Each kind is assigned at most once.
fn select_fans(available: &[String]) -> Vec<CoolingFan> {
    // Snapmaker's `[purifier]` module drives the exhaust and circulation fans
    // itself and refuses manual speeds while no purifier is attached, so those
    // fans are not offered as controllable.
    let module_owned_exhaust = available.iter().any(|object| object == "purifier");
    let mut fans: Vec<CoolingFan> = Vec::new();
    for name in available {
        let Some(mut kind) = fan_kind(name, module_owned_exhaust) else {
            continue;
        };
        if fans.iter().any(|fan| fan.kind == kind) {
            if kind != PrinterCoolingFanKind::Hotend {
                continue;
            }
            kind = PrinterCoolingFanKind::HotendSecond;
        }
        if fans.iter().any(|fan| fan.kind == kind) {
            continue;
        }
        fans.push(CoolingFan {
            object: name.clone(),
            kind,
        });
    }
    fans
}

fn fan_kind(name: &str, module_owned_exhaust: bool) -> Option<PrinterCoolingFanKind> {
    match name.split_once(' ') {
        Some(("heater_fan", suffix)) => heater_fan_kind(suffix),
        Some(("controller_fan", _)) => Some(PrinterCoolingFanKind::Controller),
        Some(("fan_generic" | "temperature_fan", suffix)) => {
            named_fan_kind(suffix, module_owned_exhaust)
        }
        None if name == "fan" => Some(PrinterCoolingFanKind::PartCooling),
        None if name.ends_with("_fan") => named_fan_kind(name, module_owned_exhaust),
        _ => None,
    }
}

/// `heater_fan` sections cool either a hotend or the electronics; the Snapmaker
/// U1, for example, declares `heater_fan power_fan` next to `heater_fan e0_nozzle_fan`.
fn heater_fan_kind(name: &str) -> Option<PrinterCoolingFanKind> {
    let name = name.to_ascii_lowercase();
    let contains = |keywords: &[&str]| keywords.iter().any(|keyword| name.contains(keyword));
    if contains(&["nozzle", "hotend", "extruder", "tool"]) {
        Some(PrinterCoolingFanKind::Hotend)
    } else if contains(&["power", "board", "mcu", "electronic", "cpu", "controller"]) {
        Some(PrinterCoolingFanKind::Controller)
    } else {
        None
    }
}

fn named_fan_kind(name: &str, module_owned_exhaust: bool) -> Option<PrinterCoolingFanKind> {
    let name = name.to_ascii_lowercase();
    if is_chamber_name(&name) {
        Some(PrinterCoolingFanKind::Chamber)
    } else if name.contains("aux") || (name.contains("exhaust") && !module_owned_exhaust) {
        Some(PrinterCoolingFanKind::Auxiliary)
    } else {
        None
    }
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
pub(super) struct Extruder {
    pub(super) temperature: Option<f64>,
    pub(super) target: Option<f64>,
    pub(super) nozzle_diameter: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct Fan {
    pub(super) speed: Option<f64>,
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
