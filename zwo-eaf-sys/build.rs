//! Build script for `zwo-eaf-sys`.
//!
//! Locates the ZWO EAF SDK (header + prebuilt library) for the current target
//! and emits the link directives. The SDK binaries are **not** vendored in the
//! crate; they are resolved in this order:
//!
//! 1. `ZWO_EAF_SDK_PATH` — an extracted ZWO SDK root (`eaf/` on Unix, the
//!    `EAF_Windows_SDK_Vx.y.z/` dir on Windows), *or* a directory in the
//!    normalized tarball layout (`include/`, `lib/` flat).
//! 2. `ZWO_EAF_SDK_TARBALL` — a local copy of the per-target tarball published
//!    on the GitHub release; verified and extracted exactly like a download.
//! 3. Auto-discovery of an `EAF_SDK_V1.8.1/` checkout next to the crate or
//!    any of its parent directories (so a sibling of the workspace is found).
//! 4. Download of the per-target tarball from the `sdk-1.8.1` GitHub release,
//!    SHA-256 verified and cached in `OUT_DIR`.
//!
//! On macOS without the `bluetooth` feature the staged copy of
//! `libEAFFocuser.a` has its Bluetooth LE objects removed; see
//! [`strip_macos_ble`] for why.
//!
//! When `DOCS_RS` is set all SDK work is skipped so the docs build offline.

use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// SDK release this crate is pinned to. Bump together with `SDK_SHA256`.
const SDK_VERSION: &str = "1.8.1";
const RELEASE_BASE: &str = "https://github.com/ssmichael1/zwo-eaf/releases/download";

/// SHA-256 of each per-target tarball, as produced by `scripts/package-sdk.sh`.
const SDK_SHA256: &[(&str, &str)] = &[
    (
        "linux-aarch64",
        "284d427dbe8e019f7eeb546e663ee73785c3334dfd923393476d9afbbb58dbb5",
    ),
    (
        "linux-armv6",
        "5e1707b649e977ccbd8b33e9bcc097294f8ab972a3b865f50a07ae694caf04be",
    ),
    (
        "linux-armv7",
        "24e4f9e68efbfb239aaa01cbce4416b0cce7aef65d98eaf7af7d94cea192a6fa",
    ),
    (
        "linux-x86_64",
        "6ae9fa54f7890acf7ff7d8b968ce6699d35066844615d94db5d37404465d7294",
    ),
    (
        "linux-x86",
        "1d61b6d9cc3044dd020667d184217b2e3d760efd9867fda18798faa1b4bfe23f",
    ),
    (
        "macos-aarch64",
        "cba5efe2ca56d2b2a0455948ba3b46edd0a23834f32f14dcaa87bf77c8b13a96",
    ),
    (
        "macos-x86_64",
        "76b6e1e820372f6ea8c7ad250377ec90c01e96c3fdf0a08d992daa4fa23aed0e",
    ),
    (
        "windows-x86_64",
        "fdac997aec535f1807eb940c9c2d6fc87d7a929390bebc468d7cb61a727a59a4",
    ),
    (
        "windows-x86",
        "1c7b4b346cae3d510376200e7b7114c17be0647f2e2c7d5039c1214fcb0fe9c0",
    ),
];

/// Everything build.rs needs to know about the compile target.
struct Target {
    os: String,
    /// Tarball/release target name, e.g. `macos-aarch64`.
    name: &'static str,
    /// Library subdirectory inside an *unmodified* ZWO SDK tree.
    zwo_lib_subdir: &'static str,
}

fn target() -> Target {
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    // armv6 vs armv7 are both `arm` to cargo; distinguish on the target triple.
    let triple = env::var("TARGET").unwrap_or_default();
    let (name, zwo_lib_subdir) = match (os.as_str(), arch.as_str()) {
        ("macos", "aarch64") => ("macos-aarch64", "lib/mac_arm64"),
        ("macos", "x86_64") => ("macos-x86_64", "lib/mac_x64"),
        ("linux", "x86_64") => ("linux-x86_64", "lib/x64"),
        ("linux", "x86") => ("linux-x86", "lib/x86"),
        ("linux", "aarch64") => ("linux-aarch64", "lib/armv8"),
        ("linux", "arm") if triple.starts_with("arm-") || triple.contains("v6") => {
            ("linux-armv6", "lib/armv6")
        }
        ("linux", "arm") => ("linux-armv7", "lib/armv7"),
        ("windows", "x86_64") => ("windows-x86_64", "lib/Windows/x64/Release"),
        ("windows", "x86") => ("windows-x86", "lib/Windows/Win32/Release"),
        _ => panic!("zwo-eaf-sys: unsupported target {os}-{arch} ({triple})"),
    };
    Target {
        os,
        name,
        zwo_lib_subdir,
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=ZWO_EAF_SDK_PATH");
    println!("cargo:rerun-if-env-changed=ZWO_EAF_SDK_TARBALL");
    println!("cargo:rerun-if-env-changed=DOCS_RS");
    println!("cargo:rustc-check-cfg=cfg(zwo_eaf_ble_stripped)");

    if env::var_os("DOCS_RS").is_some() {
        println!("cargo:warning=zwo-eaf-sys: DOCS_RS detected, skipping SDK link");
        return;
    }

    let tgt = target();
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Resolve to (lib_dir, sdk_root) — lib_dir holds the platform library files.
    let lib_dir = if let Some(p) = env::var_os("ZWO_EAF_SDK_PATH") {
        let root = PathBuf::from(p);
        lib_dir_in_sdk_root(&root, &tgt).unwrap_or_else(|| {
            panic!(
                "ZWO_EAF_SDK_PATH={} does not contain include/EAF_focuser.h and a \
                 library directory for {} (expected `{}` or `lib/`)",
                root.display(),
                tgt.name,
                tgt.zwo_lib_subdir
            )
        })
    } else if let Some(p) = env::var_os("ZWO_EAF_SDK_TARBALL") {
        let tarball = PathBuf::from(p);
        println!("cargo:rerun-if-changed={}", tarball.display());
        let data = fs::read(&tarball).unwrap_or_else(|e| {
            panic!("cannot read ZWO_EAF_SDK_TARBALL {}: {e}", tarball.display())
        });
        install_tarball(&data, &out_dir, &tgt)
    } else if let Some(root) = discover_sdk(&manifest_dir, &tgt) {
        println!(
            "cargo:warning=zwo-eaf-sys: using auto-discovered SDK at {}",
            root.display()
        );
        lib_dir_in_sdk_root(&root, &tgt).unwrap()
    } else {
        download_sdk(&out_dir, &tgt)
    };

    // Let dependent build scripts find the SDK (e.g. for copying the header).
    println!("cargo:lib_dir={}", lib_dir.display());

    // The SDK ships the static archive and a shared library side by side, and
    // both ld64 and GNU ld prefer the shared one when a `-l` search hits a
    // directory containing both (which breaks e.g. this crate's own test
    // binary with a dyld "@loader_path/libEAFFocuser.dylib" lookup). Stage
    // only the files we actually link into a private directory.
    let link_dir = stage_link_dir(&lib_dir, &out_dir, &tgt);
    println!("cargo:rustc-link-search=native={}", link_dir.display());

    match tgt.os.as_str() {
        "macos" => {
            let ble_stripped = env::var_os("CARGO_FEATURE_BLUETOOTH").is_none()
                && strip_macos_ble(&link_dir.join("libEAFFocuser.a"));
            if ble_stripped {
                // Tells src/lib.rs to provide the stub for the one symbol
                // the removed objects defined.
                println!("cargo:rustc-cfg=zwo_eaf_ble_stripped");
            }
            println!("cargo:rustc-link-lib=static=EAFFocuser");
            for fw in ["IOKit", "CoreFoundation", "Foundation", "Cocoa", "AppKit"] {
                println!("cargo:rustc-link-lib=framework={fw}");
            }
            if !ble_stripped {
                println!("cargo:rustc-link-lib=framework=CoreBluetooth");
            }
            println!("cargo:rustc-link-lib=c++");
        }
        "linux" => {
            // The archive bundles hidapi (hidraw backend) and dlopens the
            // sdbus BLE helper at runtime, so only the C++ runtime is needed.
            println!("cargo:rustc-link-lib=static=EAFFocuser");
            println!("cargo:rustc-link-lib=stdc++");
            println!("cargo:rustc-link-lib=dl");
        }
        "windows" => {
            println!("cargo:rustc-link-lib=dylib=EAF_focuser");
            copy_runtime_libs(&lib_dir);
        }
        _ => unreachable!(),
    }
}

/// Copy the link-time library files for this target into `OUT_DIR/link` and
/// return that directory. See the comment at the call site for why.
fn stage_link_dir(lib_dir: &Path, out_dir: &Path, tgt: &Target) -> PathBuf {
    let files: &[&str] = match tgt.os.as_str() {
        "windows" => &["EAF_focuser.lib"],
        _ => &["libEAFFocuser.a"],
    };
    let link_dir = out_dir.join("link");
    fs::create_dir_all(&link_dir).unwrap();
    for f in files {
        let src = lib_dir.join(f);
        let dst = link_dir.join(f);
        println!("cargo:rerun-if-changed={}", src.display());
        // The copy may be modified afterwards (see `strip_macos_ble`), and
        // `fs::copy` carries over a read-only mode from the SDK, so replace
        // rather than overwrite and make sure the result is writable.
        let _ = fs::remove_file(&dst);
        fs::copy(&src, &dst)
            .and_then(|_| make_owner_writable(&dst))
            .unwrap_or_else(|e| {
                panic!(
                    "zwo-eaf-sys: copy {} -> {}: {e}",
                    src.display(),
                    dst.display()
                )
            });
    }
    link_dir
}

#[cfg(unix)]
fn make_owner_writable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(perms.mode() | 0o200);
    fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
fn make_owner_writable(path: &Path) -> std::io::Result<()> {
    let mut perms = fs::metadata(path)?.permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(path, perms)
}

/// Archive members of the macOS `libEAFFocuser.a` that implement Bluetooth
/// LE. Removed from the staged copy unless the `bluetooth` feature is on.
const MACOS_BLE_MEMBERS: &[&str] = &[
    "IBluetoothLEManager.o",
    "BluetoothLEManagerWin.o",
    "BluetoothLEManagerMac.o",
    "MacBLEManagerBridge.o",
    "MacCoreBluetoothImp.o",
];

/// Remove the Bluetooth LE objects from the staged macOS archive. Returns
/// `true` on success; on any failure the archive is left intact (and the
/// caller keeps linking CoreBluetooth) after a build warning.
///
/// Why: `BluetoothLEManagerWin.o` has a C++ static initializer
/// (`g_pBleManager = new BluetoothLEManagerMac()`) that creates a
/// `CBCentralManager` and spins the run loop waiting for it, *while dyld is
/// loading the executable*, before `main`. It is always linked in, because
/// `EAF.o` (needed by every SDK function) references
/// `IBluetoothLEManager::CreateBluetoothLEManager()`, whose object references
/// `BluetoothLEManagerWin`'s vtable. Creating a `CBCentralManager` makes TCC
/// check that the process's *responsible* app has
/// `NSBluetoothAlwaysUsageDescription` in its Info.plist; if it does not
/// (Terminal.app, Claude Code, some IDEs) TCC kills the process with SIGABRT.
/// So without this, every program linking the SDK dies at startup in those
/// environments even if it only ever talks USB.
///
/// Only `EAFBLEScan` and `EAFBLEConnect` reach `CreateBluetoothLEManager`;
/// those are declared only with the `bluetooth` feature, and `src/lib.rs`
/// supplies an aborting stub for the symbol (cfg `zwo_eaf_ble_stripped`).
fn strip_macos_ble(archive: &Path) -> bool {
    use std::process::Command;

    let target = env::var("TARGET").unwrap_or_default();
    let ar_var = format!("AR_{}", target.replace('-', "_"));
    let ar_vars = [ar_var.as_str(), "TARGET_AR", "AR"];
    for v in ar_vars {
        println!("cargo:rerun-if-env-changed={v}");
    }
    let ar = ar_vars
        .iter()
        .find_map(env::var_os)
        .unwrap_or_else(|| "ar".into());
    let ar_name = ar.to_string_lossy().into_owned();

    let warn = |msg: String| {
        println!(
            "cargo:warning=zwo-eaf-sys: {msg}; linking the unmodified SDK archive, whose \
             static initializer creates a CBCentralManager at load time (see README, macOS notes)"
        );
        false
    };
    let run = |args: &[&str], path: &Path| -> Result<String, String> {
        let out = Command::new(&ar)
            .arg(args[0])
            .arg(path)
            .args(&args[1..])
            .output()
            .map_err(|e| format!("cannot run `{ar_name}`: {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(format!(
                "`{ar_name} {}` failed: {}",
                args[0],
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    };

    let listing = match run(&["t"], archive) {
        Ok(s) => s,
        Err(e) => return warn(e),
    };
    let present: Vec<&str> = listing.lines().map(str::trim).collect();
    if let Some(missing) = MACOS_BLE_MEMBERS.iter().find(|m| !present.contains(m)) {
        return warn(format!(
            "unexpected libEAFFocuser.a layout (member {missing} not found)"
        ));
    }

    // Work on a temporary copy so a failure half way leaves the staged
    // archive untouched. `d` deletes the members; `s` rewrites the symbol
    // table (`__.SYMDEF`) that ld64 uses to pull members in.
    let tmp = archive.with_extension("a.tmp");
    let _ = fs::remove_file(&tmp);
    if let Err(e) = fs::copy(archive, &tmp) {
        return warn(format!(
            "copy {} -> {}: {e}",
            archive.display(),
            tmp.display()
        ));
    }
    let delete: Vec<&str> = std::iter::once("d")
        .chain(MACOS_BLE_MEMBERS.iter().copied())
        .collect();
    if let Err(e) = run(&delete, &tmp).and_then(|_| run(&["s"], &tmp)) {
        let _ = fs::remove_file(&tmp);
        return warn(e);
    }
    if let Err(e) = fs::rename(&tmp, archive) {
        return warn(format!(
            "rename {} -> {}: {e}",
            tmp.display(),
            archive.display()
        ));
    }
    true
}

/// Given an SDK root, return the directory holding this target's libraries.
/// Accepts both the unmodified ZWO layout and the normalized tarball layout.
fn lib_dir_in_sdk_root(root: &Path, tgt: &Target) -> Option<PathBuf> {
    if !root.join("include/EAF_focuser.h").exists() {
        return None;
    }
    let lib_file = match tgt.os.as_str() {
        "windows" => "EAF_focuser.lib",
        _ => "libEAFFocuser.a",
    };
    [root.join(tgt.zwo_lib_subdir), root.join("lib")]
        .into_iter()
        .find(|cand| cand.join(lib_file).exists())
}

/// Probe for an SDK checkout when no env var is set: `EAF_SDK_V<ver>/<sub>`
/// or a bare `<sub>` in the crate's parent directory or any ancestor of it.
/// Cargo does not tell build scripts where the workspace root is, but walking
/// all ancestors covers "next to the workspace" however deeply the crate is
/// nested (including git worktrees inside the repository).
fn discover_sdk(manifest_dir: &Path, tgt: &Target) -> Option<PathBuf> {
    let sdk_dir = format!("EAF_SDK_V{SDK_VERSION}");
    let sub = if tgt.os == "windows" {
        format!("EAF_Windows_SDK_V{SDK_VERSION}")
    } else {
        "eaf".to_string()
    };
    manifest_dir
        .ancestors()
        .skip(1)
        .flat_map(|b| [b.join(&sdk_dir).join(&sub), b.join(&sub)])
        .find(|p| lib_dir_in_sdk_root(p, tgt).is_some())
        .map(|p| p.canonicalize().unwrap_or(p))
}

fn expected_sha(tgt: &Target) -> &'static str {
    SDK_SHA256
        .iter()
        .find(|(t, _)| *t == tgt.name)
        .map(|(_, s)| *s)
        .unwrap_or_else(|| panic!("no SDK checksum for target {}", tgt.name))
}

fn tarball_name(tgt: &Target) -> String {
    format!("eaf-sdk-{SDK_VERSION}-{}.tar.gz", tgt.name)
}

/// Download the per-target tarball from the GitHub release, or reuse the
/// cached extraction in `OUT_DIR` if it is already present.
fn download_sdk(out_dir: &Path, tgt: &Target) -> PathBuf {
    let sdk_dir = out_dir.join("sdk");
    let stamp = sdk_dir.join(".sha256");
    if fs::read_to_string(&stamp)
        .map(|s| s.trim() == expected_sha(tgt))
        .unwrap_or(false)
    {
        if let Some(lib) = lib_dir_in_sdk_root(&sdk_dir, tgt) {
            return lib;
        }
    }

    let url = format!("{RELEASE_BASE}/sdk-{SDK_VERSION}/{}", tarball_name(tgt));
    println!("cargo:warning=zwo-eaf-sys: downloading {url}");
    let data = fetch(&url).unwrap_or_else(|e| {
        panic!(
            "zwo-eaf-sys: failed to download the ZWO EAF SDK from {url}: {e}\n\
             \n\
             If you are offline or behind a proxy, download the ZWO EAF SDK v{SDK_VERSION} \
             yourself and set ZWO_EAF_SDK_PATH to its root (the `eaf/` directory on \
             Linux/macOS, the `EAF_Windows_SDK_V{SDK_VERSION}` directory on Windows), or set \
             ZWO_EAF_SDK_TARBALL to a local copy of {}.",
            tarball_name(tgt)
        )
    });
    install_tarball(&data, out_dir, tgt)
}

fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let resp = ureq::get(url).call().map_err(|e| e.to_string())?;
    let mut body = Vec::new();
    resp.into_body()
        .into_reader()
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    Ok(body)
}

/// Verify `data` against the pinned checksum and extract it to `OUT_DIR/sdk`.
fn install_tarball(data: &[u8], out_dir: &Path, tgt: &Target) -> PathBuf {
    let actual = hex(&Sha256::digest(data));
    let expected = expected_sha(tgt);
    if actual != expected {
        panic!(
            "zwo-eaf-sys: SDK tarball checksum mismatch for {}\n  expected {expected}\n  actual   {actual}",
            tarball_name(tgt)
        );
    }

    let sdk_dir = out_dir.join("sdk");
    let _ = fs::remove_dir_all(&sdk_dir);
    fs::create_dir_all(&sdk_dir).unwrap();
    let gz = flate2::read::GzDecoder::new(data);
    tar::Archive::new(gz)
        .unpack(&sdk_dir)
        .unwrap_or_else(|e| panic!("zwo-eaf-sys: failed to extract SDK tarball: {e}"));
    fs::write(sdk_dir.join(".sha256"), expected).unwrap();

    lib_dir_in_sdk_root(&sdk_dir, tgt).unwrap_or_else(|| {
        panic!(
            "zwo-eaf-sys: extracted SDK tarball is missing include/EAF_focuser.h or lib/ ({})",
            sdk_dir.display()
        )
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Windows only: put `EAF_focuser.dll` next to the built executables so
/// `cargo run`/`cargo test` find it without PATH changes. Best-effort.
fn copy_runtime_libs(lib_dir: &Path) {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let Some(profile_dir) = out_dir.ancestors().nth(3) else {
        return;
    };
    let src = lib_dir.join("EAF_focuser.dll");
    if !src.exists() {
        return;
    }
    for dst_dir in [profile_dir.to_path_buf(), profile_dir.join("deps")] {
        let dst = dst_dir.join("EAF_focuser.dll");
        if let Err(e) = fs::create_dir_all(&dst_dir).and_then(|_| fs::copy(&src, &dst)) {
            println!(
                "cargo:warning=zwo-eaf-sys: could not copy {} -> {}: {e}",
                src.display(),
                dst.display()
            );
        }
    }
}
