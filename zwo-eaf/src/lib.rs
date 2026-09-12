//! Safe, idiomatic Rust wrapper for the ZWO EAF (Electronic Automatic
//! Focuser) SDK.
//!
//! This crate wraps the C SDK in a safe API with an RAII [`Focuser`] handle,
//! Rust enums and `Result`-based error handling. For raw FFI access see
//! [`zwo_eaf_sys`].
//!
//! # Setup
//!
//! The ZWO SDK binaries are fetched automatically at build time (or supplied
//! via `ZWO_EAF_SDK_PATH`); see the `zwo-eaf-sys` README for details. On
//! Linux, install the SDK's `eaf.rules` udev rule for non-root access.
//!
//! # Quick start
//!
//! ```no_run
//! use std::time::Duration;
//! use zwo_eaf::{connected_focusers, Focuser};
//!
//! let focusers = connected_focusers().unwrap();
//! for info in &focusers {
//!     println!("{}: {} (max step {})", info.id, info.name, info.max_step);
//! }
//!
//! if let Some(info) = focusers.first() {
//!     let eaf = Focuser::open(info.id).unwrap();
//!     println!("position {} at {:.1} °C", eaf.position().unwrap(), eaf.temperature().unwrap());
//!     eaf.move_to_and_wait(5000, Duration::from_secs(30)).unwrap();
//! }
//! ```
//!
//! # Threading
//!
//! [`Focuser`] is `Send` so it can be moved to a worker thread, but the SDK
//! keeps global per-ID state and is not documented as thread safe, so
//! `Focuser` is deliberately not `Sync`. Share it behind a `Mutex` if needed.
//!
//! # Bluetooth
//!
//! The SDK's Bluetooth LE API (`EAFBLE*`) is exposed by `zwo-eaf-sys` behind
//! its `bluetooth` feature but is not yet wrapped here.

mod error;
mod types;

pub use error::{Error, Result};
pub use types::*;

use std::mem::MaybeUninit;
use std::os::raw::{c_int, c_uchar};
use std::time::{Duration, Instant};

use error::check;
use zwo_eaf_sys as sys;

/// Returns the SDK version string, e.g. `"1, 8, 1"`.
pub fn sdk_version() -> Result<String> {
    types::cstr_to_string(unsafe { sys::EAFGetSDKVersion() }).ok_or(Error::NullString)
}

/// Number of focusers currently attached. Refreshes the SDK's device list.
pub fn focuser_count() -> usize {
    unsafe { sys::EAFGetNum() }.max(0) as usize
}

/// Enumerates every attached focuser.
///
/// The SDK sometimes reports `Moving` for a focuser that has just been
/// plugged in; this call retries the property read briefly before giving up.
pub fn connected_focusers() -> Result<Vec<FocuserInfo>> {
    let n = focuser_count();
    let mut out = Vec::with_capacity(n);
    for index in 0..n {
        let mut id: c_int = 0;
        check(unsafe { sys::EAFGetID(index as c_int, &mut id) })?;
        let deadline = Instant::now() + Duration::from_secs(2);
        let info = loop {
            match raw_property(id) {
                Err(Error::Moving) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                r => break r?,
            }
        };
        out.push(FocuserInfo::from(info));
    }
    Ok(out)
}

/// Returns `true` if the USB vendor/product pair belongs to an EAF.
pub fn is_eaf_device(vendor_id: u16, product_id: u16) -> bool {
    unsafe { sys::EAFCheck(vendor_id as c_int, product_id as c_int) == 1 }
}

fn raw_property(id: c_int) -> Result<sys::EAF_INFO> {
    let mut info = MaybeUninit::<sys::EAF_INFO>::uninit();
    check(unsafe { sys::EAFGetProperty(id, info.as_mut_ptr()) })?;
    Ok(unsafe { info.assume_init() })
}

/// An open focuser. Closed automatically on drop.
///
/// See the [crate-level docs](crate#threading) for thread-safety notes.
#[derive(Debug)]
pub struct Focuser {
    id: c_int,
    // SDK state is global and not documented thread-safe: allow moving the
    // handle between threads but not sharing references across them.
    _not_sync: std::marker::PhantomData<std::cell::Cell<()>>,
}

// PhantomData<Cell<()>> is !Sync but Send; make Send explicit for readers.
unsafe impl Send for Focuser {}

macro_rules! getter {
    ($(#[$m:meta])* $name:ident, $f:ident, $ty:ty) => {
        $(#[$m])*
        pub fn $name(&self) -> Result<$ty> {
            let mut v: $ty = Default::default();
            check(unsafe { sys::$f(self.id, &mut v) })?;
            Ok(v)
        }
    };
}

impl Focuser {
    /// Opens the focuser with the given SDK ID (see [`connected_focusers`]).
    pub fn open(id: i32) -> Result<Self> {
        check(unsafe { sys::EAFOpen(id) })?;
        Ok(Self {
            id,
            _not_sync: std::marker::PhantomData,
        })
    }

    /// The SDK ID this handle was opened with.
    pub fn id(&self) -> i32 {
        self.id
    }

    /// ID, name and maximum step of this focuser.
    pub fn info(&self) -> Result<FocuserInfo> {
        raw_property(self.id).map(FocuserInfo::from)
    }

    // --- Motion -----------------------------------------------------------

    /// Starts moving to an absolute position in `0..=max_step`. Returns
    /// immediately; poll [`is_moving`](Self::is_moving) or use
    /// [`move_to_and_wait`](Self::move_to_and_wait).
    pub fn move_to(&self, step: i32) -> Result<()> {
        check(unsafe { sys::EAFMove(self.id, step) })
    }

    /// Moves to `step` and blocks until the focuser stops or `timeout`
    /// elapses, polling every 50 ms. Returns [`Error::Timeout`] if the move
    /// is still in progress at the deadline (the focuser keeps moving).
    pub fn move_to_and_wait(&self, step: i32, timeout: Duration) -> Result<()> {
        self.move_to(step)?;
        let deadline = Instant::now() + timeout;
        loop {
            let (moving, _hand) = self.is_moving()?;
            if !moving {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Stops any motion. Returns immediately.
    pub fn stop(&self) -> Result<()> {
        check(unsafe { sys::EAFStop(self.id) })
    }

    /// Stops and blocks until the focuser reports idle or `timeout` elapses
    /// (returning [`Error::Moving`] in that case).
    pub fn stop_and_wait(&self, timeout: Duration) -> Result<()> {
        let ms = timeout.as_millis().min(c_int::MAX as u128) as c_int;
        check(unsafe { sys::EAFStopAndWait(self.id, ms) })
    }

    /// Returns `(moving, hand_control)`. `hand_control` is set when the
    /// motion was started from the hand controller and cannot be stopped
    /// by [`stop`](Self::stop).
    pub fn is_moving(&self) -> Result<(bool, bool)> {
        let mut moving = false;
        let mut hand = false;
        check(unsafe { sys::EAFIsMoving(self.id, &mut moving, &mut hand) })?;
        Ok((moving, hand))
    }

    getter! {
        /// Current position in steps.
        position, EAFGetPosition, c_int
    }

    /// Redefines the current physical position as `step` without moving.
    pub fn reset_position(&self, step: i32) -> Result<()> {
        check(unsafe { sys::EAFResetPostion(self.id, step) })
    }

    // --- Sensors ----------------------------------------------------------

    /// Temperature in °C. Returns [`Error::GeneralError`] if the reading is
    /// unusable (e.g. while being moved by hand).
    pub fn temperature(&self) -> Result<f32> {
        let mut t: f32 = 0.0;
        check(unsafe { sys::EAFGetTemp(self.id, &mut t) })?;
        Ok(t)
    }

    // --- Settings ---------------------------------------------------------

    getter! {
        /// Whether the focuser beeps when a move starts.
        beep, EAFGetBeep, bool
    }

    /// Enables or disables the start-of-move beep.
    pub fn set_beep(&self, on: bool) -> Result<()> {
        check(unsafe { sys::EAFSetBeep(self.id, on) })
    }

    getter! {
        /// Maximum position in steps. Returns [`Error::Moving`] while moving.
        max_step, EAFGetMaxStep, c_int
    }

    /// Sets the maximum position. Returns [`Error::Moving`] while moving.
    pub fn set_max_step(&self, max: i32) -> Result<()> {
        check(unsafe { sys::EAFSetMaxStep(self.id, max) })
    }

    getter! {
        /// Hardware position range, the upper bound for [`set_max_step`](Self::set_max_step).
        step_range, EAFStepRange, c_int
    }

    getter! {
        /// Whether the motor direction is reversed.
        reverse, EAFGetReverse, bool
    }

    /// Reverses (or restores) the motor direction.
    pub fn set_reverse(&self, reversed: bool) -> Result<()> {
        check(unsafe { sys::EAFSetReverse(self.id, reversed) })
    }

    getter! {
        /// Backlash compensation in steps.
        backlash, EAFGetBacklash, c_int
    }

    /// Sets backlash compensation, `0..=255` steps.
    pub fn set_backlash(&self, steps: i32) -> Result<()> {
        check(unsafe { sys::EAFSetBacklash(self.id, steps) })
    }

    getter! {
        /// LED state, `true` = on.
        led, EAFGetLedState, bool
    }

    /// Turns the LED on (`true`, its normal state) or off.
    pub fn set_led(&self, on: bool) -> Result<()> {
        check(unsafe { sys::EAFSetLedState(self.id, on) })
    }

    // --- Identity ---------------------------------------------------------

    /// Firmware version.
    pub fn firmware_version(&self) -> Result<FirmwareVersion> {
        let (mut major, mut minor, mut build): (c_uchar, c_uchar, c_uchar) = (0, 0, 0);
        check(unsafe { sys::EAFGetFirmwareVersion(self.id, &mut major, &mut minor, &mut build) })?;
        Ok(FirmwareVersion {
            major,
            minor,
            build,
        })
    }

    /// Serial number as a 16-character lowercase hex string.
    /// Returns [`Error::NotSupported`] on firmware without one.
    pub fn serial_number(&self) -> Result<String> {
        let mut sn = sys::EAF_SN { id: [0; 8] };
        check(unsafe { sys::EAFGetSerialNumber(self.id, &mut sn) })?;
        Ok(sn.id.iter().map(|b| format!("{b:02x}")).collect())
    }

    /// Sets the eight-byte alias. Longer input is truncated; shorter input
    /// is zero-padded. Returns [`Error::NotSupported`] on older firmware.
    pub fn set_alias(&self, alias: &[u8]) -> Result<()> {
        let mut id = sys::EAF_ID { id: [0; 8] };
        let n = alias.len().min(8);
        id.id[..n].copy_from_slice(&alias[..n]);
        check(unsafe { sys::EAFSetID(self.id, id) })
    }

    /// Model type string.
    pub fn focuser_type(&self) -> Result<String> {
        let mut t = MaybeUninit::<sys::EAF_TYPE>::uninit();
        check(unsafe { sys::EAFGetType(self.id, t.as_mut_ptr()) })?;
        Ok(types::chars_to_string(&unsafe { t.assume_init() }.r#type))
    }

    /// Bluetooth advertising name (USB connection only).
    pub fn ble_name(&self) -> Result<String> {
        let mut n = MaybeUninit::<sys::EAF_BLE_NAME>::uninit();
        check(unsafe { sys::EAFGetBLEName(self.id, n.as_mut_ptr()) })?;
        Ok(types::chars_to_string(&unsafe { n.assume_init() }.name))
    }

    /// Sets the last six characters of the Bluetooth name (1 to 6 ASCII
    /// characters). On success the device disconnects and must be rescanned.
    pub fn set_ble_name(&self, suffix: &str) -> Result<()> {
        if suffix.is_empty() || suffix.len() > 6 || !suffix.is_ascii() {
            return Err(Error::InvalidLength);
        }
        let mut n = sys::EAF_BLE_NAME { name: [0; 16] };
        for (dst, src) in n.name.iter_mut().zip(suffix.bytes()) {
            *dst = src as _;
        }
        check(unsafe { sys::EAFSetBLEName(self.id, n) })
    }

    // --- Diagnostics ------------------------------------------------------

    /// Motor and battery error codes. Returns [`Error::NotSupported`] on
    /// models without this feature.
    pub fn error_codes(&self) -> Result<ErrorCodes> {
        let mut m = MaybeUninit::<sys::EAF_ERROR_MSG>::uninit();
        check(unsafe { sys::EAFGetErrorCode(self.id, m.as_mut_ptr()) })?;
        Ok(ErrorCodes::from(unsafe { m.assume_init() }))
    }

    /// Battery telemetry. Returns [`Error::NotSupported`] on USB-powered
    /// models and [`Error::BatteryInfo`] if the battery temperature is
    /// abnormal.
    pub fn battery_info(&self) -> Result<BatteryInfo> {
        let mut b = sys::EAF_BATTERY_INFO::default();
        check(unsafe { sys::EAFGetBatteryInfo(self.id, &mut b) })?;
        Ok(BatteryInfo::from(b))
    }

    /// Reason the focuser last powered off.
    pub fn reason(&self) -> Result<PowerOffReason> {
        let mut r: c_int = 0;
        check(unsafe { sys::EAFGetReason(self.id, &mut r) })?;
        Ok(PowerOffReason::from(r))
    }

    /// Puts the focuser in shipping mode. ZWO notes this is currently unused.
    pub fn set_shipping_mode(&self) -> Result<()> {
        check(unsafe { sys::EAFSetShippingMode(self.id) })
    }

    /// Capabilities of every control the SDK reports for this focuser.
    pub fn control_caps(&self) -> Result<Vec<ControlCaps>> {
        let mut n: c_int = 0;
        check(unsafe { sys::EAFGetNumOfControls(self.id, &mut n) })?;
        let mut out = Vec::with_capacity(n.max(0) as usize);
        for i in 0..n.max(0) {
            let mut caps = MaybeUninit::<sys::EAF_CONTROL_CAPS>::uninit();
            check(unsafe { sys::EAFGetControlCaps(self.id, i, caps.as_mut_ptr()) })?;
            out.push(ControlCaps::from(unsafe { caps.assume_init() }));
        }
        Ok(out)
    }
}

impl Drop for Focuser {
    fn drop(&mut self) {
        // Nothing useful to do with a failure here.
        let _ = unsafe { sys::EAFClose(self.id) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focuser_is_send_not_sync() {
        fn assert_send<T: Send>() {}
        assert_send::<Focuser>();
        // Compile-time: `Focuser` must not be Sync. (Verified by the
        // PhantomData<Cell<()>> marker; a static assertion would need a
        // negative trait bound, so this is documented rather than asserted.)
    }

    #[test]
    fn sdk_version_is_nonempty() {
        let v = sdk_version().expect("SDK version");
        assert!(!v.is_empty());
        eprintln!("SDK version: {v}");
    }
}
