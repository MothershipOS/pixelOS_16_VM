//! pixelos-install: the PixelOS VM's install step. mothershipOS's `vm-install` runs it in the VM's directory on the
//! phone, as the session user, after it put the package's files there (`mothership-package.json` names it):
//!
//! - `install`: the VM's disk (`disk.img`, GPT with Cuttlefish's partition names, slot a) from the package's images
//!   in `pkg/`; userdata expanded and grown (f2fs) to MSHIP_PIXELOS_USERDATA_GIB (32). Needs resize.f2fs.
//! - `update`: the disk brought up to the new images without losing the user's data. Every image goes into its
//!   slot-a partition (`super`, the four `vbmeta*`, `boot`, `init_boot`, `vendor_boot`); `userdata` and `metadata`
//!   stay. Each partition is first cleared (a hole punched over it: it reads as zeros and the disk stays sparse), so
//!   nothing of the old image survives where the new one has holes (dm-verity hashes those blocks as zeros). Every
//!   image is checked to fit before anything is written.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use android_image::{gpt, sparse};
use anyhow::{Context, Result, bail, ensure};

const MIB: u64 = 1 << 20;
const DISK: &str = "disk.img";

/// (image in pkg/, partition)
const IMAGES: &[(&str, &str)] = &[
    ("super", "super"),
    ("vbmeta", "vbmeta_a"),
    ("vbmeta_system", "vbmeta_system_a"),
    ("vbmeta_vendor_dlkm", "vbmeta_vendor_dlkm_a"),
    ("vbmeta_system_dlkm", "vbmeta_system_dlkm_a"),
    ("boot", "boot_a"),
    ("init_boot", "init_boot_a"),
    ("vendor_boot", "vendor_boot_a"),
];

/// An image's size once expanded (Android sparse images say so in their header).
fn image_size(p: &Path) -> Result<u64> {
    let mut head = [0u8; 28];
    let n = File::open(p).with_context(|| p.display().to_string())?.read(&mut head).unwrap_or(0);
    Ok(sparse::expanded_size(&head[..n]).unwrap_or(std::fs::metadata(p)?.len()))
}

fn is_sparse(p: &Path) -> Result<bool> {
    let mut head = [0u8; 28];
    let n = File::open(p)?.read(&mut head).unwrap_or(0);
    Ok(sparse::is_sparse(&head[..n]))
}

/// Copy keeping zero blocks as holes.
fn copy_sparse(src: &mut File, dst: &mut File) -> Result<()> {
    let mut buf = vec![0u8; MIB as usize];
    loop {
        let n = src.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        if buf[..n].iter().all(|b| *b == 0) {
            dst.seek(SeekFrom::Current(n as i64))?;
        } else {
            dst.write_all(&buf[..n])?;
        }
    }
}

/// Write an image (sparse or raw) into the disk at `at`.
fn put(img: &Path, disk: &mut File, at: u64) -> Result<()> {
    if is_sparse(img)? {
        sparse::expand(&mut std::io::BufReader::new(File::open(img)?), disk, at).with_context(|| img.display().to_string()).map(|_| ())
    } else {
        disk.seek(SeekFrom::Start(at))?;
        copy_sparse(&mut File::open(img)?, disk)
    }
}

/// The disk from `pkg/`.
fn install(dir: &Path, userdata_gib: u64) -> Result<()> {
    let pkg = dir.join("pkg");
    let disk = dir.join(DISK);
    ensure!(!disk.exists(), "{}: exists (an installed VM: update)", disk.display());
    // userdata: expanded, grown (f2fs) to its partition's size
    let userdata = pkg.join("userdata.raw");
    {
        let mut out = File::create(&userdata)?;
        sparse::expand(&mut File::open(pkg.join("userdata.img"))?, &mut out, 0)?;
        out.set_len(userdata_gib << 30)?;
    }
    let st = Command::new("resize.f2fs").arg(&userdata).stdout(std::process::Stdio::null()).status().context("resize.f2fs (f2fs-tools)")?;
    ensure!(st.success(), "resize.f2fs {}", userdata.display());
    let mib = |b: u64| b.div_ceil(MIB);
    // (partition, image, MiB)
    let mut layout: Vec<(&str, Option<PathBuf>, u64)> = vec![("misc", None, 1)];
    for (part, img, min) in [("boot_a", "boot", 0), ("init_boot_a", "init_boot", 0), ("vendor_boot_a", "vendor_boot", 0), ("vbmeta_a", "vbmeta", 1),
                             ("vbmeta_system_a", "vbmeta_system", 1), ("vbmeta_vendor_dlkm_a", "vbmeta_vendor_dlkm", 1),
                             ("vbmeta_system_dlkm_a", "vbmeta_system_dlkm", 1), ("super", "super", 0)] {
        let p = pkg.join(format!("{img}.img"));
        let size = if min > 0 { min } else { mib(image_size(&p)?) };
        layout.push((part, Some(p), size));
    }
    layout.push(("userdata", Some(userdata.clone()), userdata_gib * 1024));
    layout.push(("metadata", None, 64));
    // 1 MiB ahead and 1 MiB after for the GPT
    let total = 2 + layout.iter().map(|(_, _, s)| s).sum::<u64>();
    let mut f = File::create(&disk)?;
    f.set_len(total * MIB)?;
    let mut parts = vec![];
    let mut start = 1;
    for (name, _, size) in &layout {
        parts.push((*name, start * 2048, size * 2048));
        start += size;
    }
    gpt::write(&mut f, total * 2048, &parts)?;
    for ((name, img, size), (_, first, _)) in layout.iter().zip(&parts) {
        let Some(img) = img else {
            println!("{name}: empty, {size} MiB");
            continue;
        };
        put(img, &mut f, first * 512)?;
        println!("{name} <- {} ({size} MiB)", img.file_name().unwrap().to_string_lossy());
    }
    f.sync_all()?;
    std::fs::remove_file(&userdata)?;
    println!("{} ({total} MiB, sparse)", disk.display());
    Ok(())
}

/// The disk brought up to `pkg/`, its userdata kept.
fn update(dir: &Path) -> Result<()> {
    let disk_path = dir.join(DISK);
    let mut disk = std::fs::OpenOptions::new().read(true).write(true).open(&disk_path).with_context(|| disk_path.display().to_string())?;
    let parts = gpt::read(&mut disk).with_context(|| disk_path.display().to_string())?;
    let mut plan = vec![];
    for (img, part) in IMAGES {
        let p = dir.join("pkg").join(format!("{img}.img"));
        if !p.exists() {
            continue;
        }
        let Some(target) = parts.iter().find(|x| x.name == *part) else { bail!("{}: no partition {part}", disk_path.display()) };
        let size = image_size(&p)?;
        ensure!(size <= target.size(), "{img}.img ({size} B) does not fit {part} ({} B): the disk needs a fresh install", target.size());
        plan.push((p, target.clone()));
    }
    for (img, part) in plan {
        // clear: the range reads as zeros afterwards
        let r = unsafe {
            libc::fallocate(disk.as_raw_fd(), libc::FALLOC_FL_PUNCH_HOLE | libc::FALLOC_FL_KEEP_SIZE, part.offset() as libc::off_t, part.size() as libc::off_t)
        };
        ensure!(r == 0, "{}: clearing {}: {}", disk_path.display(), part.name, std::io::Error::last_os_error());
        put(&img, &mut disk, part.offset())?;
        println!("{} <- {}", part.name, img.file_name().unwrap().to_string_lossy());
    }
    disk.sync_all()?;
    println!("{}: updated, userdata kept", disk_path.display());
    Ok(())
}

fn main() -> ExitCode {
    let dir = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let gib = std::env::var("MSHIP_PIXELOS_USERDATA_GIB").ok().and_then(|g| g.parse().ok()).unwrap_or(32);
    let r = match std::env::args().nth(1).as_deref() {
        Some("install") => install(&dir, gib),
        Some("update") => update(&dir),
        _ => Err(anyhow::anyhow!("usage: pixelos-install install|update (in the VM's directory)")),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pixelos-install: {e:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_replaces_partitions_and_keeps_the_rest() {
        let dir = std::env::temp_dir().join(format!("pixelos-install-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("pkg")).unwrap();
        let disk_path = dir.join(DISK);
        let mut disk = File::create(&disk_path).unwrap();
        disk.set_len(8 << 20).unwrap();
        // 1 MiB each: vbmeta_a at 1 MiB, userdata at 2 MiB
        let parts = gpt::write(&mut disk, (8 << 20) / 512, &[("vbmeta_a", 2048, 2048), ("userdata", 4096, 2048)]).unwrap();
        for p in &parts {
            disk.seek(SeekFrom::Start(p.offset())).unwrap();
            disk.write_all(&vec![0xee; p.size() as usize]).unwrap(); // the old contents
        }
        drop(disk);
        std::fs::write(dir.join("pkg/vbmeta.img"), b"AVB0new").unwrap();
        update(&dir).unwrap();
        let mut d = vec![];
        File::open(&disk_path).unwrap().read_to_end(&mut d).unwrap();
        let (vb, ud) = (&d[1 << 20..2 << 20], &d[2 << 20..3 << 20]);
        assert_eq!(&vb[..7], b"AVB0new");
        assert!(vb[7..].iter().all(|b| *b == 0), "the old image is gone");
        assert!(ud.iter().all(|b| *b == 0xee), "userdata kept");
        std::fs::write(dir.join("pkg/vbmeta.img"), vec![1u8; (1 << 20) + 1]).unwrap();
        assert!(format!("{:#}", update(&dir).unwrap_err()).contains("does not fit"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
