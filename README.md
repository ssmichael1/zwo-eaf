# zwo-eaf

Rust bindings for the [ZWO EAF](https://www.zwoastro.com/) Electronic
Automatic Focuser SDK (v1.8.1).

| Crate | Description |
|-------|-------------|
| [`zwo-eaf-sys`](zwo-eaf-sys) | Raw, unsafe FFI bindings to the C SDK. Fetches the SDK binaries at build time. |
| [`zwo-eaf`](zwo-eaf) | Safe wrapper: RAII `Focuser` handle, `Result` errors, Rust types. Most users want this one. |

## Supported platforms

| OS | Architectures |
|----|---------------|
| macOS | aarch64, x86_64 |
| Linux | x86_64, x86, armv6, armv7, aarch64 |
| Windows | x86_64, x86 |

## Building

The ZWO SDK binaries are MIT licensed but are not checked into this repo or
packaged in the crates. `zwo-eaf-sys`'s build script downloads a per-target
tarball from this repository's `sdk-1.8.1` GitHub release, verifies its
SHA-256, and caches it in `OUT_DIR`. To build offline, point it at an SDK you
downloaded from ZWO:

```sh
export ZWO_EAF_SDK_PATH=/path/to/EAF_SDK_V1.8.1/eaf     # Linux / macOS
set ZWO_EAF_SDK_PATH=C:\path\to\EAF_Windows_SDK_V1.8.1  # Windows
cargo build --release
cargo run --example list
```

An `EAF_SDK_V1.8.1/` checkout next to this repo (or next to any parent
directory) is also picked up automatically. See `zwo-eaf-sys/README.md` for
the full resolution order.

On macOS the stock SDK creates a Bluetooth `CBCentralManager` before `main`.
macOS kills the process at startup, with no output, unless the app that
launched it declares Bluetooth usage. Terminal.app does not. By default
`zwo-eaf-sys` removes the SDK's Bluetooth code, so USB use, `cargo test` and
the examples work from any terminal. With the `bluetooth` feature that code
is kept, and apps must add `NSBluetoothAlwaysUsageDescription` to their
bundle's `Info.plist`. See "macOS notes" in `zwo-eaf-sys/README.md`.

On Linux, install the SDK's udev rule so the HID device is accessible without
root:

```sh
sudo cp /path/to/EAF_SDK_V1.8.1/eaf/lib/eaf.rules /etc/udev/rules.d/99-eaf.rules
sudo udevadm control --reload-rules
```

## Cutting a new SDK release

When ZWO publishes a new SDK version `x.y.z`:

1. Download both the Linux/macOS tarball and the Windows zip from ZWO and
   extract them into one directory `EAF_SDK_Vx.y.z/` containing `eaf/` and
   `EAF_Windows_SDK_Vx.y.z/`.
2. Run `scripts/package-sdk.sh /path/to/EAF_SDK_Vx.y.z`. It writes one
   normalized tarball per target plus `SHA256SUMS` to `dist/` (gitignored).
3. Update `SDK_VERSION` and the `SDK_SHA256` table in `zwo-eaf-sys/build.rs`
   from `dist/SHA256SUMS`, and re-check the header for API changes.
4. Create a GitHub release tagged `sdk-x.y.z` and upload every
   `dist/eaf-sdk-x.y.z-*.tar.gz` as release assets. The tag is a data release
   and need not point at a crate version bump.
5. Verify with `ZWO_EAF_SDK_TARBALL=dist/eaf-sdk-x.y.z-<target>.tar.gz cargo build`
   before publishing the crates.

## License

MIT. The ZWO SDK is distributed by ZWO under the MIT license.
