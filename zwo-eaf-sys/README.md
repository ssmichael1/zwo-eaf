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
| 3 | Auto-discovery | `EAF_SDK_V1.8.1/eaf` (or the Windows dir) in the crate's parent directory or any ancestor, e.g. next to the workspace. |
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
| macOS | static `libEAFFocuser.a` plus IOKit, CoreFoundation, Foundation, Cocoa, AppKit and `libc++`; CoreBluetooth only with the `bluetooth` feature (see [macOS notes](#macos-notes)) |
| Linux | static `libEAFFocuser.a` plus `libstdc++` and `libdl` (hidapi is compiled in; the BLE helper is dlopened at runtime) |
| Windows | `EAF_focuser.dll` via its import library; the DLL is copied next to built executables |

On Linux install the SDK's `eaf.rules` udev rule for non-root access:

```
ACTION=="add", ATTRS{idVendor}=="03c3", ATTRS{idProduct}=="1f10", GROUP="users", MODE="0666"
```

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `bluetooth` | off | Exposes the `EAFBLE*` functions and callback types. On macOS this also keeps the SDK's Bluetooth LE code and links CoreBluetooth; read [macOS notes](#macos-notes) first. |

## macOS notes

### What the SDK does

The macOS `libEAFFocuser.a` contains a C++ static initializer
(`BluetoothLEManagerWin.o`: `g_pBleManager = new BluetoothLEManagerMac()`)
that creates a `CBCentralManager` and briefly spins the run loop **while dyld
loads the executable, before `main` runs**. The object always
ends up linked: `EAF.o`, which every SDK function lives in, references
`IBluetoothLEManager::CreateBluetoothLEManager()`, and that pulls in the
rest of the Bluetooth code.

Creating a `CBCentralManager` makes macOS privacy protection (TCC) check that
the process's **responsible app** declares `NSBluetoothAlwaysUsageDescription`
in its `Info.plist`. The responsible app is the GUI app that ultimately
launched the process (the terminal app for a command-line program, not the
binary itself). If the key is missing, TCC kills the process with `SIGABRT`
before `main`, printing nothing. The crash report
(`~/Library/Logs/DiagnosticReports/<binary>-*.ips`) says "attempted to access
privacy-sensitive data without a usage description" and names the
responsible process. This happens even if the program never touches
Bluetooth, and without an EAF attached.

| Launched from | Responsible app has the key? | Result with the unmodified SDK |
|---|---|---|
| Terminal.app | no | killed at startup |
| Claude Code, other agents/IDEs that lack the key | no | killed at startup |
| iTerm2, VS Code | yes | one-time "*app* would like to use Bluetooth" prompt, then runs |
| lldb, or anything else that makes the process its own responsible app | n/a (not a bundle) | runs |
| Your `.app` bundle | only if you add it | killed unless the key is in the bundle's `Info.plist` |

The Claude Code, lldb and test-bundle-without-the-key cases were
reproduced, along with a spawn that disclaims responsibility. The other rows
follow from which apps declare the key: Terminal.app's `Info.plist` does not;
iTerm2's and VS Code's do.

Embedding an `Info.plist` in the executable (`-sectcreate __TEXT
__info_plist`) does **not** help: TCC reads the responsible app's plist, not
the executable's.

### What this crate does about it

**Without the `bluetooth` feature (the default)** the build script removes
the five Bluetooth LE objects (`IBluetoothLEManager.o`,
`BluetoothLEManagerWin.o`, `BluetoothLEManagerMac.o`, `MacBLEManagerBridge.o`,
`MacCoreBluetoothImp.o`) from its private copy of `libEAFFocuser.a`, using the
system `ar` (override with `AR` or `AR_<target>`), and does not link
CoreBluetooth. The one symbol `EAF.o` still needs,
`CreateBluetoothLEManager()`, is supplied by a stub in this crate. Only
`EAFBLEScan` and `EAFBLEConnect` can reach it, and without the feature those
are not declared; if something calls them anyway, the stub prints a message
and aborts. USB programs, tests and examples then run anywhere and never touch
Bluetooth or TCC. If `ar` is unavailable, the build prints a warning and links
the unmodified archive.

**With the `bluetooth` feature** the SDK is linked unmodified, because its
BLE code needs that `CBCentralManager`. Every binary that links this crate
then needs a responsible app that declares Bluetooth usage:

- **GUI apps**: add `NSBluetoothAlwaysUsageDescription` (with a
  user-facing reason) to the app bundle's `Info.plist`. Sandboxed apps also
  need the `com.apple.security.device.bluetooth` entitlement.
- **Command-line tools and `cargo test`/`cargo run`**: run them from a
  terminal whose app declares the key (iTerm2, VS Code), not Terminal.app.
  Or make the tool its own responsible process. Running it via
  `lldb -- <binary>` does this (verified). Launchd jobs and ssh sessions
  should behave the same way.

## License

MIT. The ZWO SDK itself is distributed by ZWO under the MIT license.
