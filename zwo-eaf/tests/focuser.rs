//! Integration tests requiring a physical ZWO EAF focuser.
//!
//! All tests are `#[ignore]`d so `cargo test` passes without hardware, and
//! none of them move the focuser. With no focuser attached each test prints
//! a note and passes. Run them with:
//!
//! ```sh
//! cargo test -p zwo-eaf -- --ignored --nocapture --test-threads=1
//! ```

use std::sync::{Mutex, MutexGuard};
use zwo_eaf::*;

/// Serializes tests: they all open the same focuser, and a second open of an
/// ID that is already open returns [`Error::AlreadyOpen`].
fn hardware() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// The first attached focuser, or `None` (after printing a skip note) if
/// nothing is plugged in.
fn first_info() -> Option<FocuserInfo> {
    let list = connected_focusers().expect("enumerate focusers");
    match list.into_iter().next() {
        Some(info) => {
            eprintln!("using {info:?}");
            Some(info)
        }
        None => {
            eprintln!("no EAF focuser attached; skipping");
            None
        }
    }
}

fn open_first() -> Option<Focuser> {
    first_info().map(|info| Focuser::open(info.id).expect("open focuser"))
}

#[test]
#[ignore]
fn enumerate() {
    let _hw = hardware();
    let list = connected_focusers().expect("enumerate focusers");
    if list.is_empty() {
        eprintln!("no EAF focuser attached; skipping");
        return;
    }
    for f in &list {
        eprintln!("id={} name={:?} max_step={}", f.id, f.name, f.max_step);
        assert!(f.max_step > 0);
    }
}

#[test]
#[ignore]
fn read_state() {
    let _hw = hardware();
    let Some(eaf) = open_first() else { return };
    let info = eaf.info().unwrap();
    let pos = eaf.position().unwrap();
    assert!(
        (0..=info.max_step).contains(&pos),
        "position {pos} outside 0..={}",
        info.max_step
    );
    let (moving, hand) = eaf.is_moving().unwrap();
    eprintln!("position={pos} moving={moving} hand={hand}");
    eprintln!("temperature={:?}", eaf.temperature());
    eprintln!("firmware={}", eaf.firmware_version().unwrap());
    eprintln!("serial={:?}", eaf.serial_number());
    eprintln!("type={:?}", eaf.focuser_type());
    eprintln!(
        "beep={:?} reverse={:?} backlash={:?} led={:?}",
        eaf.beep(),
        eaf.reverse(),
        eaf.backlash(),
        eaf.led()
    );
    eprintln!(
        "max_step={:?} step_range={:?}",
        eaf.max_step(),
        eaf.step_range()
    );
}

#[test]
#[ignore]
fn control_caps() {
    let _hw = hardware();
    let Some(eaf) = open_first() else { return };
    for c in eaf.control_caps().unwrap() {
        eprintln!("{c:?}");
    }
}

#[test]
#[ignore]
fn reopen_after_drop() {
    let _hw = hardware();
    let Some(info) = first_info() else { return };
    {
        let _a = Focuser::open(info.id).unwrap();
    }
    let b = Focuser::open(info.id).expect("reopen after drop");
    b.position().unwrap();
}

#[test]
#[ignore]
fn double_open_is_rejected() {
    let _hw = hardware();
    let Some(a) = open_first() else { return };
    assert_eq!(Focuser::open(a.id()).unwrap_err(), Error::AlreadyOpen);
    // The rejected open must leave the first handle working.
    a.position().expect("first handle still open");
}
