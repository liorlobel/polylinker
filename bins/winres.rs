//! A Windows RESOURCE file (`.res`), written by hand and linked into the binary.
//!
//! Shared by `bins/pl/build.rs`, `bins/pl-gui/build.rs` and `bins/pl-mcp/build.rs`
//! through `#[path = "../winres.rs"] mod winres;`. It is not a crate and is not
//! in anybody's `[dependencies]` or `[build-dependencies]`.
//!
//! WHY THIS IS HAND-WRITTEN
//!
//! Without it `(Get-Item polylinker.exe).VersionInfo` is empty: Add/Remove
//! Programs shows no version, the shell shows no icon, and the installer's
//! upgrade comparison has nothing in the binary to read. The ordinary fix is a
//! `[build-dependencies]` entry. Measured with `cargo tree --edges normal,build`
//! on 2026-08-05, that costs:
//!
//!   winres 0.1.12            4 crates   toml -> serde -> serde_core
//!   embed-resource 3.0.11   22 crates   cc, rustc_version, semver, toml,
//!                                       vswhom(-sys, which compiles C at build
//!                                       time), winreg, windows-sys, ...
//!
//! and BOTH still shell out to `rc.exe` / `llvm-rc` / `windres`, so every person
//! building from source would need the Windows SDK on PATH. `link.exe` consumes
//! a `.res` natively, so this file adds no crate, no `Cargo.lock` churn and no
//! toolchain requirement. `winres` was last published 2021-09-28 and Tauri
//! forked it rather than wait for it.
//!
//! The project hand-wrote DEFLATE, PNG and a TrueType parser for the same
//! reason. The difference is that a `.res` is checkable against an oracle, and
//! it was checked rather than assumed.
//!
//! WHY IT IS BELIEVED CORRECT
//!
//! The output is BYTE-IDENTICAL to Microsoft's `rc.exe` (10.0.10011.16384, as
//! shipped in Windows SDK 10.0.26100.0) given an equivalent `.rc` source, on
//! both shapes this repository produces. Measured 2026-08-05 with the exact
//! strings below; sha256 of the whole `.res`:
//!
//!   icon + version (pl-gui)     3,856 bytes
//!     6b5f84730c5ad4621952bfb22f486e3ac5cdf4570479bba87bcdd24af4a817e7
//!   version only   (pl)           960 bytes
//!     6290db24e86c259edfb8b2e552179b3a91970820e37b83d44ea873e464d6e4da
//!
//! (`tools/ci.ps1` does not re-run `rc.exe`. A gate step that needed the
//! Windows SDK would defeat half the point of this file, which is to produce
//! these bytes without one. It reads the resource back out of the linked `.exe`
//! instead, which is the property that actually matters. That sentence used to
//! begin "it is not on a CI runner"; since 2026-08-09 the gate runs on
//! windows-latest, and the reason above is the one that was always doing the
//! work.)
//!
//! `rc.exe` caught two things this file had wrong, which is the whole argument
//! for cross-checking rather than reasoning:
//!
//!   1. RT_ICON memory flags are 0x1010, not 0x1030. `rc` uses 0x1030 only for
//!      RT_GROUP_ICON and 0x0030 for RT_VERSION.
//!   2. GRPICONDIRENTRY.wPlanes must be 1. The `.ico` directory says 0 and `rc`
//!      normalises it. Copying the `.ico` entry verbatim -- what most hand-rolled
//!      implementations do -- is wrong.
//!
//! WHERE IT DOES NOTHING, ON PURPOSE
//!
//! Non-Windows targets and `x86_64-pc-windows-gnu` are skipped silently: a
//! `.res` is an MSVC-linker input, and the GNU toolchain needs the same bytes
//! fed through `windres` instead. CI therefore builds unchanged on its Linux
//! and macOS runners. If a `-gnu` build ever needs an icon, the fix
//! is to invoke `windres --input-format=rc` here, not to change these bytes.
//!
//! ONE COPY OF THE VERSION
//!
//! Every string comes from a `CARGO_PKG_*` variable, so the only version in the
//! tree is still `Cargo.toml`'s -- the same one `tools/release.ps1` reads.
//!
//! WHAT IT COSTS
//!
//! 6,656 bytes across the three binaries. Measured 2026-08-05 by building
//! `--release` twice with only the `emit` calls removed:
//!
//!   polylinker.exe  12,892,672 -> 12,897,280   +4,608
//!   pl.exe           3,598,848 ->  3,599,872   +1,024
//!   pl-mcp.exe       1,224,192 ->  1,225,216   +1,024
//!
//! which is exactly the `.rsrc` section's SizeOfRawData in each -- these images
//! had no resource directory at all before, so the whole cost is one section
//! rounded up to the 512-byte file alignment.
//!
//! A LIMIT WORTH WRITING DOWN
//!
//! This file sits outside both packages, so `cargo package` would not carry it.
//! Nothing here is published to crates.io today. If that changes, move it to a
//! `crates/pl-winres` path build-dependency -- still zero external crates.

use std::path::Path;

// ---------------------------------------------------------------- primitives

fn u16le(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn u32le(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

/// Pad to the next 4-byte boundary. Every structure in a `.res` and every node
/// of a VS_VERSIONINFO tree is DWORD-aligned; `rc` does this and the loader
/// assumes it.
fn pad4(v: &mut Vec<u8>) {
    while !v.len().is_multiple_of(4) {
        v.push(0);
    }
}

/// UTF-16LE, NUL-terminated. Every string that reaches this is ours and ASCII,
/// but `encode_utf16` costs nothing and removes the assumption.
fn wide(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() * 2 + 2);
    for c in s.encode_utf16() {
        out.extend_from_slice(&c.to_le_bytes());
    }
    out.extend_from_slice(&[0, 0]);
    out
}

// -------------------------------------------------------------- .res entries

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;
const LANG_EN_US: u16 = 0x0409;

/// One RESOURCEHEADER plus its data, padded to a 4-byte boundary.
///
/// `mem_flags` is not a free choice: it is what `rc.exe` emits for each type,
/// and the values are the ones the byte comparison above pinned down.
fn entry(out: &mut Vec<u8>, ty: u16, name: u16, mem_flags: u16, data: &[u8]) {
    // Type and Name are ordinals -- 0xFFFF then the id -- so both are 4 bytes
    // and the header is already 4-aligned. A string name would need padding
    // here; nothing in this file uses one.
    let header_size: u32 = 4 + 4 + 4 + 4 + 4 + 2 + 2 + 4 + 4;
    u32le(out, data.len() as u32);
    u32le(out, header_size);
    u16le(out, 0xFFFF);
    u16le(out, ty);
    u16le(out, 0xFFFF);
    u16le(out, name);
    u32le(out, 0); // DataVersion
    u16le(out, mem_flags);
    u16le(out, LANG_EN_US);
    u32le(out, 0); // Version
    u32le(out, 0); // Characteristics
    out.extend_from_slice(data);
    pad4(out);
}

/// The 32-byte null entry every `.res` must open with. `link.exe` uses it to
/// tell a 32-bit resource file from the 16-bit format that shares the suffix.
fn null_entry(out: &mut Vec<u8>) {
    u32le(out, 0);
    u32le(out, 32);
    u16le(out, 0xFFFF);
    u16le(out, 0);
    u16le(out, 0xFFFF);
    u16le(out, 0);
    u32le(out, 0); // DataVersion
    u16le(out, 0); // MemoryFlags
    u16le(out, 0); // LanguageId
    u32le(out, 0); // Version
    u32le(out, 0); // Characteristics
}

// -------------------------------------------------------------------- icon

struct IconFrame {
    width: u8,
    height: u8,
    colors: u8,
    planes: u16,
    bits: u16,
    data: Vec<u8>,
}

/// Read an `.ico` directory. The frame payloads are copied out unchanged --
/// PNG-compressed frames included, which is what `polylinker.ico` is: all nine
/// frames are PNG, not just the 256. Windows has loaded PNG icon frames since
/// Vista, and all nine were checked through `LoadImage(IMAGE_ICON, ...)` out of
/// a linked binary before this was written.
fn parse_ico(ico: &[u8]) -> Result<Vec<IconFrame>, String> {
    if ico.len() < 6 {
        return Err(format!("{} bytes is too short to be an .ico", ico.len()));
    }
    let rd16 = |o: usize| u16::from_le_bytes([ico[o], ico[o + 1]]);
    let rd32 = |o: usize| u32::from_le_bytes([ico[o], ico[o + 1], ico[o + 2], ico[o + 3]]);
    if rd16(0) != 0 || rd16(2) != 1 {
        return Err("the header is not an icon directory (idReserved/idType)".into());
    }
    let count = rd16(4) as usize;
    if count == 0 {
        return Err("the icon directory is empty".into());
    }
    if ico.len() < 6 + count * 16 {
        return Err(format!(
            "the directory claims {count} frame(s) but the file is only {} bytes",
            ico.len()
        ));
    }
    let mut frames = Vec::with_capacity(count);
    for i in 0..count {
        let e = 6 + i * 16;
        let size = rd32(e + 8) as usize;
        let off = rd32(e + 12) as usize;
        if off.saturating_add(size) > ico.len() {
            return Err(format!("frame {i} runs past the end of the file"));
        }
        frames.push(IconFrame {
            width: ico[e],
            height: ico[e + 1],
            colors: ico[e + 2],
            planes: rd16(e + 4),
            bits: rd16(e + 6),
            data: ico[off..off + size].to_vec(),
        });
    }
    Ok(frames)
}

/// The GRPICONDIR the shell reads to pick one frame out of the set.
///
/// It is NOT the `.ico` directory: the 4-byte file offset of each frame becomes
/// a 2-byte RT_ICON resource id, and `wPlanes` is forced to 1 (see the header
/// comment -- `polylinker.ico` records 0 and `rc.exe` rewrites it).
fn group_icon(frames: &[IconFrame], first_id: u16) -> Vec<u8> {
    let mut g = Vec::new();
    u16le(&mut g, 0); // idReserved
    u16le(&mut g, 1); // idType = icon
    u16le(&mut g, frames.len() as u16);
    for (i, f) in frames.iter().enumerate() {
        g.push(f.width);
        g.push(f.height);
        g.push(f.colors);
        g.push(0); // bReserved
        u16le(&mut g, if f.planes == 0 { 1 } else { f.planes });
        u16le(&mut g, f.bits);
        u32le(&mut g, f.data.len() as u32);
        u16le(&mut g, first_id + i as u16);
    }
    g
}

// --------------------------------------------------------------- VERSIONINFO

/// One node of the VS_VERSIONINFO tree.
///
/// Layout: `wLength`, `wValueLength`, `wType`, `szKey`, padding, `Value`,
/// padding, children. `wLength` counts everything from `wLength` through the
/// last child, including padding BETWEEN members and between children, but not
/// the padding that aligns the next sibling. Getting that boundary wrong is the
/// classic way a hand-written version block loads as empty rather than as an
/// error, which is why it was diffed against `rc.exe` rather than eyeballed.
fn vs_node(
    key: &str,
    value: &[u8],
    value_len: u16,
    is_text: bool,
    children: &[Vec<u8>],
) -> Vec<u8> {
    let mut n = Vec::new();
    u16le(&mut n, 0); // wLength, patched below
    u16le(&mut n, value_len);
    u16le(&mut n, u16::from(is_text));
    n.extend_from_slice(&wide(key));
    pad4(&mut n);
    n.extend_from_slice(value);
    if !children.is_empty() {
        pad4(&mut n);
        for (i, c) in children.iter().enumerate() {
            n.extend_from_slice(c);
            if i + 1 < children.len() {
                pad4(&mut n);
            }
        }
    }
    let len = n.len() as u16;
    n[0..2].copy_from_slice(&len.to_le_bytes());
    n
}

fn vs_string(key: &str, val: &str) -> Vec<u8> {
    let w = wide(val);
    // `wValueLength` for a text value counts WORDS, not bytes, and includes the
    // terminating NUL. Bytes here is the single most common bug in this
    // structure and it presents as a truncated string, not as a failure.
    vs_node(key, &w, (w.len() / 2) as u16, true, &[])
}

/// Everything the version resource says, in one place.
pub struct Version {
    pub file_version: [u16; 4],
    pub product_version: [u16; 4],
    /// `(key, value)` for the `040904B0` string table, in the order `rc` would
    /// have seen them in the `.rc`.
    pub strings: Vec<(String, String)>,
}

/// VS_FIXEDFILEINFO: the binary version, which is what the shell sorts and
/// compares on. The string table is what it displays. Both are written, from
/// the same `CARGO_PKG_VERSION`.
fn fixed_file_info(fv: [u16; 4], pv: [u16; 4]) -> Vec<u8> {
    let mut f = Vec::new();
    u32le(&mut f, 0xFEEF_04BD); // dwSignature
    u32le(&mut f, 0x0001_0000); // dwStrucVersion 1.0
    u32le(&mut f, (u32::from(fv[0]) << 16) | u32::from(fv[1]));
    u32le(&mut f, (u32::from(fv[2]) << 16) | u32::from(fv[3]));
    u32le(&mut f, (u32::from(pv[0]) << 16) | u32::from(pv[1]));
    u32le(&mut f, (u32::from(pv[2]) << 16) | u32::from(pv[3]));
    u32le(&mut f, 0x3F); // dwFileFlagsMask = VS_FFI_FILEFLAGSMASK
    u32le(&mut f, 0); // dwFileFlags: no debug/prerelease/patched claim
    u32le(&mut f, 0x0004_0004); // dwFileOS = VOS_NT_WINDOWS32
    u32le(&mut f, 1); // dwFileType = VFT_APP
    u32le(&mut f, 0); // dwFileSubtype
    u32le(&mut f, 0); // dwFileDateMS
    u32le(&mut f, 0); // dwFileDateLS
    f
}

fn version_block(v: &Version) -> Vec<u8> {
    let strings: Vec<Vec<u8>> = v.strings.iter().map(|(k, val)| vs_string(k, val)).collect();
    // 0409 = en-US, 04B0 = 1200 = Unicode. The pair is repeated verbatim in the
    // VarFileInfo\Translation value below; a version block whose table name and
    // translation disagree is read as having no strings at all.
    let table = vs_node("040904B0", &[], 0, true, &strings);
    let sfi = vs_node("StringFileInfo", &[], 0, true, &[table]);

    let mut xlat = Vec::new();
    u16le(&mut xlat, 0x0409);
    u16le(&mut xlat, 0x04B0);
    let translation = vs_node("Translation", &xlat, xlat.len() as u16, false, &[]);
    let vfi = vs_node("VarFileInfo", &[], 0, true, &[translation]);

    let ffi = fixed_file_info(v.file_version, v.product_version);
    vs_node(
        "VS_VERSION_INFO",
        &ffi,
        ffi.len() as u16,
        false,
        &[sfi, vfi],
    )
}

// ------------------------------------------------------------------ the file

/// Build a complete `.res`. `ico` is the raw bytes of an `.ico` file, or `None`
/// for a console binary that wants a version block and no window icon.
pub fn build(ico: Option<&[u8]>, version: &Version) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    null_entry(&mut out);
    if let Some(ico) = ico {
        let frames = parse_ico(ico)?;
        // Ids start at 1 and run consecutively. Windows picks the frame it
        // wants by reading the group, so the ids only have to agree with it.
        for (i, f) in frames.iter().enumerate() {
            entry(&mut out, RT_ICON, 1 + i as u16, 0x1010, &f.data);
        }
        entry(&mut out, RT_GROUP_ICON, 1, 0x1030, &group_icon(&frames, 1));
    }
    entry(&mut out, RT_VERSION, 1, 0x0030, &version_block(version));
    Ok(out)
}

// ------------------------------------------------------- the build.rs surface

/// `"0.1.0"` -> `[0, 1, 0, 0]`, `"0.2.0-rc.1"` -> `[0, 2, 0, 0]`.
///
/// VS_FIXEDFILEINFO has no room for a pre-release tag; the full string still
/// reaches the user through `FileVersion`, so nothing is lost, only rounded.
fn quad(v: &str) -> [u16; 4] {
    let mut q = [0u16; 4];
    let core = v.split(['-', '+']).next().unwrap_or(v);
    for (i, part) in core.split('.').take(4).enumerate() {
        q[i] = part.parse().unwrap_or(0);
    }
    q
}

/// The version resource for this package, assembled from Cargo's own
/// environment so that `Cargo.toml` stays the only place a version is written.
///
/// `bin_name` cannot come from the environment: `CARGO_BIN_NAME` is set for the
/// crate being compiled, not for its build script. It is passed in, and it is
/// the same string the `cargo:rustc-link-arg-bin=` key needs, so a typo makes
/// the link argument a no-op rather than a wrong value -- and `tools/ci.ps1`
/// reads the resource back out of the `.exe`, so a no-op is a red gate.
pub fn version_from_env(bin_name: &str) -> Version {
    let ver = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION is always set by cargo");
    let desc = std::env::var("CARGO_PKG_DESCRIPTION").unwrap_or_default();
    let licence = std::env::var("CARGO_PKG_LICENSE").unwrap_or_default();
    let q = quad(&ver);
    Version {
        file_version: q,
        product_version: q,
        strings: vec![
            // NOTICE:1-2 and the About page (bins/pl-gui/src/help.rs) already
            // say these two sentences; this is the third place they appear and
            // the only one Windows reads.
            ("CompanyName".into(), String::new()),
            ("FileDescription".into(), desc),
            ("FileVersion".into(), ver.clone()),
            ("InternalName".into(), bin_name.into()),
            (
                "LegalCopyright".into(),
                format!("Copyright 2026 The Polylinker contributors. {licence}."),
            ),
            ("OriginalFilename".into(), format!("{bin_name}.exe")),
            ("ProductName".into(), "Polylinker".into()),
            ("ProductVersion".into(), ver),
        ],
    }
}

/// Generate the `.res` and hand it to the linker. The whole of what a
/// `build.rs` has to do.
///
/// Does nothing at all off Windows-MSVC, so the Linux and macOS legs of the CI
/// matrix are unaffected -- see the header comment.
pub fn emit(bin_name: &str, icon: Option<&Path>) {
    println!("cargo:rerun-if-changed=../winres.rs");
    if let Some(p) = icon {
        println!("cargo:rerun-if-changed={}", p.display());
    }

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }

    let ico = icon.map(|p| {
        std::fs::read(p).unwrap_or_else(|e| {
            panic!(
                "cannot read the application icon {}: {e}\n\
                 Run `python bins/pl-gui/icon/build-icon.py` -- it rebuilds the .ico \
                 from polylinker.svg and works from any directory.",
                p.display()
            )
        })
    });

    let version = version_from_env(bin_name);
    let res = build(ico.as_deref(), &version).unwrap_or_else(|e| {
        panic!(
            "the application icon is not a usable .ico: {e}\n\
             Run `python bins/pl-gui/icon/build-icon.py` -- it rebuilds the .ico \
             from polylinker.svg and works from any directory."
        )
    });

    let out = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR is always set by cargo"))
        .join(format!("{bin_name}.res"));
    std::fs::write(&out, &res).unwrap_or_else(|e| panic!("cannot write {}: {e}", out.display()));

    // `link.exe` takes a .res on the command line and folds it into the image's
    // resource directory. `rustc-link-arg-bin` rather than `rustc-link-arg`
    // because the latter would also apply to test and example binaries, where
    // an OriginalFilename of `polylinker.exe` would be a lie.
    println!("cargo:rustc-link-arg-bin={bin_name}={}", out.display());
}
