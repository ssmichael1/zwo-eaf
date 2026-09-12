//! Raw FFI bindings to the ZWO EAF (Electronic Automatic Focuser) C SDK (v1.8.1).
//!
//! This crate provides unsafe, low-level access to every function and type in
//! `EAF_focuser.h`. For a safe, idiomatic Rust API use the
//! [`zwo-eaf`](https://crates.io/crates/zwo-eaf) crate instead.
//!
//! # SDK resolution
//!
//! The ZWO SDK binaries are MIT licensed but are **not vendored** in this
//! crate. The build script locates them in this order:
//!
//! 1. `ZWO_EAF_SDK_PATH` — root of an extracted ZWO SDK (`eaf/` on Linux and
//!    macOS, `EAF_Windows_SDK_V1.8.1/` on Windows).
//! 2. `ZWO_EAF_SDK_TARBALL` — a local copy of the per-target tarball from the
//!    GitHub release (checksum verified, then extracted into `OUT_DIR`).
//! 3. A sibling `EAF_SDK_V1.8.1/` directory next to the crate or workspace.
//! 4. Download from the `sdk-1.8.1` release of the
//!    [zwo-eaf](https://github.com/ssmichael1/zwo-eaf) repository, verified
//!    against a SHA-256 pinned in `build.rs` and cached in `OUT_DIR`.
//!
//! # Platform support
//!
//! | OS | Arch | Linkage |
//! |----|------|---------|
//! | macOS | aarch64, x86_64 | static `libEAFFocuser.a` + IOKit/CoreFoundation/Foundation/Cocoa/AppKit/CoreBluetooth |
//! | Linux | x86_64, x86, armv6, armv7, aarch64 | static `libEAFFocuser.a` + `libstdc++`, `libdl` |
//! | Windows | x86_64, x86 | `EAF_focuser.dll` via import library |
//!
//! On Linux the focuser is a USB HID device; install the `eaf.rules` udev
//! rule shipped with the SDK for non-root access (VID `03c3`, PID `1f10`).
//!
//! # Feature flags
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `bluetooth` | off | Exposes the `EAFBLE*` functions and callback types for BLE-capable focusers. |
//!
//! # Safety
//!
//! All functions in the `extern "C"` blocks are unsafe. Callers must pass
//! valid pointers and open a focuser with [`EAFOpen`] before calling any
//! per-device function.

#![allow(non_camel_case_types, non_snake_case, clippy::upper_case_acronyms)]

use std::os::raw::{c_char, c_float, c_int, c_longlong, c_uchar};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Focuser IDs returned by [`EAFGetID`] are in `0..EAF_ID_MAX`.
pub const EAF_ID_MAX: c_int = 128;
/// IDs at or above this value denote Bluetooth LE connections.
pub const BLE_DEVICE_MIN_ID: c_int = 50;

/// USB vendor ID of ZWO devices (used with [`EAFCheck`]).
pub const EAF_VENDOR_ID: c_int = 0x03C3;

// ---------------------------------------------------------------------------
// EAF_ERROR_CODE
// ---------------------------------------------------------------------------

/// Return code of nearly every SDK function. See the `EAF_*` constants.
pub type EAF_ERROR_CODE = c_int;

pub const EAF_SUCCESS: EAF_ERROR_CODE = 0;
pub const EAF_ERROR_INVALID_INDEX: EAF_ERROR_CODE = 1;
pub const EAF_ERROR_INVALID_ID: EAF_ERROR_CODE = 2;
pub const EAF_ERROR_INVALID_VALUE: EAF_ERROR_CODE = 3;
/// Failed to find the focuser; it may have been removed.
pub const EAF_ERROR_REMOVED: EAF_ERROR_CODE = 4;
/// Focuser is moving.
pub const EAF_ERROR_MOVING: EAF_ERROR_CODE = 5;
/// Focuser is in an error state.
pub const EAF_ERROR_ERROR_STATE: EAF_ERROR_CODE = 6;
/// Other error.
pub const EAF_ERROR_GENERAL_ERROR: EAF_ERROR_CODE = 7;
pub const EAF_ERROR_NOT_SUPPORTED: EAF_ERROR_CODE = 8;
pub const EAF_ERROR_CLOSED: EAF_ERROR_CODE = 9;
/// Battery temperature is abnormal (sic: `BATTER` in the SDK header).
pub const EAF_ERROR_BATTER_INFO: EAF_ERROR_CODE = 10;
pub const EAF_ERROR_INVALID_LENGTH: EAF_ERROR_CODE = 11;

// Bluetooth LE codes start at 50.
pub const EAF_BLE_READ_DATA_FAILED: EAF_ERROR_CODE = 50;
pub const EAF_BLE_SEND_DATA_FAILED: EAF_ERROR_CODE = 51;
pub const EAF_BLE_CONNECT_FAILED: EAF_ERROR_CODE = 52;
/// BLE is not connected; initialization failed.
pub const EAF_BLE_DISCONNECT: EAF_ERROR_CODE = 53;
pub const EAF_BLE_PAIR_FAILED: EAF_ERROR_CODE = 54;
pub const EAF_BLE_CLEAR_PAIR_FAILED: EAF_ERROR_CODE = 55;
pub const EAF_BLE_PAIRING_TIMEOUT: EAF_ERROR_CODE = 56;
pub const EAF_BLE_RECEIVE_TIMEOUT: EAF_ERROR_CODE = 57;
/// The device is not supported.
pub const EAF_BLE_DEVICE_NOT_EXISTS: EAF_ERROR_CODE = 58;
pub const EAF_BLE_INVALID_CALLBACK: EAF_ERROR_CODE = 59;
/// A new pairing request was received.
pub const EAF_BLE_NEW_PAIR_REQUEST: EAF_ERROR_CODE = 60;
pub const EAF_BLE_DATA_BUSY: EAF_ERROR_CODE = 61;
/// Firmware length sent and received do not match.
pub const EAF_BLE_CHECK_SIZE_FAILED: EAF_ERROR_CODE = 62;

pub const EAF_ERROR_END: EAF_ERROR_CODE = -1;

// ---------------------------------------------------------------------------
// EAF_CONTROL_TYPE
// ---------------------------------------------------------------------------

/// Identifies a control reported by [`EAFGetControlCaps`].
pub type EAF_CONTROL_TYPE = c_int;

pub const CONTROL_BLE_NAME: EAF_CONTROL_TYPE = 0;
pub const CONTROL_LED: EAF_CONTROL_TYPE = 1;
pub const CONTROL_BUZZER: EAF_CONTROL_TYPE = 2;
pub const CONTROL_EAF_TYPE: EAF_CONTROL_TYPE = 3;
pub const CONTROL_BAT_INFO: EAF_CONTROL_TYPE = 4;
pub const CONTROL_ERR_CODE: EAF_CONTROL_TYPE = 5;
/// Firmware update over BLE.
pub const CONTROL_FW_UPDATE_BLE: EAF_CONTROL_TYPE = 6;
/// Firmware update over USB.
pub const CONTROL_FW_UPDATE_USB: EAF_CONTROL_TYPE = 7;
/// Number of control types.
pub const CONTROL_MAX_INDEX: EAF_CONTROL_TYPE = 8;

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

/// Basic focuser property, filled by [`EAFGetProperty`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EAF_INFO {
    /// Unique focuser ID, `0..EAF_ID_MAX`.
    pub ID: c_int,
    /// Null-terminated display name.
    pub Name: [c_char; 64],
    /// Fixed maximum position.
    pub MaxStep: c_int,
}

/// Eight-byte alias / serial number.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EAF_ID {
    pub id: [c_uchar; 8],
}

/// Serial number; same layout as [`EAF_ID`].
pub type EAF_SN = EAF_ID;

/// Focuser model type string, filled by [`EAFGetType`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EAF_TYPE {
    pub r#type: [c_char; 16],
}

/// Bluetooth advertising name, used by [`EAFGetBLEName`] / [`EAFSetBLEName`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EAF_BLE_NAME {
    pub name: [c_char; 16],
}

/// Two-character motor and battery error codes plus terminators.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EAF_ERROR_MSG {
    /// `"E0"` no error, `"E5"` motor stall, etc. Null-terminated.
    pub motor_error_code: [c_char; 3],
    /// `"E6"`/`"E7"`/`"E8"` temperature charge/discharge faults. Null-terminated.
    pub battery_error_code: [c_char; 3],
}

/// Battery telemetry, filled by [`EAFGetBatteryInfo`].
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct EAF_BATTERY_INFO {
    pub battery_temp: c_int,
    pub battery_vol: c_int,
    pub battery_charge_curr: c_int,
    pub battery_percentage: c_int,
    pub battery_discharge_curr: c_int,
    pub battery_health: c_int,
    pub battery_charge_vol: c_int,
    pub battery_num_of_cycles: c_int,
}

/// Complete focuser state, returned in one round trip by [`EAFBLEgetAllInfo`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EAF_ALL_INFO {
    /// Motor running state.
    pub is_run: c_int,
    pub backlash_steps: c_int,
    pub current_steps: c_int,
    pub temperature: c_float,
    pub buzzer_state: c_int,
    pub reverse_state: c_int,
    /// The hand controller button is pressed.
    pub handle_pressed: c_int,
    /// Hand controller connection state.
    pub handle_connect: c_int,
    pub max_steps: c_int,
    pub led_state: c_int,
    /// Motor error code, two characters, *not* null-terminated.
    pub motor_error_code: [c_char; 2],
    /// Battery error code, two characters, *not* null-terminated.
    pub battery_error_code: [c_char; 2],
    pub battery_info: EAF_BATTERY_INFO,
}

/// Capabilities of one control, filled by [`EAFGetControlCaps`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EAF_CONTROL_CAPS {
    pub name: [c_char; 32],
    pub description: [c_char; 128],
    pub isSupported: bool,
    pub isWritable: bool,
    pub maxValue: c_int,
    pub minValue: c_int,
    pub defaultValue: c_int,
    pub controlType: EAF_CONTROL_TYPE,
    pub unused: [c_char; 32],
}

/// A Bluetooth LE device found by [`EAFBLEScan`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct BLE_DEVICE_INFO_T {
    pub name: [c_char; 64],
    /// Formatted Bluetooth address.
    pub address: [c_char; 64],
    pub signalStrength: c_int,
    pub bluetoothAddress: c_longlong,
}

/// Called with `true` on connect and `false` on disconnect.
pub type ConnStateCallback = Option<unsafe extern "C" fn(state: bool)>;
/// Called with the pairing outcome (`EAF_SUCCESS`, `EAF_BLE_NEW_PAIR_REQUEST`,
/// `EAF_BLE_PAIRING_TIMEOUT` or `EAF_BLE_PAIR_FAILED`).
pub type PairStateCallback = Option<unsafe extern "C" fn(state: EAF_ERROR_CODE)>;

// ---------------------------------------------------------------------------
// USB / common functions
// ---------------------------------------------------------------------------

extern "C" {
    /// Returns the number of connected focusers. Call this first; it refreshes
    /// the device list when a focuser is plugged in or removed.
    pub fn EAFGetNum() -> c_int;

    /// Get the product ID of each focuser. Pass null to query the length, then
    /// allocate a buffer and call again. Returns the array length.
    ///
    /// Deprecated by ZWO in favour of [`EAFCheck`].
    pub fn EAFGetProductIDs(pPIDs: *mut c_int) -> c_int;

    /// Returns 1 if the USB VID/PID pair is an EAF focuser, 0 otherwise.
    /// The VID is `0x03C3` ([`EAF_VENDOR_ID`]).
    pub fn EAFCheck(iVID: c_int, iPID: c_int) -> c_int;

    /// Get the ID of the focuser at `index` (`0..EAFGetNum()`). The ID is a
    /// stable integer in `0..EAF_ID_MAX` used by every other function.
    ///
    /// Returns `EAF_ERROR_INVALID_INDEX` or `EAF_SUCCESS`.
    pub fn EAFGetID(index: c_int, ID: *mut c_int) -> EAF_ERROR_CODE;

    /// Open the focuser.
    ///
    /// Returns `EAF_ERROR_INVALID_ID`, `EAF_ERROR_GENERAL_ERROR` (too many
    /// open focusers), `EAF_ERROR_REMOVED` or `EAF_SUCCESS`.
    pub fn EAFOpen(ID: c_int) -> EAF_ERROR_CODE;

    /// Fill `pInfo` with the focuser's ID, name and maximum step.
    ///
    /// Returns `EAF_ERROR_INVALID_ID` or `EAF_SUCCESS`. May return
    /// `EAF_ERROR_MOVING` right after open; retry until it succeeds.
    pub fn EAFGetProperty(ID: c_int, pInfo: *mut EAF_INFO) -> EAF_ERROR_CODE;

    /// Number of controls available; the focuser must be open.
    ///
    /// Returns `EAF_SUCCESS`, `EAF_ERROR_CLOSED`, `EAF_BLE_DISCONNECT` or
    /// `EAF_ERROR_INVALID_ID`.
    pub fn EAFGetNumOfControls(ID: c_int, piNumberOfControls: *mut c_int) -> EAF_ERROR_CODE;

    /// Capabilities of the control at `iControlIndex` (an index, not a
    /// control type). The focuser must be open.
    ///
    /// Returns `EAF_SUCCESS`, `EAF_ERROR_CLOSED`, `EAF_BLE_DISCONNECT` or
    /// `EAF_ERROR_INVALID_ID`.
    pub fn EAFGetControlCaps(
        ID: c_int,
        iControlIndex: c_int,
        pControlCaps: *mut EAF_CONTROL_CAPS,
    ) -> EAF_ERROR_CODE;

    /// Move to an absolute position in `0..=MaxStep`. Returns immediately;
    /// poll [`EAFIsMoving`] for completion.
    ///
    /// Returns `EAF_ERROR_INVALID_ID`, `EAF_ERROR_CLOSED`, `EAF_SUCCESS`,
    /// `EAF_ERROR_ERROR_STATE` or `EAF_ERROR_REMOVED`.
    pub fn EAFMove(ID: c_int, iStep: c_int) -> EAF_ERROR_CODE;

    /// Stop moving.
    ///
    /// Returns `EAF_ERROR_INVALID_ID`, `EAF_ERROR_CLOSED`, `EAF_SUCCESS`,
    /// `EAF_ERROR_ERROR_STATE` or `EAF_ERROR_REMOVED`.
    pub fn EAFStop(ID: c_int) -> EAF_ERROR_CODE;

    /// Stop moving and block until the focuser reports idle or `timeoutMs`
    /// elapses. The C++ header defaults `timeoutMs` to 1000.
    ///
    /// Returns the [`EAFStop`] codes plus `EAF_ERROR_MOVING` on timeout.
    pub fn EAFStopAndWait(ID: c_int, timeoutMs: c_int) -> EAF_ERROR_CODE;

    /// Query whether the focuser is moving. `pbHandControl` is set when the
    /// motion was started from the hand controller and cannot be stopped by
    /// [`EAFStop`].
    ///
    /// Returns `EAF_ERROR_INVALID_ID`, `EAF_ERROR_CLOSED`, `EAF_SUCCESS`,
    /// `EAF_ERROR_ERROR_STATE` or `EAF_ERROR_REMOVED`.
    pub fn EAFIsMoving(ID: c_int, pbVal: *mut bool, pbHandControl: *mut bool) -> EAF_ERROR_CODE;

    /// Get the current position in steps.
    ///
    /// Returns `EAF_ERROR_INVALID_ID`, `EAF_ERROR_CLOSED`, `EAF_SUCCESS`,
    /// `EAF_ERROR_ERROR_STATE` or `EAF_ERROR_REMOVED`.
    pub fn EAFGetPosition(ID: c_int, piStep: *mut c_int) -> EAF_ERROR_CODE;

    /// Redefine the current physical position as `iStep` without moving.
    /// (Spelled `Postion` in the SDK.)
    ///
    /// Returns `EAF_ERROR_INVALID_ID`, `EAF_ERROR_CLOSED`, `EAF_SUCCESS`,
    /// `EAF_ERROR_ERROR_STATE` or `EAF_ERROR_REMOVED`.
    pub fn EAFResetPostion(ID: c_int, iStep: c_int) -> EAF_ERROR_CODE;

    /// Read the temperature sensor in °C. If the focuser is being moved by
    /// hand the value is unusable (-273) and `EAF_ERROR_GENERAL_ERROR` is
    /// returned.
    pub fn EAFGetTemp(ID: c_int, pfTemp: *mut c_float) -> EAF_ERROR_CODE;

    /// Enable or disable the beep emitted when a move starts.
    pub fn EAFSetBeep(ID: c_int, bVal: bool) -> EAF_ERROR_CODE;

    /// Query whether the start-of-move beep is enabled.
    pub fn EAFGetBeep(ID: c_int, pbVal: *mut bool) -> EAF_ERROR_CODE;

    /// Set the maximum position. Returns `EAF_ERROR_MOVING` if the focuser
    /// is moving.
    pub fn EAFSetMaxStep(ID: c_int, iVal: c_int) -> EAF_ERROR_CODE;

    /// Get the maximum position. Returns `EAF_ERROR_MOVING` if the focuser
    /// is moving.
    pub fn EAFGetMaxStep(ID: c_int, piVal: *mut c_int) -> EAF_ERROR_CODE;

    /// Get the hardware position range (upper bound for [`EAFSetMaxStep`]).
    /// Returns `EAF_ERROR_MOVING` if the focuser is moving.
    pub fn EAFStepRange(ID: c_int, piVal: *mut c_int) -> EAF_ERROR_CODE;

    /// Set the motor direction; `true` reverses it.
    pub fn EAFSetReverse(ID: c_int, bVal: bool) -> EAF_ERROR_CODE;

    /// Get the motor direction; `true` means reversed.
    pub fn EAFGetReverse(ID: c_int, pbVal: *mut bool) -> EAF_ERROR_CODE;

    /// Set backlash compensation in steps, `0..=255`. Returns
    /// `EAF_ERROR_INVALID_VALUE` if out of range.
    pub fn EAFSetBacklash(ID: c_int, iVal: c_int) -> EAF_ERROR_CODE;

    /// Get backlash compensation in steps.
    pub fn EAFGetBacklash(ID: c_int, piVal: *mut c_int) -> EAF_ERROR_CODE;

    /// Close the focuser.
    ///
    /// Returns `EAF_ERROR_INVALID_ID` or `EAF_SUCCESS`.
    pub fn EAFClose(ID: c_int) -> EAF_ERROR_CODE;

    /// SDK version string such as `"1, 8, 1"`. The pointer is owned by the SDK.
    pub fn EAFGetSDKVersion() -> *const c_char;

    /// Firmware version as major/minor/build bytes.
    pub fn EAFGetFirmwareVersion(
        ID: c_int,
        major: *mut c_uchar,
        minor: *mut c_uchar,
        build: *mut c_uchar,
    ) -> EAF_ERROR_CODE;

    /// Read the serial number. Returns `EAF_ERROR_NOT_SUPPORTED` on firmware
    /// without one.
    pub fn EAFGetSerialNumber(ID: c_int, pSN: *mut EAF_SN) -> EAF_ERROR_CODE;

    /// Read battery telemetry. Returns `EAF_ERROR_NOT_SUPPORTED` on
    /// non-battery models and `EAF_ERROR_BATTER_INFO` (fields set to -1) if
    /// the battery temperature is abnormal.
    pub fn EAFGetBatteryInfo(ID: c_int, pBatteryInfo: *mut EAF_BATTERY_INFO) -> EAF_ERROR_CODE;

    /// Set the eight-byte alias. Returns `EAF_ERROR_NOT_SUPPORTED` on older
    /// firmware.
    pub fn EAFSetID(ID: c_int, alias: EAF_ID) -> EAF_ERROR_CODE;

    /// Read the focuser model type string.
    pub fn EAFGetType(ID: c_int, pEAFType: *mut EAF_TYPE) -> EAF_ERROR_CODE;

    /// Read the Bluetooth advertising name. Only available over the HID (USB)
    /// connection.
    pub fn EAFGetBLEName(ID: c_int, pEAFBLEName: *mut EAF_BLE_NAME) -> EAF_ERROR_CODE;

    /// Set the last six characters of the Bluetooth name (`1..=6` chars, e.g.
    /// `EAF Pro_57fb5d` -> `EAF Pro_123456`). A successful update drops the
    /// connection; rescan to reconnect.
    pub fn EAFSetBLEName(ID: c_int, EAFBLEName: EAF_BLE_NAME) -> EAF_ERROR_CODE;

    /// Get the LED state (`true` = on).
    pub fn EAFGetLedState(ID: c_int, bState: *mut bool) -> EAF_ERROR_CODE;

    /// Set the LED state; `true` restores the normal (on) state, `false`
    /// turns the LED off.
    pub fn EAFSetLedState(ID: c_int, bState: bool) -> EAF_ERROR_CODE;

    /// Read the motor and battery error codes (`E0` none, `E5` motor stall,
    /// `E6`/`E7`/`E8` temperature charge/discharge faults). Returns
    /// `EAF_ERROR_NOT_SUPPORTED` on models without this feature.
    pub fn EAFGetErrorCode(ID: c_int, pErrorCode: *mut EAF_ERROR_MSG) -> EAF_ERROR_CODE;

    /// Put the focuser in shipping mode. ZWO notes this API is currently
    /// unused.
    pub fn EAFSetShippingMode(ID: c_int) -> EAF_ERROR_CODE;

    /// Get the last power-off reason: `0` normal, `1` shipping mode.
    pub fn EAFGetReason(ID: c_int, pReason: *mut c_int) -> EAF_ERROR_CODE;
}

// ---------------------------------------------------------------------------
// Bluetooth LE functions
// ---------------------------------------------------------------------------

#[cfg(feature = "bluetooth")]
extern "C" {
    /// Scan for nearby EAF Bluetooth devices for `DurationMs`, writing up to
    /// `maxDeviceCount` entries to `devices` and the count found to
    /// `actualDeviceCount`. BLE focusers do not need [`EAFGetNum`]/[`EAFOpen`].
    ///
    /// Returns `EAF_SUCCESS` or `EAF_BLE_DEVICE_NOT_EXISTS`.
    pub fn EAFBLEScan(
        DurationMs: c_int,
        devices: *mut BLE_DEVICE_INFO_T,
        maxDeviceCount: c_int,
        actualDeviceCount: *mut c_int,
    ) -> EAF_ERROR_CODE;

    /// Connect to a focuser by BLE name and address, returning its ID (which
    /// is `>= BLE_DEVICE_MIN_ID`).
    ///
    /// Returns `EAF_SUCCESS` or `EAF_ERROR_CLOSED`.
    pub fn EAFBLEConnect(
        pDeviceName: *const c_char,
        pDeviceAddress: *const c_char,
        ID: *mut c_int,
    ) -> EAF_ERROR_CODE;

    /// Register (or clear with `None`) a pairing state callback.
    ///
    /// Returns `EAF_SUCCESS` or `EAF_ERROR_CLOSED`.
    pub fn EAFBLERegPairStateCallback(ID: c_int, callback: PairStateCallback) -> EAF_ERROR_CODE;

    /// Start BLE pairing. Returns `EAF_SUCCESS` or `EAF_BLE_PAIR_FAILED`.
    pub fn EAFBLEPair(ID: c_int) -> EAF_ERROR_CODE;

    /// Clear BLE pairing. Returns `EAF_SUCCESS` or `EAF_BLE_CLEAR_PAIR_FAILED`.
    pub fn EAFBLEClearPair(ID: c_int) -> EAF_ERROR_CODE;

    /// Disconnect the BLE connection. Returns `EAF_SUCCESS` or `EAF_ERROR_CLOSED`.
    pub fn EAFBLEDisconnect(ID: c_int) -> EAF_ERROR_CODE;

    /// Register (or clear with `None`) a connection state callback.
    ///
    /// Returns `EAF_SUCCESS` or `EAF_ERROR_CLOSED`.
    pub fn EAFBLERegConnStateCallback(ID: c_int, callback: ConnStateCallback) -> EAF_ERROR_CODE;

    /// Fetch the complete focuser state in one BLE transaction.
    ///
    /// Returns `EAF_SUCCESS`, `EAF_BLE_DISCONNECT`, `EAF_ERROR_INVALID_ID`,
    /// `EAF_ERROR_INVALID_VALUE` or `EAF_ERROR_NOT_SUPPORTED`.
    pub fn EAFBLEgetAllInfo(ID: c_int, pAllInfo: *mut EAF_ALL_INFO) -> EAF_ERROR_CODE;
}

// ---------------------------------------------------------------------------
// Layout checks against the C header
// ---------------------------------------------------------------------------

#[cfg(test)]
mod layout {
    use super::*;
    use std::mem::{align_of, size_of};

    #[test]
    fn struct_sizes_match_header() {
        assert_eq!(size_of::<EAF_INFO>(), 72);
        assert_eq!(size_of::<EAF_ID>(), 8);
        assert_eq!(size_of::<EAF_TYPE>(), 16);
        assert_eq!(size_of::<EAF_BLE_NAME>(), 16);
        assert_eq!(size_of::<EAF_ERROR_MSG>(), 6);
        assert_eq!(size_of::<EAF_BATTERY_INFO>(), 32);
        assert_eq!(size_of::<EAF_ALL_INFO>(), 76);
        assert_eq!(size_of::<EAF_CONTROL_CAPS>(), 212);
        assert_eq!(align_of::<EAF_CONTROL_CAPS>(), 4);
    }

    #[test]
    fn ble_device_info_size_matches_header() {
        // `long long` is 8-byte aligned on every 64-bit target and on Windows;
        // on 32-bit Linux/ARM the ABI packs it to 4.
        let expected = if cfg!(target_pointer_width = "64") || cfg!(target_os = "windows") {
            144
        } else {
            140
        };
        assert_eq!(size_of::<BLE_DEVICE_INFO_T>(), expected);
    }

    #[test]
    fn callbacks_are_nullable_pointers() {
        assert_eq!(size_of::<ConnStateCallback>(), size_of::<usize>());
        assert_eq!(size_of::<PairStateCallback>(), size_of::<usize>());
    }
}
