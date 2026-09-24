//! Scan for Bluetooth LE devices and, optionally, connect to one EAF and
//! print read-only state. Never moves the focuser.
//!
//! ```sh
//! cargo run -p zwo-eaf --features bluetooth --example ble_scan            # scan only
//! cargo run -p zwo-eaf --features bluetooth --example ble_scan -- "EAF Pro_90c92c"
//! ```
//!
//! Connecting also pairs, as ZWO's call sequence requires before any other
//! command.

use std::time::Duration;

use zwo_eaf::ble::{self, PairState};

fn main() -> zwo_eaf::Result<()> {
    let target = std::env::args().nth(1);

    println!("Scanning for 3 s...");
    let devices = ble::scan(Duration::from_secs(3))?;
    println!("{} device(s) found", devices.len());
    for d in &devices {
        println!(
            "  {} {:<24} {:<20} rssi={:>4} raw={:#x}",
            if d.is_eaf() { "*" } else { " " },
            format!("{:?}", d.name),
            d.address,
            d.signal_strength,
            d.bluetooth_address
        );
    }

    let Some(name) = target else {
        println!("(* = EAF; pass a device name to connect and read its state)");
        return Ok(());
    };
    let Some(dev) = devices.iter().find(|d| d.name == name) else {
        println!("{name:?} not found in scan");
        return Ok(());
    };

    let eaf = dev.connect()?;
    println!("\nConnected to {:?} as id {}", dev.name, eaf.id());
    eaf.set_connection_callback(|on| println!("  [callback] connected = {on}"))?;
    eaf.set_pair_callback(|s: PairState| println!("  [callback] pair state = {s:?}"))?;
    eaf.pair()?;
    std::thread::sleep(Duration::from_secs(1));

    let show = |label: &str, r: zwo_eaf::Result<String>| match r {
        Ok(v) => println!("  {label:<18}{v}"),
        Err(e) => println!("  {label:<18}<{e}>"),
    };
    show("all info", eaf.all_info().map(|a| format!("{a:#?}")));
    show("firmware", eaf.firmware_version().map(|v| v.to_string()));
    show("serial", eaf.serial_number());
    show("type", eaf.focuser_type());
    show("position", eaf.position().map(|v| v.to_string()));
    show(
        "temperature",
        eaf.temperature().map(|v| format!("{v:.1} °C")),
    );
    show(
        "battery",
        eaf.battery_info()
            .map(|b| format!("{}% {:?}", b.percentage, b)),
    );

    let _ = eaf.clear_connection_callback();
    let _ = eaf.clear_pair_callback();
    drop(eaf); // EAFBLEDisconnect
    ble::clear_callbacks();
    Ok(())
}
