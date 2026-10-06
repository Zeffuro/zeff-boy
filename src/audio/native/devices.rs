use anyhow::Context;
use cpal::DeviceId;
use cpal::traits::{DeviceTrait, HostTrait};

use super::AudioOutputDevice;

pub(crate) fn output_devices() -> anyhow::Result<Vec<AudioOutputDevice>> {
    let host = cpal::default_host();
    let mut devices = host
        .output_devices()
        .context("failed to enumerate audio output devices")?
        .filter_map(|device| output_device_info(&device).ok())
        .collect::<Vec<_>>();
    devices.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    Ok(devices)
}

pub(super) fn output_device_info(device: &cpal::Device) -> anyhow::Result<AudioOutputDevice> {
    Ok(AudioOutputDevice {
        id: device
            .id()
            .context("failed to obtain audio output device ID")?
            .to_string(),
        name: device
            .description()
            .map(|description| description.name().to_owned())
            .unwrap_or_else(|_| "Unnamed output".to_owned()),
    })
}

pub(in crate::audio) fn resolve_output_device(host: &cpal::Host, id: &str) -> Option<cpal::Device> {
    let id: DeviceId = id.parse().ok()?;
    // ALSA's compatibility lookup rewrites some valid enumerated plugin IDs.
    host.output_devices()
        .ok()
        .and_then(|mut devices| devices.find(|device| device.id().ok().as_ref() == Some(&id)))
        .or_else(|| host.device_by_id(&id).filter(DeviceTrait::supports_output))
}
