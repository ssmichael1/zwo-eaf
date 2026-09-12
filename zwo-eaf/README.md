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

## Hardware tests

`cargo test` needs no hardware. The tests in `tests/focuser.rs` are ignored
by default, only read state, and never move the focuser:

```sh
cargo test -p zwo-eaf -- --ignored --nocapture --test-threads=1
```

## Future work

The SDK's Bluetooth LE API (scan, connect, pair, callbacks) is bound in
`zwo-eaf-sys` behind its `bluetooth` feature but not yet wrapped here.

## License

MIT.
