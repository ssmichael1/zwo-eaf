//! Enumerate attached EAF focusers and print read-only state. Never moves
//! the focuser.

use zwo_eaf::{connected_focusers, sdk_version, Focuser};

fn main() -> zwo_eaf::Result<()> {
    println!("EAF SDK version: {}", sdk_version()?);

    let focusers = connected_focusers()?;
    if focusers.is_empty() {
        println!("No EAF focuser attached.");
        return Ok(());
    }

    for info in &focusers {
        println!(
            "\nFocuser id={} name={:?} max_step={}",
            info.id, info.name, info.max_step
        );
        let eaf = Focuser::open(info.id)?;

        let show = |label: &str, r: zwo_eaf::Result<String>| match r {
            Ok(v) => println!("  {label:<18}{v}"),
            Err(e) => println!("  {label:<18}<{e}>"),
        };

        show("firmware", eaf.firmware_version().map(|v| v.to_string()));
        show("serial", eaf.serial_number());
        show("type", eaf.focuser_type());
        show("position", eaf.position().map(|v| v.to_string()));
        show("max step", eaf.max_step().map(|v| v.to_string()));
        show("step range", eaf.step_range().map(|v| v.to_string()));
        show(
            "temperature",
            eaf.temperature().map(|v| format!("{v:.1} °C")),
        );
        show(
            "moving",
            eaf.is_moving()
                .map(|(m, h)| format!("{m} (hand control: {h})")),
        );
        show("beep", eaf.beep().map(|v| v.to_string()));
        show("reverse", eaf.reverse().map(|v| v.to_string()));
        show("backlash", eaf.backlash().map(|v| v.to_string()));
        show("led", eaf.led().map(|v| v.to_string()));
        show(
            "error codes",
            eaf.error_codes()
                .map(|e| format!("motor={} battery={}", e.motor, e.battery)),
        );
        show(
            "battery",
            eaf.battery_info()
                .map(|b| format!("{}% {:?}", b.percentage, b)),
        );
        show("power-off reason", eaf.reason().map(|r| format!("{r:?}")));

        match eaf.control_caps() {
            Ok(caps) => {
                println!("  controls:");
                for c in caps {
                    println!(
                        "    {:<16} {:?} supported={} writable={} range=[{}, {}] default={}",
                        c.name,
                        c.control_type,
                        c.is_supported,
                        c.is_writable,
                        c.min_value,
                        c.max_value,
                        c.default_value
                    );
                }
            }
            Err(e) => println!("  controls           <{e}>"),
        }
    }
    Ok(())
}
