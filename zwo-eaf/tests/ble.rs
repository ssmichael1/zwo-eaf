//! Integration tests requiring a Bluetooth adapter and a powered-on,
//! BLE-capable ZWO EAF in range. Built only with the `bluetooth` feature.
//!
//! All tests are `#[ignore]`d and none of them move the focuser. Connecting
//! pairs with the device, as ZWO's call sequence requires. Run them with:
//!
//! ```sh
//! cargo test -p zwo-eaf --features bluetooth --test ble -- --ignored --nocapture --test-threads=1
//! ```

use std::time::Duration;

use zwo_eaf::ble;

fn find_eaf() -> ble::BleDevice {
    let devices = ble::scan(Duration::from_secs(3)).expect("BLE scan");
    devices
        .into_iter()
        .find(|d| d.is_eaf())
        .expect("no EAF advertising in range")
}

#[test]
#[ignore]
fn scan_lists_devices() {
    let devices = ble::scan(Duration::from_secs(3)).expect("BLE scan");
    for d in &devices {
        eprintln!("{d:?}");
    }
    assert!(devices.len() <= ble::MAX_SCAN_DEVICES);
}

#[test]
#[ignore]
fn connect_pair_and_read() {
    let dev = find_eaf();
    eprintln!("using {dev:?}");
    let eaf = dev.connect().expect("connect");
    assert!(eaf.is_ble());
    assert!(eaf.id() >= zwo_eaf_sys::BLE_DEVICE_MIN_ID);
    eaf.pair().expect("pair");

    let all = eaf.all_info().expect("all_info");
    eprintln!("{all:#?}");
    assert!(
        (0..=all.max_step).contains(&all.position),
        "position {} outside 0..={}",
        all.position,
        all.max_step
    );
    // The single-call snapshot agrees with the individual getters.
    assert_eq!(eaf.position().expect("position"), all.position);
    assert_eq!(eaf.max_step().expect("max_step"), all.max_step);
}
