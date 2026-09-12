# zwo-eaf-sys

Raw FFI bindings to the [ZWO](https://www.zwoastro.com/) EAF (Electronic
Automatic Focuser) C SDK, v1.8.1.

This crate exposes every function, struct and constant from `EAF_focuser.h`
as unsafe `extern "C"` declarations. For a safe, idiomatic API use the
[`zwo-eaf`](https://crates.io/crates/zwo-eaf) crate instead.

## SDK resolution

The SDK binaries are MIT licensed but are **not vendored** in this crate or
its git repository. The build script resolves them in this order:

| Order | Source | Notes |
|-------|--------|-------|
| 1 | `ZWO_EAF_SDK_PATH` | Root of an extracted ZWO SDK: the `eaf/` directory on Linux/macOS, `EAF_Windows_SDK_V1.8.1/` on Windows. The normalized tarball layout is also accepted. |
| 2 | `ZWO_EAF_SDK_TARBALL` | Local copy of the per-target tarball from the GitHub release. Verified and extracted the same way as a download. |
| 3 | Sibling checkout | `../EAF_SDK_V1.8.1/eaf` (or the Windows dir) next to the crate or workspace. |
| 4 | Download | `https://github.com/ssmichael1/zwo-eaf/releases/download/sdk-1.8.1/eaf-sdk-1.8.1-<target>.tar.gz`, SHA-256 verified against a table in `build.rs`, cached in `OUT_DIR`. |

Targets: `macos-aarch64`, `macos-x86_64`, `linux-x86_64`, `linux-x86`,
`linux-armv6`, `linux-armv7`, `linux-aarch64`, `windows-x86_64`, `windows-x86`.

```sh
# Offline build with a downloaded ZWO SDK
export ZWO_EAF_SDK_PATH=/path/to/EAF_SDK_V1.8.1/eaf
cargo build
```

## Linking

| OS | Linkage |
|----|---------|
| macOS | static `libEAFFocuser.a` plus IOKit, CoreFoundation, Foundation, Cocoa, AppKit, CoreBluetooth and `libc++` |
| Linux | static `libEAFFocuser.a` plus `libstdc++` and `libdl` (hidapi is compiled in; the BLE helper is dlopened at runtime) |
| Windows | `EAF_focuser.dll` via its import library; the DLL is copied next to built executables |

On Linux install the SDK's `eaf.rules` udev rule for non-root access:

```
ACTION=="add", ATTRS{idVendor}=="03c3", ATTRS{idProduct}=="1f10", GROUP="users", MODE="0666"
```

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `bluetooth` | off | Exposes the `EAFBLE*` functions and callback types |

## License

MIT. The ZWO SDK itself is distributed by ZWO under the MIT license.
