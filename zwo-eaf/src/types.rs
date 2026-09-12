//! Safe Rust types mirroring the C SDK structs and enums.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};

use zwo_eaf_sys as sys;

/// Converts a null-terminated, fixed-size `c_char` buffer to an owned `String`.
/// A buffer with no terminator is truncated at its end.
pub(crate) fn chars_to_string(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf.iter().map(|&c| c as u8).collect();
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Identity of a connected focuser, returned by [`connected_focusers`](crate::connected_focusers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocuserInfo {
    /// Stable SDK ID; pass to [`Focuser::open`](crate::Focuser::open).
    pub id: i32,
    /// Display name, e.g. `"EAF"`.
    pub name: String,
    /// Hardware position range in steps (the SDK reports the full range
    /// here, e.g. 600000; the user-configurable limit is
    /// [`Focuser::max_step`](crate::Focuser::max_step)).
    pub max_step: i32,
}

impl From<sys::EAF_INFO> for FocuserInfo {
    fn from(i: sys::EAF_INFO) -> Self {
        Self {
            id: i.ID,
            name: chars_to_string(&i.Name),
            max_step: i.MaxStep,
        }
    }
}

/// Which control an [`ControlCaps`] entry describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum ControlType {
    BleName = sys::CONTROL_BLE_NAME,
    Led = sys::CONTROL_LED,
    Buzzer = sys::CONTROL_BUZZER,
    FocuserType = sys::CONTROL_EAF_TYPE,
    BatteryInfo = sys::CONTROL_BAT_INFO,
    ErrorCode = sys::CONTROL_ERR_CODE,
    FirmwareUpdateBle = sys::CONTROL_FW_UPDATE_BLE,
    FirmwareUpdateUsb = sys::CONTROL_FW_UPDATE_USB,
}

impl TryFrom<c_int> for ControlType {
    type Error = crate::Error;
    fn try_from(v: c_int) -> crate::Result<Self> {
        Ok(match v {
            sys::CONTROL_BLE_NAME => Self::BleName,
            sys::CONTROL_LED => Self::Led,
            sys::CONTROL_BUZZER => Self::Buzzer,
            sys::CONTROL_EAF_TYPE => Self::FocuserType,
            sys::CONTROL_BAT_INFO => Self::BatteryInfo,
            sys::CONTROL_ERR_CODE => Self::ErrorCode,
            sys::CONTROL_FW_UPDATE_BLE => Self::FirmwareUpdateBle,
            sys::CONTROL_FW_UPDATE_USB => Self::FirmwareUpdateUsb,
            other => return Err(crate::Error::Unknown(other)),
        })
    }
}

/// Capabilities of one control, from [`Focuser::control_caps`](crate::Focuser::control_caps).
#[derive(Debug, Clone, PartialEq)]
pub struct ControlCaps {
    pub name: String,
    pub description: String,
    pub is_supported: bool,
    pub is_writable: bool,
    pub max_value: i32,
    pub min_value: i32,
    pub default_value: i32,
    /// `Err(Unknown)` if the SDK reports a type this crate does not know.
    pub control_type: std::result::Result<ControlType, i32>,
}

impl From<sys::EAF_CONTROL_CAPS> for ControlCaps {
    fn from(c: sys::EAF_CONTROL_CAPS) -> Self {
        Self {
            name: chars_to_string(&c.name),
            description: chars_to_string(&c.description),
            is_supported: c.isSupported,
            is_writable: c.isWritable,
            max_value: c.maxValue,
            min_value: c.minValue,
            default_value: c.defaultValue,
            control_type: ControlType::try_from(c.controlType).map_err(|_| c.controlType),
        }
    }
}

/// Battery telemetry for battery-powered models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BatteryInfo {
    pub temperature: i32,
    pub voltage: i32,
    pub charge_current: i32,
    pub percentage: i32,
    pub discharge_current: i32,
    pub health: i32,
    pub charge_voltage: i32,
    pub num_cycles: i32,
}

impl From<sys::EAF_BATTERY_INFO> for BatteryInfo {
    fn from(b: sys::EAF_BATTERY_INFO) -> Self {
        Self {
            temperature: b.battery_temp,
            voltage: b.battery_vol,
            charge_current: b.battery_charge_curr,
            percentage: b.battery_percentage,
            discharge_current: b.battery_discharge_curr,
            health: b.battery_health,
            charge_voltage: b.battery_charge_vol,
            num_cycles: b.battery_num_of_cycles,
        }
    }
}

/// Motor and battery error codes as two-character strings.
///
/// `E0` no error, `E5` motor stall, `E6` high temperature stop charge,
/// `E7` high temperature stop discharge, `E8` low temperature stop charge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorCodes {
    pub motor: String,
    pub battery: String,
}

impl From<sys::EAF_ERROR_MSG> for ErrorCodes {
    fn from(m: sys::EAF_ERROR_MSG) -> Self {
        Self {
            motor: chars_to_string(&m.motor_error_code),
            battery: chars_to_string(&m.battery_error_code),
        }
    }
}

/// Why the focuser last powered off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerOffReason {
    Normal,
    ShippingMode,
    Other(i32),
}

impl From<c_int> for PowerOffReason {
    fn from(v: c_int) -> Self {
        match v {
            0 => Self::Normal,
            1 => Self::ShippingMode,
            other => Self::Other(other),
        }
    }
}

/// Firmware version triple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FirmwareVersion {
    pub major: u8,
    pub minor: u8,
    pub build: u8,
}

impl std::fmt::Display for FirmwareVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.build)
    }
}

pub(crate) fn cstr_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        None
    } else {
        Some(
            unsafe { CStr::from_ptr(ptr) }
                .to_string_lossy()
                .into_owned(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chars_to_string_stops_at_nul() {
        let buf: [c_char; 8] = [
            b'E' as c_char,
            b'A' as c_char,
            b'F' as c_char,
            0,
            b'x' as c_char,
            0,
            0,
            0,
        ];
        assert_eq!(chars_to_string(&buf), "EAF");
    }

    #[test]
    fn chars_to_string_without_nul_uses_whole_buffer() {
        let buf: [c_char; 2] = [b'E' as c_char, b'5' as c_char];
        assert_eq!(chars_to_string(&buf), "E5");
    }

    #[test]
    fn control_type_round_trips() {
        for v in 0..sys::CONTROL_MAX_INDEX {
            let t = ControlType::try_from(v).unwrap();
            assert_eq!(t as i32, v);
        }
        assert!(ControlType::try_from(sys::CONTROL_MAX_INDEX).is_err());
    }

    #[test]
    fn firmware_version_display() {
        let v = FirmwareVersion {
            major: 3,
            minor: 1,
            build: 7,
        };
        assert_eq!(v.to_string(), "3.1.7");
    }
}
