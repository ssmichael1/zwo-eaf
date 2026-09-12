//! Integration tests requiring a physical ZWO EAF focuser.
//!
//! All tests are `#[ignore]`d so `cargo test` passes without hardware, and
//! none of them move the focuser. Run them with:
//!
//! ```sh
//! cargo test -p zwo-eaf -- --ignored --nocapture --test-threads=1
//! ```

use zwo_eaf::*;

fn open_first() -> Focuser {
    let list = connected_focusers().expect("enumerate focusers");
    assert!(!list.is_empty(), "no EAF focuser attached");
    eprintln!("using {:?}", list[0]);
    Focuser::open(list[0].id).expect("open focuser")
}

#[test]
#[ignore]
fn enumerate() {
    let list = connected_focusers().expect("enumerate focusers");
    assert!(!list.is_empty(), "no EAF focuser attached");
    for f in &list {
        eprintln!("id={} name={:?} max_step={}", f.id, f.name, f.max_step);
        assert!(f.max_step > 0);
    }
}

#[test]
#[ignore]
fn read_state() {
    let eaf = open_first();
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
    let eaf = open_first();
    for c in eaf.control_caps().unwrap() {
        eprintln!("{c:?}");
    }
}

#[test]
#[ignore]
fn reopen_after_drop() {
    let list = connected_focusers().unwrap();
    let id = list[0].id;
    {
        let _a = Focuser::open(id).unwrap();
    }
    let b = Focuser::open(id).expect("reopen after drop");
    b.position().unwrap();
}
