# zwo-eaf

Safe Rust wrapper for the [ZWO](https://www.zwoastro.com/) EAF (Electronic
Automatic Focuser) SDK.

Provides an RAII `Focuser` handle (closed on drop), `Result`-based error
handling, and plain Rust types for every USB-accessible SDK feature. Built on
[`zwo-eaf-sys`](https://crates.io/crates/zwo-eaf-sys), which fetches the SDK
binaries at build time (or uses `ZWO_EAF_SDK_PATH`).

## Example

```rust
use std::time::Duration;
use zwo_eaf::{connected_focusers, Focuser};

fn main() -> zwo_eaf::Result<()> {
    for info in connected_focusers()? {
        let eaf = Focuser::open(info.id)?;
        println!("{} at step {} ({:.1} °C)", info.name, eaf.position()?, eaf.temperature()?);
        eaf.move_to_and_wait(5000, Duration::from_secs(30))?;
    }
    Ok(())
}
```

Run `cargo run --example list` to print the state of every attached focuser.

Only one `Focuser` per ID may be open at a time in a process, since dropping
a handle closes the SDK connection for that ID; a second `Focuser::open` of
the same ID returns `Error::AlreadyOpen`.

## Hardware tests

`cargo test` needs no hardware. The tests in `tests/focuser.rs` are ignored
by default, only read state, and never move the focuser. With no focuser
attached they print `no EAF focuser attached; skipping` and pass:

```sh
cargo test -p zwo-eaf -- --ignored --nocapture --test-threads=1
```

## Feature flags

| Feature | Default | Description |
|---------|---------|-------------|
| `bluetooth` | off | Bluetooth LE support: `ble::scan`, `ble::connect`, pairing, `Focuser::all_info` and connection/pairing callbacks. Enables `zwo-eaf-sys/bluetooth`. |

## Bluetooth

Battery-powered focusers (e.g. EAF Pro) can be controlled over Bluetooth LE
with the `bluetooth` feature:

```toml
zwo-eaf = { version = "0.1", features = ["bluetooth"] }
```

```rust
use std::time::Duration;
use zwo_eaf::ble;

fn main() -> zwo_eaf::Result<()> {
    let devices = ble::scan(Duration::from_secs(3))?;
    if let Some(dev) = devices.iter().find(|d| d.is_eaf()) {
        let eaf = dev.connect()?; // or ble::connect("EAF Pro_90c92c", None)
        eaf.pair()?;              // required before any other command
        let info = eaf.all_info()?;
        println!("{} at step {} ({:.1} °C)", dev.name, info.position, info.temperature);
    } // dropping the Focuser disconnects
    Ok(())
}
```

The connected handle is an ordinary `Focuser`; every method works over BLE
except `ble_name` (USB only). The BLE-only methods (`pair`, `clear_pair`,
`all_info`, callbacks) return `Error::NotSupported` on a USB handle.

Connection and pairing callbacks take closures, but the SDK's C callbacks
carry no device ID or user data, so each kind has **one process-wide slot**:
the last registration wins, and a closure cannot tell which device an event
came from. Closures run on SDK threads (`Send + Sync + 'static`); panics are
caught. Clear them with `Focuser::clear_*_callback` or `ble::clear_callbacks`.

Identify devices by name; on macOS the SDK does not report a usable address.

```sh
cargo run -p zwo-eaf --features bluetooth --example ble_scan                    # scan only
cargo run -p zwo-eaf --features bluetooth --example ble_scan -- "EAF Pro_90c92c" # connect, pair, read
cargo test -p zwo-eaf --features bluetooth --test ble -- --ignored --nocapture --test-threads=1
```

## License

MIT.
