//! Device enumeration and identity that survives a replug.
//!
//! A device found again after being unplugged must be recognised as the same
//! one. The audio host provides a stable identifier for exactly that, including
//! across a change of port, and it is that identifier, not the display name,
//! which gets persisted. Two headsets of the same model stay distinguishable,
//! and a renamed device stays recognised.

use std::collections::HashSet;
use std::str::FromStr;

use cpal::device_description::InterfaceType;
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{Device, DeviceId, Host, SupportedStreamConfig};
use serde::Serialize;

use super::convert;

/// True when the platform can capture what a render device is playing without
/// an extra driver.
///
/// Windows offers this natively, macOS since 14.6 by building an aggregate
/// device. Elsewhere, capturing an output goes through a monitor source exposed
/// by the sound server, which then shows up as an ordinary input.
pub const fn supports_loopback() -> bool {
    cfg!(any(target_os = "windows", target_os = "macos"))
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeviceEntry {
    pub id: String,
    pub name: String,
    /// Connection type, as a stable key the interface renders.
    pub interface: &'static str,
    pub is_default: bool,
    pub sample_rate: u32,
    pub channels: u16,
    /// True when using it as a source means capturing an output.
    pub loopback: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCatalog {
    /// Devices usable as the mirror's source.
    pub sources: Vec<DeviceEntry>,
    /// Devices usable as a destination.
    pub targets: Vec<DeviceEntry>,
    /// True when the platform can capture an output directly.
    pub loopback_available: bool,
}

/// Lists the usable devices.
///
/// A device with no format convertible to linear pulse-code modulation is left
/// out: better not to offer it than to let the user pick a destination that
/// would render noise.
pub fn catalog(host: &Host) -> DeviceCatalog {
    let default_output = default_id(host.default_output_device().as_ref());
    let default_input = default_id(host.default_input_device().as_ref());

    let mut catalog = DeviceCatalog {
        loopback_available: supports_loopback(),
        ..Default::default()
    };

    let Ok(devices) = host.devices() else {
        return catalog;
    };

    for device in devices {
        let Ok(id) = device.id() else { continue };
        let id = id.to_string();
        let name = device
            .description()
            .map(|description| description.name().to_owned())
            .unwrap_or_else(|_| device.to_string());
        let interface = interface_key(&device);

        if let Ok(config) = device.default_output_config() {
            if convert::is_linear_pcm(config.sample_format()) {
                let is_default = default_output.as_deref() == Some(id.as_str());

                catalog.targets.push(DeviceEntry {
                    id: id.clone(),
                    name: name.clone(),
                    interface,
                    is_default,
                    sample_rate: config.sample_rate(),
                    channels: config.channels(),
                    loopback: false,
                });

                // An output doubles as a source wherever the system can capture
                // what it renders.
                if supports_loopback() {
                    catalog.sources.push(DeviceEntry {
                        id: id.clone(),
                        name: name.clone(),
                        interface,
                        is_default,
                        sample_rate: config.sample_rate(),
                        channels: config.channels(),
                        loopback: true,
                    });
                }
            }
        }

        if let Ok(config) = device.default_input_config() {
            if convert::is_linear_pcm(config.sample_format()) {
                let is_default = default_input.as_deref() == Some(id.as_str());
                catalog.sources.push(DeviceEntry {
                    id,
                    name,
                    interface,
                    is_default,
                    sample_rate: config.sample_rate(),
                    channels: config.channels(),
                    loopback: false,
                });
            }
        }
    }

    // Outputs first, then inputs, each group alphabetically: host enumeration
    // order carries no stability guarantee at all.
    catalog
        .sources
        .sort_by(|a, b| b.loopback.cmp(&a.loopback).then_with(|| a.name.cmp(&b.name)));
    catalog.targets.sort_by(|a, b| a.name.cmp(&b.name));

    catalog
}

/// Identifiers currently present on the system. Used to track plugging events,
/// where only appearance and disappearance matter: this avoids querying every
/// device's formats several times per second.
pub fn present_ids(host: &Host) -> HashSet<String> {
    let Ok(devices) = host.devices() else {
        return HashSet::new();
    };

    devices
        .filter_map(|device| device.id().ok())
        .map(|id| id.to_string())
        .collect()
}

/// Finds a device from the persisted identifier.
pub fn find(host: &Host, id: &str) -> Option<Device> {
    let parsed = DeviceId::from_str(id).ok()?;
    host.device_by_id(&parsed)
}

/// Display name of an already opened device.
pub fn name_of(device: &Device) -> String {
    device
        .description()
        .map(|description| description.name().to_owned())
        .unwrap_or_else(|_| device.to_string())
}

/// The format to capture this device with, plus whether that means loopback.
///
/// An input is captured in its input format. An output is captured by loopback,
/// and the host then imposes the system mixer's format, which is its output
/// format: asking for anything else makes the stream fail to open.
pub fn capture_config(device: &Device) -> Result<(SupportedStreamConfig, bool), cpal::Error> {
    match device.default_input_config() {
        Ok(config) => Ok((config, false)),
        Err(input_error) => match device.default_output_config() {
            Ok(config) if supports_loopback() => Ok((config, true)),
            _ => Err(input_error),
        },
    }
}

/// The format to render on this device with.
pub fn render_config(device: &Device) -> Result<SupportedStreamConfig, cpal::Error> {
    device.default_output_config()
}

fn default_id(device: Option<&Device>) -> Option<String> {
    device
        .and_then(|device| device.id().ok())
        .map(|id| id.to_string())
}

fn interface_key(device: &Device) -> &'static str {
    let Ok(description) = device.description() else {
        return "unknown";
    };

    match description.interface_type() {
        InterfaceType::BuiltIn => "builtin",
        InterfaceType::Usb => "usb",
        InterfaceType::Bluetooth => "bluetooth",
        InterfaceType::Pci => "pci",
        InterfaceType::FireWire => "firewire",
        InterfaceType::Thunderbolt => "thunderbolt",
        InterfaceType::Hdmi => "hdmi",
        InterfaceType::Line => "line",
        InterfaceType::Spdif => "spdif",
        InterfaceType::Network => "network",
        InterfaceType::Virtual => "virtual",
        InterfaceType::DisplayPort => "displayport",
        InterfaceType::Aggregate => "aggregate",
        _ => "unknown",
    }
}
