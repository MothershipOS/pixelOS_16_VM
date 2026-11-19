//! Guest kernel modules for PixelOS's stock GKI kernel (docs/gki-modules.md has the one-time setup of
//! the GKI workspace: the kernel/common source at the shipped kernel's commit, its exact clang, the guest's config):
//!
//! - `gki-module <gki dir> <aosp dir> <driver>`: build mothershipOS's guest/drivers/<driver> as a module against the GKI tree
//!   (`make O=out-vd M=... modules` in the Android build image, the toolchain from the AOSP prebuilts); the host
//!   paths of the workspace and the AOSP tree (they are mounted into the container). target/gki/<driver>.ko. A driver
//!   with an `upstream` file (a directory of the kernel tree, e.g. drivers/gpu/drm/virtio) is that in-tree driver:
//!   its sources are copied from the GKI tree and the driver's `*.patch` applied; `*.symvers` in the driver are
//!   passed as KBUILD_EXTRA_SYMBOLS (exports of vendor modules it uses)
//! - `gki-symvers <out> <Image> <anchor.ko>...`: Module.symvers of a trimmed arm64 kernel Image, read from the
//!   Image (its __ksymtab tables and CRC tables), the CRC tables found with modules known to load on it (anchors)
//! - `gki-symvers-elf <out> <module>:<file>...`: Module.symvers entries from ELF files (__crc_<sym> symbols), e.g. the
//!   vendor modules a driver depends on (cfg80211.ko)

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use object::{Object, ObjectSection, ObjectSymbol};

use crate::util::*;

const IMAGE: &str = "mothership-android-build:24.04";

/// Build a driver against the GKI workspace (host paths: the container mounts them).
pub fn module(gki: &str, aosp: &str, driver: &str) -> Result<PathBuf> {
    let src = mothership()?.join("guest/drivers").join(driver);
    ensure!(src.join("Kbuild").exists(), "{}: no Kbuild", src.display());
    // the driver's sources into the workspace (mounted at /gki); the container sees it at /gki/<driver>
    let stage = target_dir().join("gki").join(driver);
    let _ = std::fs::remove_dir_all(&stage);
    sync(&src, &stage)?;
    // an in-tree driver: its sources from the GKI tree, then the patches
    if let Ok(up) = std::fs::read_to_string(src.join("upstream")) {
        let tree = Path::new(gki).join("common").join(up.trim());
        for e in std::fs::read_dir(&tree).with_context(|| tree.display().to_string())?.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "c" || x == "h") {
                std::fs::copy(&p, stage.join(e.file_name()))?;
            }
        }
        let mut patches: Vec<PathBuf> = std::fs::read_dir(&src)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "patch")).collect();
        patches.sort();
        for p in patches {
            let st = std::process::Command::new("patch").args(["-p1", "-d"]).arg(&stage).arg("-i").arg(&p).status()?;
            ensure!(st.success(), "{}: does not apply to {}", p.display(), tree.display());
        }
    }
    let extra: Vec<String> = std::fs::read_dir(&src)?.flatten().map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "symvers"))
        .map(|p| format!("/driver/{}", p.file_name().unwrap().to_string_lossy())).collect();
    let extra = format!("KBUILD_EXTRA_SYMBOLS={}", extra.join(" "));
    docker(&["build", "-q", "-t", IMAGE, "--build-arg", &format!("UID={}", unsafe { libc::getuid() }), &root().to_string_lossy(),
             "-f", &root().join("Dockerfile.build").to_string_lossy()])?;
    let path = "/gki/clang-r536225/bin:/aosp/prebuilts/rust/linux-x86/1.88.0/bin:/aosp/prebuilts/clang-tools/linux-x86/bin:/aosp/prebuilts/kernel-build-tools/linux-x86/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
    docker(&["run", "--rm", "-v", &format!("{gki}:/gki"), "-v", &format!("{aosp}:/aosp:ro"), "-v", &format!("{}:/driver", host_path(&stage)?),
             "-w", "/gki/common", "-e", &format!("PATH={path}"), IMAGE,
             "make", "O=../out-vd", "ARCH=arm64", "LLVM=1", "M=/driver", &extra, "modules"])?;
    let ko = std::fs::read_dir(&stage)?.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "ko")).context("no module built")?;
    println!("{}", ko.display());
    Ok(ko)
}

// ---------------------------------------------------------------- Module.symvers

/// The CRCs a module expects (its __versions section: struct modversion_info {unsigned long crc; char name[56]})
fn expected_crcs(ko: &Path) -> Result<BTreeMap<String, u32>> {
    let data = std::fs::read(ko)?;
    let f = object::File::parse(&*data)?;
    let s = f.section_by_name("__versions").with_context(|| format!("{}: no __versions", ko.display()))?;
    let d = s.data()?;
    Ok(d.as_chunks::<64>().0.iter()
        .map(|e| {
            let crc = u64::from_le_bytes(e[..8].try_into().unwrap()) as u32;
            let name: Vec<u8> = e[8..].iter().copied().take_while(|b| *b != 0).collect();
            (String::from_utf8_lossy(&name).into_owned(), crc)
        })
        .collect())
}

fn is_ident(b: &[u8]) -> Option<String> {
    let first = *b.first()?;
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }
    let n = b.iter().take(128).position(|c| *c == 0)?;
    b[..n].iter().all(|c| c.is_ascii_alphanumeric() || b"_.$".contains(c)).then(|| String::from_utf8_lossy(&b[..n]).into_owned())
}

/// Module.symvers of a trimmed arm64 Image. The Image holds __ksymtab / __ksymtab_gpl (struct kernel_symbol {s32
/// value, s32 name, s32 namespace}, PREL32, sorted by name), then __kcrctab / __kcrctab_gpl (a u32 per symbol, same
/// order). The tables are runs of 12-byte entries whose name offset lands on a NUL-terminated identifier; each CRC
/// table is placed by the anchors' expected CRCs and checked against them.
pub fn symvers_from_image(out: &Path, image: &Path, anchors: &[PathBuf]) -> Result<()> {
    let img = std::fs::read(image)?;
    let name_at = |t: i64| -> Option<String> { (t > 0 && (t as usize) < img.len()).then(|| is_ident(&img[t as usize..])).flatten() };
    let entry = |p: usize| -> Option<String> {
        let off = i32::from_le_bytes(img[p + 4..p + 8].try_into().unwrap()) as i64;
        name_at(p as i64 + 4 + off)
    };
    let mut runs: Vec<(usize, Vec<String>)> = vec![];
    let (mut p, end) = (0x10_0000usize, img.len() - 12);
    while p < end {
        if entry(p).is_some() {
            let mut q = p;
            let mut names: Vec<String> = vec![];
            while q < end {
                let Some(m) = entry(q) else { break };
                // a new table starts where the sort order restarts
                if names.last().is_some_and(|l| m < *l) {
                    break;
                }
                names.push(m);
                q += 12;
            }
            if names.len() > 500 {
                eprintln!("ksymtab run at {p:#x}: {} symbols ({} .. {})", names.len(), names[0], names[names.len() - 1]);
                runs.push((p, names));
                p = q;
                continue;
            }
        }
        p += 4;
    }
    let mut expect = BTreeMap::new();
    for a in anchors {
        expect.extend(expected_crcs(a)?);
    }
    let mut res: BTreeMap<String, (u32, &str)> = BTreeMap::new();
    for (ri, (_, names)) in runs.iter().enumerate() {
        let idx: BTreeMap<&str, usize> = names.iter().enumerate().map(|(i, s)| (s.as_str(), i)).collect();
        let mut votes: BTreeMap<usize, usize> = BTreeMap::new();
        for (s, c) in &expect {
            let Some(i) = idx.get(s.as_str()) else { continue };
            let pat = c.to_le_bytes();
            let mut from = 0;
            while let Some(k) = img[from..].windows(4).position(|w| w == pat) {
                let at = from + k;
                if let Some(start) = at.checked_sub(4 * i) {
                    *votes.entry(start).or_default() += 1;
                }
                from = at + 1;
            }
        }
        let (start, v) = votes.into_iter().max_by_key(|(_, v)| *v).context("no CRC table found for a ksymtab run")?;
        eprintln!("  crc table at {start:#x} ({v} anchor votes)");
        // __ksymtab precedes __ksymtab_gpl
        let kind = if ri == 0 { "EXPORT_SYMBOL" } else { "EXPORT_SYMBOL_GPL" };
        for (i, s) in names.iter().enumerate() {
            let c = u32::from_le_bytes(img[start + 4 * i..start + 4 * i + 4].try_into().unwrap());
            res.insert(s.clone(), (c, kind));
        }
    }
    let mut text = String::new();
    for (s, (c, kind)) in &res {
        writeln!(text, "0x{c:08x}\t{s}\tvmlinux\t{kind}\t")?;
    }
    std::fs::write(out, text)?;
    let bad: Vec<&String> = expect.iter().filter(|(s, c)| res.get(*s).is_some_and(|(r, _)| r != *c)).map(|(s, _)| s).collect();
    let missing = expect.keys().filter(|s| !res.contains_key(*s)).count();
    eprintln!("{} exports; anchors: {} total, {} mismatched, {missing} not vmlinux exports (module to module)", res.len(), expect.len(), bad.len());
    ensure!(bad.is_empty(), "CRCs differ from the anchors: {:?}", &bad[..bad.len().min(5)]);
    Ok(())
}

/// Module.symvers entries from ELF files: the CRC each __crc_<sym> symbol points to.
pub fn symvers_from_elf(out: &Path, specs: &[String]) -> Result<()> {
    let mut text = String::new();
    for spec in specs {
        let (module, path) = spec.split_once(':').context("<module>:<file>")?;
        let data = std::fs::read(path)?;
        let f = object::File::parse(&*data)?;
        let relocatable = f.kind() == object::ObjectKind::Relocatable;
        // the export type: __ksymtab_<sym> in __ksymtab or __ksymtab_gpl
        let mut kind: BTreeMap<String, &str> = BTreeMap::new();
        for s in f.symbols() {
            let (Ok(name), Some(sec)) = (s.name(), s.section_index()) else { continue };
            if let Some(sym) = name.strip_prefix("__ksymtab_") {
                let gpl = f.section_by_index(sec).ok().and_then(|x| x.name().ok().map(|n| n.contains("gpl"))).unwrap_or(false);
                kind.insert(sym.to_string(), if gpl { "EXPORT_SYMBOL_GPL" } else { "EXPORT_SYMBOL" });
            }
        }
        let mut n = 0;
        for s in f.symbols() {
            let (Ok(name), Some(sec)) = (s.name(), s.section_index()) else { continue };
            let Some(sym) = name.strip_prefix("__crc_") else { continue };
            let section = f.section_by_index(sec)?;
            let d = section.data()?;
            let off = s.address() - if relocatable { 0 } else { section.address() };
            let Some(b) = d.get(off as usize..off as usize + 4) else { continue };
            let crc = u32::from_le_bytes(b.try_into().unwrap());
            writeln!(text, "0x{crc:08x}\t{sym}\t{module}\t{}\t", kind.get(sym).copied().unwrap_or("EXPORT_SYMBOL_GPL"))?;
            n += 1;
        }
        eprintln!("{module}: {n} CRCs");
    }
    std::fs::write(out, text)?;
    Ok(())
}

pub fn dispatch(task: &str, args: &[String]) -> Result<()> {
    let a = |i: usize| args.get(i).with_context(|| format!("{task}: missing argument {}", i + 1));
    match task {
        "gki-module" => module(a(0)?, a(1)?, a(2)?).map(|_| ()),
        "gki-symvers" => symvers_from_image(Path::new(a(0)?), Path::new(a(1)?), &args[2..].iter().map(PathBuf::from).collect::<Vec<_>>()),
        "gki-symvers-elf" => symvers_from_elf(Path::new(a(0)?), &args[1..]),
        _ => bail!("unknown task {task}"),
    }
}
