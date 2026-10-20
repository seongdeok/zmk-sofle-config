//! USB transport: the vendor HID interface (usage page 0xFF0B).
//!
//! Works on Linux (hidraw), macOS (IOHIDManager) and Windows (HID class);
//! the vendor collection is not grabbed by the OS keyboard driver.

use std::time::Duration;

use hidapi::{HidApi, HidDevice};
use gaggskey_proto::{self as proto, Frame, FRAME_SIZE};

use crate::{Device, Error, Result, Transport, Via};

pub struct UsbTransport {
    dev: HidDevice,
    path: String,
}

impl UsbTransport {
    /// Every keyboard exposing the GaggsKey vendor interface.
    pub fn list() -> Vec<Device> {
        let Ok(api) = HidApi::new() else { return Vec::new() };
        api.device_list()
            .filter(|d| d.usage_page() == proto::HID_USAGE_PAGE && d.usage() == proto::HID_USAGE)
            .map(|d| Device {
                name: d.product_string().unwrap_or("ZMK keyboard").to_string(),
                via: Via::Usb,
                id: d.path().to_string_lossy().into_owned(),
            })
            .collect()
    }

    /// Open a specific interface by hidapi path, or the first one found.
    pub fn open(path: Option<&str>) -> Result<Self> {
        let api = HidApi::new().map_err(|e| Error::Io(e.to_string()))?;
        let info = api
            .device_list()
            .filter(|d| d.usage_page() == proto::HID_USAGE_PAGE && d.usage() == proto::HID_USAGE)
            .find(|d| path.is_none_or(|p| d.path().to_string_lossy() == p))
            .ok_or(Error::NotFound("no USB interface with usage page 0xFF0B"))?;
        let path = info.path().to_string_lossy().into_owned();
        let dev = info.open_device(&api).map_err(|e| {
            Error::Io(format!("{path}: {e} (Linux: install udev/70-gaggskey.rules)"))
        })?;
        Ok(Self { dev, path })
    }
}

impl Transport for UsbTransport {
    fn send(&mut self, frame: &Frame) -> Result<()> {
        // Leading 0 = report id (descriptor has none).
        let mut buf = [0u8; FRAME_SIZE + 1];
        buf[1..].copy_from_slice(frame);
        self.dev.write(&buf).map_err(|e| Error::Io(e.to_string()))?;
        Ok(())
    }

    fn recv(&mut self, timeout: Option<Duration>) -> Result<Option<Vec<u8>>> {
        let mut buf = [0u8; 64];
        let ms = timeout.map_or(-1, |t| t.as_millis().clamp(1, i32::MAX as u128) as i32);
        let n = self.dev.read_timeout(&mut buf, ms).map_err(|e| Error::Io(e.to_string()))?;
        Ok((n > 0).then(|| buf[..n].to_vec()))
    }

    fn describe(&self) -> String {
        format!("usb {}", self.path)
    }
}
