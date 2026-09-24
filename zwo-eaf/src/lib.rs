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
//! The SDK itself is not thread safe either (concurrent device enumeration
//! can abort the process), so every call this crate makes into it takes one
//! process-wide lock. Focusers on different threads are therefore safe to use
//! at once, but their SDK calls run one at a time, and a blocking call such
//! as [`Focuser::stop_and_wait`] (or a Bluetooth scan) holds up every other
//! thread until it returns. [`Focuser::move_to_and_wait`] polls, so it does
//! not.
//!
//! Only one `Focuser` per ID can be open at a time in a process: dropping a
//! handle closes the SDK connection for that ID, which would silently break
//! any other handle to it. A second [`Focuser::open`] of an open ID returns
//! [`Error::AlreadyOpen`].
//!
//! Bluetooth callbacks (see below) run on SDK-owned threads, so the closures
//! must be `Send + Sync + 'static`, and they must not call back into this
//! crate's SDK functions: the thread that triggered the callback may still
//! hold the SDK lock.
//!
//! # Feature flags
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `bluetooth` | off | Bluetooth LE support: the `ble` module (scan, connect) plus pairing, `all_info` and connection/pairing callbacks on [`Focuser`]. Enables `zwo-eaf-sys/bluetooth`. |
//!
//! # Bluetooth
//!
//! With the `bluetooth` feature, battery-powered focusers (e.g. EAF Pro) can
//! be controlled over Bluetooth LE. Scan with `ble::scan`, connect with
//! `ble::connect` or `BleDevice::connect`, then call `Focuser::pair` before
//! any other command. The result is an ordinary [`Focuser`]: every method
//! above works over BLE except [`ble_name`](Focuser::ble_name), which the SDK
//! limits to USB. BLE handles skip `EAFGetNum`/`EAFOpen` and are closed with
//! `EAFBLEDisconnect` on drop.
//!
//! ```no_run
//! # #[cfg(feature = "bluetooth")]
//! # fn main() -> zwo_eaf::Result<()> {
//! use std::time::Duration;
//! use zwo_eaf::ble;
//!
//! let devices = ble::scan(Duration::from_secs(3))?;
//! if let Some(dev) = devices.iter().find(|d| d.is_eaf()) {
//!     let eaf = dev.connect()?;
//!     eaf.pair()?;
//!     let info = eaf.all_info()?;
//!     println!("{} at step {} ({:.1} °C)", dev.name, info.position, info.temperature);
//! }
//! # Ok(())
//! # }
//! # #[cfg(not(feature = "bluetooth"))]
//! # fn main() {}
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]

/// Calls `zwo_eaf_sys::$f(args)` with the process-wide SDK lock held; see
/// [`with_sdk`]. Defined before the modules so they can use it too.
macro_rules! sdk {
    ($f:ident($($arg:expr),* $(,)?)) => {
        $crate::with_sdk(|| unsafe { ::zwo_eaf_sys::$f($($arg),*) })
    };
}

mod error;
mod types;

#[cfg(feature = "bluetooth")]
#[cfg_attr(docsrs, doc(cfg(feature = "bluetooth")))]
pub mod ble;

pub use error::{Error, Result};
pub use types::*;

use std::cell::Cell;
use std::collections::BTreeSet;
use std::mem::MaybeUninit;
use std::os::raw::{c_int, c_uchar};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use error::check;
use zwo_eaf_sys as sys;

// --- SDK lock ---------------------------------------------------------------

/// Serializes every call into the SDK. Its HID layer is not thread safe:
/// e.g. two threads in `EAFGetNum` at once can abort the process inside
/// `hid_init`.
static SDK_LOCK: Mutex<()> = Mutex::new(());

thread_local! {
    /// Whether this thread holds [`SDK_LOCK`], so a nested call (e.g. from a
    /// Bluetooth callback the SDK runs synchronously on the calling thread)
    /// proceeds instead of deadlocking.
    static SDK_LOCK_HELD: Cell<bool> = const { Cell::new(false) };
}

/// Runs `f` while holding the process-wide SDK lock. Re-entrant on the same
/// thread. The lock is held for the whole call, so blocking SDK functions
/// (`EAFStopAndWait`, `EAFBLEScan`) hold it until they return.
pub(crate) fn with_sdk<R>(f: impl FnOnce() -> R) -> R {
    if SDK_LOCK_HELD.with(Cell::get) {
        return f();
    }
    // The lock guards no data, so a panic in `f` cannot leave anything
    // inconsistent and a poisoned lock is still usable.
    let _guard = SDK_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    // Declared after `_guard`, so dropped (and the flag cleared) first, also
    // when `f` panics.
    struct Held;
    impl Drop for Held {
        fn drop(&mut self) {
            SDK_LOCK_HELD.with(|h| h.set(false));
        }
    }
    SDK_LOCK_HELD.with(|h| h.set(true));
    let _held = Held;
    f()
}

/// Returns the SDK version string, e.g. `"1, 8, 1"`.
pub fn sdk_version() -> Result<String> {
    // Copy the SDK-owned string before releasing the lock.
    with_sdk(|| types::cstr_to_string(unsafe { sys::EAFGetSDKVersion() })).ok_or(Error::NullString)
}

/// Number of focusers currently attached. Refreshes the SDK's device list.
pub fn focuser_count() -> usize {
    sdk!(EAFGetNum()).max(0) as usize
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
        check(sdk!(EAFGetID(index as c_int, &mut id)))?;
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
    sdk!(EAFCheck(vendor_id as c_int, product_id as c_int)) == 1
}

fn raw_property(id: c_int) -> Result<sys::EAF_INFO> {
    let mut info = MaybeUninit::<sys::EAF_INFO>::uninit();
    check(sdk!(EAFGetProperty(id, info.as_mut_ptr())))?;
    Ok(unsafe { info.assume_init() })
}

// --- Open-ID registry -------------------------------------------------------

/// IDs with a live handle in this process. Guards against two handles to the
/// same ID, where dropping either would `EAFClose` the other.
static OPEN_IDS: Mutex<BTreeSet<c_int>> = Mutex::new(BTreeSet::new());

fn open_ids() -> MutexGuard<'static, BTreeSet<c_int>> {
    // A panic while holding the lock cannot leave the set half-updated, so a
    // poisoned registry is still valid.
    OPEN_IDS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Marks `id` as open, or returns [`Error::AlreadyOpen`] if it already is.
/// Call before opening the SDK connection and pair with [`release_id`].
pub(crate) fn claim_id(id: c_int) -> Result<()> {
    if open_ids().insert(id) {
        Ok(())
    } else {
        Err(Error::AlreadyOpen)
    }
}

/// Marks `id` as closed. Call after the SDK connection is closed (or failed
/// to open). Releasing an unclaimed ID is a no-op.
pub(crate) fn release_id(id: c_int) {
    open_ids().remove(&id);
}

// --- Transport (BLE support) -------------------------------------------------
/// How a [`Focuser`] handle was opened, which decides how it is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transport {
    /// Opened with `EAFOpen`; closed with `EAFClose`.
    Usb,
    /// Connected with `EAFBLEConnect`; closed with `EAFBLEDisconnect`.
    #[cfg(feature = "bluetooth")]
    Ble,
}
// --- end Transport -----------------------------------------------------------

/// An open focuser, connected over USB ([`Focuser::open`]) or, with the
/// `bluetooth` feature, Bluetooth LE. Closed automatically on drop.
///
/// See the [crate-level docs](crate#threading) for thread-safety notes.
#[derive(Debug)]
pub struct Focuser {
    id: c_int,
    transport: Transport,
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
            check(sdk!($f(self.id, &mut v)))?;
            Ok(v)
        }
    };
}

impl Focuser {
    /// Opens the focuser with the given SDK ID (see [`connected_focusers`]).
    ///
    /// Returns [`Error::AlreadyOpen`] if another `Focuser` for `id` is still
    /// alive in this process.
    pub fn open(id: i32) -> Result<Self> {
        claim_id(id)?;
        if let Err(e) = check(sdk!(EAFOpen(id))) {
            release_id(id);
            return Err(e);
        }
        Ok(Self {
            id,
            transport: Transport::Usb,
            _not_sync: std::marker::PhantomData,
        })
    }

    /// Wraps an ID returned by a successful `EAFBLEConnect`. The handle
    /// takes ownership of the connection and disconnects it on drop.
    #[cfg(feature = "bluetooth")]
    pub(crate) fn from_ble_id(id: c_int) -> Self {
        Self {
            id,
            transport: Transport::Ble,
            _not_sync: std::marker::PhantomData,
        }
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
        check(sdk!(EAFMove(self.id, step)))
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
        check(sdk!(EAFStop(self.id)))
    }

    /// Stops and blocks until the focuser reports idle or `timeout` elapses
    /// (returning [`Error::Moving`] in that case).
    pub fn stop_and_wait(&self, timeout: Duration) -> Result<()> {
        let ms = timeout.as_millis().min(c_int::MAX as u128) as c_int;
        check(sdk!(EAFStopAndWait(self.id, ms)))
    }

    /// Returns `(moving, hand_control)`. `hand_control` is set when the
    /// motion was started from the hand controller and cannot be stopped
    /// by [`stop`](Self::stop).
    pub fn is_moving(&self) -> Result<(bool, bool)> {
        let mut moving = false;
        let mut hand = false;
        check(sdk!(EAFIsMoving(self.id, &mut moving, &mut hand)))?;
        Ok((moving, hand))
    }

    getter! {
        /// Current position in steps.
        position, EAFGetPosition, c_int
    }

    /// Redefines the current physical position as `step` without moving.
    pub fn reset_position(&self, step: i32) -> Result<()> {
        check(sdk!(EAFResetPostion(self.id, step)))
    }

    // --- Sensors ----------------------------------------------------------

    /// Temperature in °C. Returns [`Error::GeneralError`] if the reading is
    /// unusable (e.g. while being moved by hand).
    pub fn temperature(&self) -> Result<f32> {
        let mut t: f32 = 0.0;
        check(sdk!(EAFGetTemp(self.id, &mut t)))?;
        Ok(t)
    }

    // --- Settings ---------------------------------------------------------

    getter! {
        /// Whether the focuser beeps when a move starts.
        beep, EAFGetBeep, bool
    }

    /// Enables or disables the start-of-move beep.
    pub fn set_beep(&self, on: bool) -> Result<()> {
        check(sdk!(EAFSetBeep(self.id, on)))
    }

    getter! {
        /// Maximum position in steps. Returns [`Error::Moving`] while moving.
        max_step, EAFGetMaxStep, c_int
    }

    /// Sets the maximum position. Returns [`Error::Moving`] while moving.
    pub fn set_max_step(&self, max: i32) -> Result<()> {
        check(sdk!(EAFSetMaxStep(self.id, max)))
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
        check(sdk!(EAFSetReverse(self.id, reversed)))
    }

    getter! {
        /// Backlash compensation in steps.
        backlash, EAFGetBacklash, c_int
    }

    /// Sets backlash compensation, `0..=255` steps.
    pub fn set_backlash(&self, steps: i32) -> Result<()> {
        check(sdk!(EAFSetBacklash(self.id, steps)))
    }

    getter! {
        /// LED state, `true` = on.
        led, EAFGetLedState, bool
    }

    /// Turns the LED on (`true`, its normal state) or off.
    pub fn set_led(&self, on: bool) -> Result<()> {
        check(sdk!(EAFSetLedState(self.id, on)))
    }

    // --- Identity ---------------------------------------------------------

    /// Firmware version.
    pub fn firmware_version(&self) -> Result<FirmwareVersion> {
        let (mut major, mut minor, mut build): (c_uchar, c_uchar, c_uchar) = (0, 0, 0);
        check(sdk!(EAFGetFirmwareVersion(
            self.id, &mut major, &mut minor, &mut build
        )))?;
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
        check(sdk!(EAFGetSerialNumber(self.id, &mut sn)))?;
        Ok(sn.id.iter().map(|b| format!("{b:02x}")).collect())
    }

    /// Sets the eight-byte alias. Longer input is truncated; shorter input
    /// is zero-padded. Returns [`Error::NotSupported`] on older firmware.
    pub fn set_alias(&self, alias: &[u8]) -> Result<()> {
        let mut id = sys::EAF_ID { id: [0; 8] };
        let n = alias.len().min(8);
        id.id[..n].copy_from_slice(&alias[..n]);
        check(sdk!(EAFSetID(self.id, id)))
    }

    /// Model type string.
    pub fn focuser_type(&self) -> Result<String> {
        let mut t = MaybeUninit::<sys::EAF_TYPE>::uninit();
        check(sdk!(EAFGetType(self.id, t.as_mut_ptr())))?;
        Ok(types::chars_to_string(&unsafe { t.assume_init() }.r#type))
    }

    /// Bluetooth advertising name (USB connection only).
    pub fn ble_name(&self) -> Result<String> {
        let mut n = MaybeUninit::<sys::EAF_BLE_NAME>::uninit();
        check(sdk!(EAFGetBLEName(self.id, n.as_mut_ptr())))?;
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
        check(sdk!(EAFSetBLEName(self.id, n)))
    }

    // --- Diagnostics ------------------------------------------------------

    /// Motor and battery error codes. Returns [`Error::NotSupported`] on
    /// models without this feature.
    pub fn error_codes(&self) -> Result<ErrorCodes> {
        let mut m = MaybeUninit::<sys::EAF_ERROR_MSG>::uninit();
        check(sdk!(EAFGetErrorCode(self.id, m.as_mut_ptr())))?;
        Ok(ErrorCodes::from(unsafe { m.assume_init() }))
    }

    /// Battery telemetry. Returns [`Error::NotSupported`] on USB-powered
    /// models and [`Error::BatteryInfo`] if the battery temperature is
    /// abnormal.
    pub fn battery_info(&self) -> Result<BatteryInfo> {
        let mut b = sys::EAF_BATTERY_INFO::default();
        check(sdk!(EAFGetBatteryInfo(self.id, &mut b)))?;
        Ok(BatteryInfo::from(b))
    }

    /// Reason the focuser last powered off.
    pub fn reason(&self) -> Result<PowerOffReason> {
        let mut r: c_int = 0;
        check(sdk!(EAFGetReason(self.id, &mut r)))?;
        Ok(PowerOffReason::from(r))
    }

    /// Puts the focuser in shipping mode. ZWO notes this is currently unused.
    pub fn set_shipping_mode(&self) -> Result<()> {
        check(sdk!(EAFSetShippingMode(self.id)))
    }

    /// Capabilities of every control the SDK reports for this focuser.
    pub fn control_caps(&self) -> Result<Vec<ControlCaps>> {
        let mut n: c_int = 0;
        check(sdk!(EAFGetNumOfControls(self.id, &mut n)))?;
        let mut out = Vec::with_capacity(n.max(0) as usize);
        for i in 0..n.max(0) {
            let mut caps = MaybeUninit::<sys::EAF_CONTROL_CAPS>::uninit();
            check(sdk!(EAFGetControlCaps(self.id, i, caps.as_mut_ptr())))?;
            out.push(ControlCaps::from(unsafe { caps.assume_init() }));
        }
        Ok(out)
    }
}

impl Drop for Focuser {
    fn drop(&mut self) {
        // Nothing useful to do with a failure here.
        let _ = match self.transport {
            Transport::Usb => sdk!(EAFClose(self.id)),
            #[cfg(feature = "bluetooth")]
            Transport::Ble => sdk!(EAFBLEDisconnect(self.id)),
        };
        release_id(self.id);
    }
}

/// Compile-time checks, run as doctests.
///
/// `Focuser` must not be `Sync`:
///
/// ```compile_fail,E0277
/// fn assert_sync<T: Sync>() {}
/// assert_sync::<zwo_eaf::Focuser>();
/// ```
///
/// The same harness compiles for `Send`, so the failure above comes from the
/// `Sync` bound and not a typo:
///
/// ```no_run
/// fn assert_send<T: Send>() {}
/// assert_send::<zwo_eaf::Focuser>();
/// ```
#[allow(dead_code)]
mod static_assertions {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focuser_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Focuser>();
        // !Sync is checked by the compile_fail doctest on `static_assertions`.
    }

    // Registry tests use IDs far above anything the SDK hands out, and a
    // distinct ID per test since tests share the registry and run in parallel.

    #[test]
    fn claim_rejects_second_claim() {
        claim_id(1000).unwrap();
        assert_eq!(claim_id(1000), Err(Error::AlreadyOpen));
        release_id(1000);
    }

    #[test]
    fn release_allows_reclaim() {
        claim_id(1001).unwrap();
        release_id(1001);
        claim_id(1001).expect("reclaim after release");
        release_id(1001);
    }

    #[test]
    fn claims_are_per_id() {
        claim_id(1002).unwrap();
        claim_id(1003).expect("different ID is independent");
        release_id(1002);
        release_id(1003);
    }

    #[test]
    fn release_unclaimed_is_noop() {
        release_id(1004);
        claim_id(1004).unwrap();
        release_id(1004);
    }

    #[test]
    fn registry_survives_poison() {
        claim_id(1005).unwrap();
        let _ = std::thread::spawn(|| {
            let _guard = open_ids();
            panic!("deliberately poisoning the open-ID registry");
        })
        .join();
        assert_eq!(claim_id(1005), Err(Error::AlreadyOpen));
        release_id(1005);
        claim_id(1005).expect("reclaim after poison");
        release_id(1005);
    }

    #[test]
    fn open_of_claimed_id_fails_before_sdk_call() {
        // The registry is checked first, so this never reaches the SDK.
        claim_id(1006).unwrap();
        assert_eq!(Focuser::open(1006).unwrap_err(), Error::AlreadyOpen);
        // The rejected open must not release the existing claim.
        assert_eq!(claim_id(1006), Err(Error::AlreadyOpen));
        release_id(1006);
    }

    #[test]
    fn sdk_version_is_nonempty() {
        let v = sdk_version().expect("SDK version");
        assert!(!v.is_empty());
        eprintln!("SDK version: {v}");
    }

    // SDK lock tests. They share the lock with every other test that calls
    // the SDK, which only makes them slower, not flaky.

    #[test]
    fn sdk_lock_is_reentrant() {
        // On another thread so a deadlock fails the test instead of hanging.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(with_sdk(|| with_sdk(|| 7)));
        });
        let v = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("nested with_sdk deadlocked");
        assert_eq!(v, 7);
    }

    #[test]
    fn sdk_lock_serializes_threads() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        static INSIDE: AtomicBool = AtomicBool::new(false);
        static OVERLAPS: AtomicUsize = AtomicUsize::new(0);
        let threads: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    for _ in 0..20 {
                        with_sdk(|| {
                            if INSIDE.swap(true, Ordering::SeqCst) {
                                OVERLAPS.fetch_add(1, Ordering::SeqCst);
                            }
                            std::thread::yield_now();
                            INSIDE.store(false, Ordering::SeqCst);
                        });
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(OVERLAPS.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn sdk_lock_recovers_from_panic() {
        let r =
            std::panic::catch_unwind(|| with_sdk(|| panic!("deliberate panic under the SDK lock")));
        assert!(r.is_err());
        // The flag was cleared on unwind, so this thread locks normally...
        assert!(!SDK_LOCK_HELD.with(Cell::get));
        // ...and the (poisoned) lock is still usable from other threads.
        let v = std::thread::spawn(|| with_sdk(|| 3)).join().unwrap();
        assert_eq!(v, 3);
    }
}
