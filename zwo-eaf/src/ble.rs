//! Bluetooth LE transport (requires the `bluetooth` feature).
//!
//! # Workflow
//!
//! ZWO's recommended call sequence for Bluetooth is:
//!
//! 1. Power the focuser on and [`scan`] for nearby devices. The SDK reports
//!    every BLE advertiser in range; [`BleDevice::is_eaf`] picks out
//!    focusers.
//! 2. [`connect`] (or [`BleDevice::connect`]) to get an ordinary
//!    [`Focuser`]. `EAFGetNum`/`EAFOpen` are not used for BLE.
//! 3. Call [`Focuser::pair`] before any other command.
//! 4. Use the focuser as usual. Every [`Focuser`] method works over BLE except
//!    [`Focuser::ble_name`], which the SDK limits to USB.
//! 5. Drop the `Focuser` to disconnect (`EAFBLEDisconnect`).
//!
//! The BLE-only methods on [`Focuser`] ([`pair`](Focuser::pair),
//! [`clear_pair`](Focuser::clear_pair), [`all_info`](Focuser::all_info) and
//! the callback setters) return [`Error::NotSupported`] on a USB handle
//! without calling the SDK. Use [`Focuser::is_ble`] to check the transport.
//!
//! # Callbacks
//!
//! The SDK's connection and pairing callbacks are bare C function pointers
//! with no user-data argument and no device ID. This module therefore keeps
//! **one process-wide slot per callback kind**, mirroring the SDK API:
//!
//! - Registering a closure through any focuser replaces the closure
//!   registered through any other (last registration wins).
//! - A closure cannot tell which device an event came from. With more than
//!   one BLE focuser connected, treat events as "some focuser changed".
//! - A registered closure outlives its focuser. Remove it with
//!   [`Focuser::clear_connection_callback`] /
//!   [`Focuser::clear_pair_callback`] while connected, or with
//!   [`clear_callbacks`] at any time.
//!
//! Closures run on SDK-owned threads, so they must be
//! `Send + Sync + 'static`. Keep them short and **do not call [`Focuser`]
//! methods or [`scan`]/[`connect`] from them**: every SDK call takes a
//! process-wide lock (see the [crate docs](crate#threading)), and the thread
//! that triggered the event may be holding it while it waits for the SDK
//! thread running your closure, which would deadlock. Forward the event over
//! a channel instead. A closure may call [`clear_callbacks`], which does not
//! touch the SDK; the slot lock is not held while it runs. Panics are caught and discarded so they
//! never unwind into C.
//!
//! # Example
//!
//! ```no_run
//! use std::sync::mpsc;
//! use std::time::Duration;
//! use zwo_eaf::ble::{self, PairState};
//!
//! # fn main() -> zwo_eaf::Result<()> {
//! let dev = ble::scan(Duration::from_secs(3))?
//!     .into_iter()
//!     .find(|d| d.is_eaf())
//!     .expect("no EAF in range");
//! let eaf = dev.connect()?;
//!
//! let (tx, rx) = mpsc::sync_channel(8);
//! eaf.set_pair_callback(move |state: PairState| {
//!     let _ = tx.try_send(state);
//! })?;
//! eaf.pair()?;
//! if let Ok(state) = rx.recv_timeout(Duration::from_secs(10)) {
//!     println!("pairing: {state:?}");
//! }
//! println!("{:?}", eaf.all_info()?);
//! # Ok(())
//! # }
//! ```

use std::ffi::CString;
use std::mem::MaybeUninit;
use std::os::raw::c_int;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use zwo_eaf_sys as sys;

use crate::error::check;
use crate::types::chars_to_string;
use crate::{claim_id, BatteryInfo, Error, Focuser, Result, Transport};

/// Maximum number of devices a single [`scan`] returns. The SDK reports
/// every BLE advertiser in range (phones, watches, ...), so this is sized
/// generously; ZWO's own demo uses 100.
pub const MAX_SCAN_DEVICES: usize = 128;

// ---------------------------------------------------------------------------
// Scan and connect
// ---------------------------------------------------------------------------

/// A Bluetooth LE device found by [`scan`].
///
/// Identify devices by [`name`](Self::name): the address fields are platform
/// dependent. On macOS (SDK 1.8.1) `address` is empty and
/// `bluetooth_address` is not meaningful (the same value for every device).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BleDevice {
    /// Advertised name, e.g. `"EAF Pro_90c92c"`. Pass to [`connect`].
    pub name: String,
    /// Bluetooth address as formatted by the SDK; may be empty.
    pub address: String,
    /// Received signal strength as reported by the SDK (RSSI, normally
    /// dBm; closer to zero is stronger).
    pub signal_strength: i32,
    /// Numeric Bluetooth address as reported by the SDK; see the type docs.
    pub bluetooth_address: i64,
}

impl BleDevice {
    /// `true` if the advertised name marks this as a ZWO EAF (the SDK demo
    /// filters on the `"EAF"` prefix).
    pub fn is_eaf(&self) -> bool {
        self.name.starts_with("EAF")
    }

    /// Connects to this device by name, plus address when the SDK reported
    /// one; see [`connect`].
    pub fn connect(&self) -> Result<Focuser> {
        let address = (!self.address.is_empty()).then_some(self.address.as_str());
        connect(&self.name, address)
    }
}

impl From<sys::BLE_DEVICE_INFO_T> for BleDevice {
    fn from(d: sys::BLE_DEVICE_INFO_T) -> Self {
        Self {
            name: chars_to_string(&d.name),
            address: chars_to_string(&d.address),
            signal_strength: d.signalStrength,
            bluetooth_address: d.bluetoothAddress,
        }
    }
}

/// Converts a duration to whole milliseconds, saturating at `c_int::MAX`.
fn millis(d: Duration) -> c_int {
    d.as_millis().min(c_int::MAX as u128) as c_int
}

/// Scans for Bluetooth LE devices for `duration` (blocking) and returns up
/// to [`MAX_SCAN_DEVICES`] of them. Returns an empty list if nothing is in
/// range.
///
/// Returns [`Error::BleDeviceNotExists`] if the host has no usable
/// Bluetooth adapter.
pub fn scan(duration: Duration) -> Result<Vec<BleDevice>> {
    // Zero-initialised (a valid bit pattern for this plain C struct) rather
    // than `set_len` over uninitialised memory, in case the SDK fills an
    // entry only partially.
    let mut buf: Vec<sys::BLE_DEVICE_INFO_T> =
        vec![unsafe { std::mem::zeroed() }; MAX_SCAN_DEVICES];
    let mut found: c_int = 0;
    check(sdk!(EAFBLEScan(
        millis(duration),
        buf.as_mut_ptr(),
        MAX_SCAN_DEVICES as c_int,
        &mut found,
    )))?;
    buf.truncate(found.max(0) as usize);
    Ok(buf.into_iter().map(BleDevice::from).collect())
}

/// Connects to a focuser by advertised `name` and, optionally, `address`
/// (ZWO's examples pass none). The returned [`Focuser`] disconnects on drop.
/// Call [`Focuser::pair`] before issuing other commands.
///
/// Returns [`Error::InteriorNul`] if either string contains a NUL byte, and
/// [`Error::AlreadyOpen`] if a live [`Focuser`] already holds this device.
pub fn connect(name: &str, address: Option<&str>) -> Result<Focuser> {
    let name = CString::new(name).map_err(|_| Error::InteriorNul)?;
    let address = address
        .map(CString::new)
        .transpose()
        .map_err(|_| Error::InteriorNul)?;
    let address_ptr = address.as_ref().map_or(std::ptr::null(), |a| a.as_ptr());
    let mut id: c_int = 0;
    check(sdk!(EAFBLEConnect(name.as_ptr(), address_ptr, &mut id)))?;
    // The ID is only known after connecting. If another handle already owns
    // it, don't disconnect: the connection belongs to that handle.
    claim_id(id)?;
    Ok(Focuser::from_ble_id(id))
}

// ---------------------------------------------------------------------------
// All-info snapshot
// ---------------------------------------------------------------------------

/// Complete focuser state fetched in one BLE round trip by
/// [`Focuser::all_info`]. Field names follow the corresponding [`Focuser`]
/// getters.
#[derive(Debug, Clone, PartialEq)]
pub struct AllInfo {
    /// The motor is running.
    pub moving: bool,
    /// Backlash compensation in steps.
    pub backlash: i32,
    /// Current position in steps.
    pub position: i32,
    /// Temperature in °C.
    pub temperature: f32,
    /// The start-of-move beep (buzzer) is enabled.
    pub beep: bool,
    /// The motor direction is reversed.
    pub reverse: bool,
    /// A hand-controller button is pressed.
    pub hand_controller_pressed: bool,
    /// A hand controller is connected.
    pub hand_controller_connected: bool,
    /// Maximum position in steps.
    pub max_step: i32,
    /// The LED is on.
    pub led: bool,
    /// Two-character motor error code (`"E0"` none, `"E5"` motor stall).
    pub motor_error: String,
    /// Two-character battery error code (`"E0"` none, `"E6"`..`"E8"`
    /// temperature charge/discharge faults).
    pub battery_error: String,
    /// Battery telemetry.
    pub battery: BatteryInfo,
}

impl From<sys::EAF_ALL_INFO> for AllInfo {
    fn from(a: sys::EAF_ALL_INFO) -> Self {
        Self {
            moving: a.is_run != 0,
            backlash: a.backlash_steps,
            position: a.current_steps,
            temperature: a.temperature,
            beep: a.buzzer_state != 0,
            reverse: a.reverse_state != 0,
            hand_controller_pressed: a.handle_pressed != 0,
            hand_controller_connected: a.handle_connect != 0,
            max_step: a.max_steps,
            led: a.led_state != 0,
            // Not NUL-terminated; chars_to_string stops at the array end.
            motor_error: chars_to_string(&a.motor_error_code),
            battery_error: chars_to_string(&a.battery_error_code),
            battery: BatteryInfo::from(a.battery_info),
        }
    }
}

// ---------------------------------------------------------------------------
// Pairing state
// ---------------------------------------------------------------------------

/// Pairing outcome delivered to a [`Focuser::set_pair_callback`] closure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PairState {
    /// Pairing succeeded (`EAF_SUCCESS`).
    Paired,
    /// The focuser received a new pairing request
    /// (`EAF_BLE_NEW_PAIR_REQUEST`).
    NewPairRequest,
    /// Pairing timed out (`EAF_BLE_PAIRING_TIMEOUT`).
    Timeout,
    /// Pairing failed (`EAF_BLE_PAIR_FAILED`).
    Failed,
    /// Any other code the SDK reports.
    Other(i32),
}

impl From<c_int> for PairState {
    fn from(code: c_int) -> Self {
        match code {
            sys::EAF_SUCCESS => Self::Paired,
            sys::EAF_BLE_NEW_PAIR_REQUEST => Self::NewPairRequest,
            sys::EAF_BLE_PAIRING_TIMEOUT => Self::Timeout,
            sys::EAF_BLE_PAIR_FAILED => Self::Failed,
            other => Self::Other(other),
        }
    }
}

impl PairState {
    /// The raw SDK code.
    pub fn code(self) -> i32 {
        match self {
            Self::Paired => sys::EAF_SUCCESS,
            Self::NewPairRequest => sys::EAF_BLE_NEW_PAIR_REQUEST,
            Self::Timeout => sys::EAF_BLE_PAIRING_TIMEOUT,
            Self::Failed => sys::EAF_BLE_PAIR_FAILED,
            Self::Other(c) => c,
        }
    }

    /// `Ok(())` for [`Paired`](Self::Paired), otherwise the matching
    /// [`Error`] (e.g. [`Error::BlePairingTimeout`]).
    pub fn into_result(self) -> Result<()> {
        check(self.code())
    }
}

// ---------------------------------------------------------------------------
// Callback slots and trampolines
// ---------------------------------------------------------------------------

type ConnFn = dyn Fn(bool) + Send + Sync + 'static;
type PairFn = dyn Fn(PairState) + Send + Sync + 'static;
type Slot<T> = Mutex<Option<Arc<T>>>;

// Process-global because the SDK's C callbacks carry no user data or ID.
// Invariant: never held while user code runs (closures are cloned out, and
// displaced closures are dropped after the guard is released), so a closure
// that re-registers cannot deadlock and a panicking closure cannot poison it.
static CONN_CB: Slot<ConnFn> = Mutex::new(None);
static PAIR_CB: Slot<PairFn> = Mutex::new(None);

fn lock<T: ?Sized>(slot: &Slot<T>) -> MutexGuard<'_, Option<Arc<T>>> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Replaces the slot contents, returning the old closure so the caller drops
/// it after the lock is released.
fn swap<T: ?Sized>(slot: &Slot<T>, new: Option<Arc<T>>) -> Option<Arc<T>> {
    std::mem::replace(&mut *lock(slot), new)
}

/// Stores `new`, then runs `sdk_register`. If the SDK rejects the
/// registration, restores the previous closure unless a concurrent
/// registration has already replaced ours.
fn install<T: ?Sized>(
    slot: &Slot<T>,
    new: Arc<T>,
    sdk_register: impl FnOnce() -> c_int,
) -> Result<()> {
    let previous = swap(slot, Some(Arc::clone(&new)));
    let result = check(sdk_register());
    if result.is_err() {
        let mut guard = lock(slot);
        let still_ours = guard.as_ref().is_some_and(|cur| Arc::ptr_eq(cur, &new));
        let displaced = if still_ours {
            std::mem::replace(&mut *guard, previous)
        } else {
            previous
        };
        drop(guard);
        drop(displaced);
    }
    result
}

/// Calls the closure in `slot`, if any, without holding the lock and without
/// letting a panic escape.
fn dispatch<T: ?Sized>(slot: &Slot<T>, call: impl FnOnce(&T)) {
    // AssertUnwindSafe: after a panic nothing observed by the closure is used
    // again here, and the slot lock is not held while it runs.
    let result = catch_unwind(AssertUnwindSafe(|| {
        let callback = lock(slot).clone();
        if let Some(cb) = callback {
            call(&cb);
        }
    }));
    if let Err(payload) = result {
        // Dropping the payload could itself panic; leak it instead.
        std::mem::forget(payload);
    }
}

extern "C" fn conn_trampoline(state: bool) {
    dispatch(&CONN_CB, |cb| cb(state));
}

extern "C" fn pair_trampoline(state: sys::EAF_ERROR_CODE) {
    dispatch(&PAIR_CB, |cb| cb(PairState::from(state)));
}

/// Drops both process-global callback closures. Any registration still held
/// by the SDK becomes a no-op. Needs no connected focuser, so it can be used
/// after the focuser that registered them is gone.
pub fn clear_callbacks() {
    drop(swap(&CONN_CB, None));
    drop(swap(&PAIR_CB, None));
}

// ---------------------------------------------------------------------------
// BLE-only Focuser methods
// ---------------------------------------------------------------------------

impl Focuser {
    /// `true` if this handle is a Bluetooth LE connection.
    pub fn is_ble(&self) -> bool {
        self.transport == Transport::Ble
    }

    fn require_ble(&self) -> Result<()> {
        if self.is_ble() {
            Ok(())
        } else {
            Err(Error::NotSupported)
        }
    }

    /// Pairs with the focuser. ZWO requires this after [`connect`] and
    /// before any other command. The outcome is also reported to a
    /// [`set_pair_callback`](Self::set_pair_callback) closure, if any.
    ///
    /// Returns [`Error::NotSupported`] on a USB handle.
    pub fn pair(&self) -> Result<()> {
        self.require_ble()?;
        check(sdk!(EAFBLEPair(self.id)))
    }

    /// Clears the focuser's Bluetooth pairing.
    ///
    /// Returns [`Error::NotSupported`] on a USB handle.
    pub fn clear_pair(&self) -> Result<()> {
        self.require_ble()?;
        check(sdk!(EAFBLEClearPair(self.id)))
    }

    /// Complete focuser state in one BLE round trip.
    ///
    /// Returns [`Error::NotSupported`] on a USB handle or on firmware without
    /// this feature.
    pub fn all_info(&self) -> Result<AllInfo> {
        self.require_ble()?;
        let mut raw = MaybeUninit::<sys::EAF_ALL_INFO>::uninit();
        check(sdk!(EAFBLEgetAllInfo(self.id, raw.as_mut_ptr())))?;
        Ok(AllInfo::from(unsafe { raw.assume_init() }))
    }

    /// Registers `callback` to receive connection changes (`true` connected,
    /// `false` disconnected).
    ///
    /// The closure lives in a **process-global** slot: it replaces any
    /// connection callback registered through another focuser and is not
    /// told which device the event is for. See the
    /// [module docs](crate::ble#callbacks).
    ///
    /// Returns [`Error::NotSupported`] on a USB handle. If the SDK rejects the
    /// registration the previous closure is kept.
    pub fn set_connection_callback<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(bool) + Send + Sync + 'static,
    {
        self.require_ble()?;
        let trampoline = conn_trampoline as unsafe extern "C" fn(bool);
        install(&CONN_CB, Arc::new(callback), || {
            sdk!(EAFBLERegConnStateCallback(self.id, Some(trampoline)))
        })
    }

    /// Unregisters this focuser's connection callback with the SDK and drops
    /// the global closure. The closure is dropped even if the SDK call fails.
    ///
    /// Returns [`Error::NotSupported`] on a USB handle.
    pub fn clear_connection_callback(&self) -> Result<()> {
        self.require_ble()?;
        let result = check(sdk!(EAFBLERegConnStateCallback(self.id, None)));
        drop(swap(&CONN_CB, None));
        result
    }

    /// Registers `callback` to receive pairing outcomes.
    ///
    /// The closure lives in a **process-global** slot: it replaces any
    /// pairing callback registered through another focuser and is not told
    /// which device the event is for. See the
    /// [module docs](crate::ble#callbacks).
    ///
    /// Returns [`Error::NotSupported`] on a USB handle. If the SDK rejects the
    /// registration the previous closure is kept.
    pub fn set_pair_callback<F>(&self, callback: F) -> Result<()>
    where
        F: Fn(PairState) + Send + Sync + 'static,
    {
        self.require_ble()?;
        let trampoline = pair_trampoline as unsafe extern "C" fn(sys::EAF_ERROR_CODE);
        install(&PAIR_CB, Arc::new(callback), || {
            sdk!(EAFBLERegPairStateCallback(self.id, Some(trampoline)))
        })
    }

    /// Unregisters this focuser's pairing callback with the SDK and drops the
    /// global closure. The closure is dropped even if the SDK call fails.
    ///
    /// Returns [`Error::NotSupported`] on a USB handle.
    pub fn clear_pair_callback(&self) -> Result<()> {
        self.require_ble()?;
        let result = check(sdk!(EAFBLERegPairStateCallback(self.id, None)));
        drop(swap(&PAIR_CB, None));
        result
    }
}

// ---------------------------------------------------------------------------
// Tests (no hardware, no SDK calls)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::raw::c_char;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    /// The callback slots are process-global; serialise tests that use them.
    static SLOT_TESTS: Mutex<()> = Mutex::new(());

    fn slot_guard() -> MutexGuard<'static, ()> {
        SLOT_TESTS.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn chars<const N: usize>(s: &str) -> [c_char; N] {
        let mut out = [0 as c_char; N];
        for (d, b) in out.iter_mut().zip(s.bytes()) {
            *d = b as c_char;
        }
        out
    }

    /// A USB-transport handle built without calling the SDK. Must be
    /// `mem::forget`-ed so Drop does not call `EAFClose`.
    fn fake_usb() -> Focuser {
        Focuser {
            id: 0,
            transport: Transport::Usb,
            _not_sync: std::marker::PhantomData,
        }
    }

    #[test]
    fn millis_saturates() {
        assert_eq!(millis(Duration::from_millis(3000)), 3000);
        assert_eq!(millis(Duration::ZERO), 0);
        assert_eq!(millis(Duration::from_secs(u64::MAX)), c_int::MAX);
    }

    #[test]
    fn ble_device_from_raw() {
        let raw = sys::BLE_DEVICE_INFO_T {
            name: chars("EAF Pro_90c92c"),
            address: chars("AA:BB:CC:DD:EE:FF"),
            signalStrength: -61,
            bluetoothAddress: 0xAABB_CCDD_EEFF,
        };
        let d = BleDevice::from(raw);
        assert_eq!(d.name, "EAF Pro_90c92c");
        assert_eq!(d.address, "AA:BB:CC:DD:EE:FF");
        assert_eq!(d.signal_strength, -61);
        assert_eq!(d.bluetooth_address, 0xAABB_CCDD_EEFF);
        assert!(d.is_eaf());
        let phone = BleDevice {
            name: "Pixel 9".into(),
            ..d
        };
        assert!(!phone.is_eaf());
    }

    #[test]
    fn all_info_from_raw() {
        let raw = sys::EAF_ALL_INFO {
            is_run: 1,
            backlash_steps: 12,
            current_steps: 5000,
            temperature: 18.5,
            buzzer_state: 0,
            reverse_state: 2, // any non-zero is true
            handle_pressed: 0,
            handle_connect: 1,
            max_steps: 60000,
            led_state: 1,
            motor_error_code: chars("E5"), // no terminator
            battery_error_code: chars("E0"),
            battery_info: sys::EAF_BATTERY_INFO {
                battery_percentage: 87,
                ..Default::default()
            },
        };
        let a = AllInfo::from(raw);
        assert!(a.moving);
        assert_eq!(a.backlash, 12);
        assert_eq!(a.position, 5000);
        assert_eq!(a.temperature, 18.5);
        assert!(!a.beep);
        assert!(a.reverse);
        assert!(!a.hand_controller_pressed);
        assert!(a.hand_controller_connected);
        assert_eq!(a.max_step, 60000);
        assert!(a.led);
        assert_eq!(a.motor_error, "E5");
        assert_eq!(a.battery_error, "E0");
        assert_eq!(a.battery.percentage, 87);
    }

    #[test]
    fn pair_state_round_trips() {
        let cases = [
            (sys::EAF_SUCCESS, PairState::Paired),
            (sys::EAF_BLE_NEW_PAIR_REQUEST, PairState::NewPairRequest),
            (sys::EAF_BLE_PAIRING_TIMEOUT, PairState::Timeout),
            (sys::EAF_BLE_PAIR_FAILED, PairState::Failed),
            (99, PairState::Other(99)),
        ];
        for (code, state) in cases {
            assert_eq!(PairState::from(code), state);
            assert_eq!(state.code(), code);
        }
        assert_eq!(PairState::Paired.into_result(), Ok(()));
        assert_eq!(
            PairState::Timeout.into_result(),
            Err(Error::BlePairingTimeout)
        );
        assert_eq!(
            PairState::NewPairRequest.into_result(),
            Err(Error::BleNewPairRequest)
        );
    }

    #[test]
    fn connect_rejects_interior_nul_before_calling_sdk() {
        assert_eq!(connect("EAF\0Pro", None).unwrap_err(), Error::InteriorNul);
        assert_eq!(
            connect("EAF Pro", Some("AA:BB\0")).unwrap_err(),
            Error::InteriorNul
        );
    }

    #[test]
    fn transport_is_tracked() {
        let usb = fake_usb();
        assert!(!usb.is_ble());
        std::mem::forget(usb);
        let ble = Focuser::from_ble_id(sys::BLE_DEVICE_MIN_ID);
        assert!(ble.is_ble());
        std::mem::forget(ble);
    }

    #[test]
    fn usb_handle_rejects_ble_methods() {
        let _g = slot_guard();
        clear_callbacks();
        let f = fake_usb();
        assert_eq!(f.pair(), Err(Error::NotSupported));
        assert_eq!(f.clear_pair(), Err(Error::NotSupported));
        assert_eq!(f.all_info(), Err(Error::NotSupported));
        assert_eq!(f.set_connection_callback(|_| {}), Err(Error::NotSupported));
        assert_eq!(f.set_pair_callback(|_| {}), Err(Error::NotSupported));
        assert_eq!(f.clear_connection_callback(), Err(Error::NotSupported));
        assert_eq!(f.clear_pair_callback(), Err(Error::NotSupported));
        // Rejected before touching the global slots.
        assert!(lock(&CONN_CB).is_none());
        assert!(lock(&PAIR_CB).is_none());
        std::mem::forget(f);
    }

    #[test]
    fn trampolines_are_noops_when_empty() {
        let _g = slot_guard();
        clear_callbacks();
        conn_trampoline(true);
        pair_trampoline(sys::EAF_SUCCESS);
    }

    #[test]
    fn trampolines_deliver_events() {
        let _g = slot_guard();
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        swap(
            &PAIR_CB,
            Some(Arc::new(move |s: PairState| {
                tx.lock().unwrap().send(s).unwrap();
            })),
        );
        pair_trampoline(sys::EAF_BLE_PAIRING_TIMEOUT);
        assert_eq!(rx.try_recv(), Ok(PairState::Timeout));

        let hits = Arc::new(AtomicUsize::new(0));
        let h = Arc::clone(&hits);
        swap(
            &CONN_CB,
            Some(Arc::new(move |on: bool| {
                if on {
                    h.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        conn_trampoline(true);
        conn_trampoline(false);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        clear_callbacks();
    }

    #[test]
    fn trampolines_contain_panics() {
        let _g = slot_guard();
        swap(&CONN_CB, Some(Arc::new(|_: bool| panic!("conn boom"))));
        swap(&PAIR_CB, Some(Arc::new(|_: PairState| panic!("pair boom"))));
        conn_trampoline(true);
        pair_trampoline(sys::EAF_SUCCESS);
        // Slots remain usable (not poisoned) afterwards.
        let hits = Arc::new(AtomicUsize::new(0));
        let h = Arc::clone(&hits);
        swap(
            &CONN_CB,
            Some(Arc::new(move |_: bool| {
                h.fetch_add(1, Ordering::SeqCst);
            })),
        );
        conn_trampoline(false);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        clear_callbacks();
    }

    #[test]
    fn callback_can_reregister_without_deadlock() {
        let _g = slot_guard();
        let first = Arc::new(AtomicUsize::new(0));
        let second = Arc::new(AtomicUsize::new(0));
        let (f, s) = (Arc::clone(&first), Arc::clone(&second));
        swap(
            &CONN_CB,
            Some(Arc::new(move |_: bool| {
                f.fetch_add(1, Ordering::SeqCst);
                let s = Arc::clone(&s);
                // Re-register from inside the callback; this displaces
                // (and drops a reference to) the running closure.
                let _ = install(
                    &CONN_CB,
                    Arc::new(move |_: bool| {
                        s.fetch_add(1, Ordering::SeqCst);
                    }),
                    || sys::EAF_SUCCESS,
                );
            })),
        );

        // Run on another thread so a deadlock fails the test instead of
        // hanging it.
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::spawn(move || {
            conn_trampoline(true);
            conn_trampoline(true);
            let _ = done_tx.send(());
        });
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("callback re-registration deadlocked");
        assert_eq!(first.load(Ordering::SeqCst), 1);
        assert_eq!(second.load(Ordering::SeqCst), 1);
        clear_callbacks();
    }

    #[test]
    fn failed_registration_restores_previous() {
        let _g = slot_guard();
        let old: Arc<ConnFn> = Arc::new(|_| {});
        swap(&CONN_CB, Some(Arc::clone(&old)));
        let r = install(&CONN_CB, Arc::new(|_: bool| {}), || sys::EAF_BLE_DISCONNECT);
        assert_eq!(r, Err(Error::BleDisconnect));
        assert!(Arc::ptr_eq(lock(&CONN_CB).as_ref().unwrap(), &old));

        let new: Arc<ConnFn> = Arc::new(|_| {});
        assert_eq!(
            install(&CONN_CB, Arc::clone(&new), || sys::EAF_SUCCESS),
            Ok(())
        );
        assert!(Arc::ptr_eq(lock(&CONN_CB).as_ref().unwrap(), &new));
        clear_callbacks();
    }
}
