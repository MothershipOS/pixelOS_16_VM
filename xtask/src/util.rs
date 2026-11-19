//! Helpers of the tasks: paths, downloads, archives, Docker.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use sha2::Digest;

/// This repository (pixelOS_16_VM)
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

/// mothershipOS, for the guest drivers' sources (guest/drivers): $MSHIP_SOURCE, or the checkout next to this one
pub fn mothership() -> Result<PathBuf> {
    let p = std::env::var_os("MSHIP_SOURCE").map(PathBuf::from).unwrap_or_else(|| root().parent().unwrap().join("mothershipOS"));
    ensure!(p.join("guest/drivers").is_dir(),
            "{}: no mothershipOS (git clone https://github.com/MothershipOS/mothershipOS next to this repository, or set MSHIP_SOURCE)", p.display());
    Ok(p)
}

pub fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root().join("target"))
}

pub fn cargo(args: &[&str]) -> Result<()> {
    let st = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())).args(args).current_dir(root()).status()?;
    if !st.success() {
        bail!("cargo {} failed", args.join(" "));
    }
    Ok(())
}

pub fn sha256(path: &Path) -> Result<String> {
    let mut h = sha2::Sha256::new();
    std::io::copy(&mut std::fs::File::open(path)?, &mut h)?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Copy `src` (a file or directory) to `dst`, leaving files whose contents are unchanged alone (their timestamps
/// decide what the kernel build redoes).
pub fn sync(src: &Path, dst: &Path) -> Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for e in std::fs::read_dir(src)? {
            let e = e?;
            sync(&e.path(), &dst.join(e.file_name()))?;
        }
    } else if std::fs::read(dst).ok() != Some(std::fs::read(src)?) {
        if let Some(d) = dst.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::copy(src, dst).with_context(|| dst.display().to_string())?;
    }
    Ok(())
}

/// `url` saved as `dest` (once), checked against its pinned SHA-256.
pub fn fetch(url: &str, sum: &str, dest: &Path) -> Result<PathBuf> {
    fetch_checked(url, sum, dest, sha256)
}

/// `url` saved as `dest` (once), checked with `digest` against `sum`.
pub fn fetch_checked(url: &str, sum: &str, dest: &Path, digest: fn(&Path) -> Result<String>) -> Result<PathBuf> {
    if let Some(d) = dest.parent() {
        std::fs::create_dir_all(d)?;
    }
    if !dest.exists() {
        println!("downloading {url}");
        let partial = dest.with_extension("partial");
        let mut body = ureq::get(url).call()?.into_body();
        std::io::copy(&mut body.as_reader(), &mut std::fs::File::create(&partial)?)?;
        std::fs::rename(&partial, dest)?;
    }
    let got = digest(dest)?;
    if got != sum {
        std::fs::remove_file(dest)?;
        bail!("{}: checksum {got}, expected {sum} (removed)", dest.display());
    }
    Ok(dest.to_path_buf())
}

/// Every file under `dir`.
pub fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            walk(&p, out)?;
        } else {
            out.push(p);
        }
    }
    Ok(())
}

/// `docker` with these arguments (the host's Docker, from the build image: its socket is mounted).
pub fn docker(args: &[&str]) -> Result<()> {
    ensure!(Command::new("docker").args(args).status().context("docker (mount /var/run/docker.sock)")?.success(), "docker {} failed", args.join(" "));
    Ok(())
}

/// Where the host sees a path under the target directory (for volumes of containers the host's Docker runs).
pub fn host_path(p: &Path) -> Result<String> {
    let host = std::env::var("MSHIP_HOST_TARGET").context("MSHIP_HOST_TARGET: the host path of the target directory")?;
    Ok(Path::new(&host).join(p.strip_prefix(target_dir())?).to_string_lossy().into_owned())
}

/// Run a program in `dir`; an error names it.
pub fn run_in(dir: &Path, args: &[&str]) -> Result<()> {
    let st = Command::new(args[0]).args(&args[1..]).current_dir(dir).status().with_context(|| args[0].to_string())?;
    ensure!(st.success(), "{} failed (in {})", args.join(" "), dir.display());
    Ok(())
}

/// A git checkout of `url` at `rev` in `dir`, with local changes dropped (patches go on top afresh).
pub fn git_checkout(url: &str, rev: &str, dir: &Path) -> Result<()> {
    if !dir.join(".git").exists() {
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir)?;
        run_in(dir, &["git", "init", "-q"])?;
        run_in(dir, &["git", "remote", "add", "origin", url])?;
    }
    let have = Command::new("git").args(["cat-file", "-e", &format!("{rev}^{{commit}}")]).current_dir(dir).status()?.success();
    if !have {
        run_in(dir, &["git", "fetch", "-q", "--depth", "1", "origin", rev])?;
    }
    run_in(dir, &["git", "checkout", "-q", "-f", rev])?;
    run_in(dir, &["git", "clean", "-q", "-f", "-d"])
}


/// store/info.txt for the store, with the package's version (mothership-package.json "version", the one source)
pub fn store_info(to: &Path) -> Result<()> {
    let pkg: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root().join("mothership-package.json"))?)?;
    let version = pkg["version"].as_str().context("mothership-package.json: no \"version\"")?;
    let info = std::fs::read_to_string(root().join("store/info.txt"))?;
    let (head, desc) = info.split_once("\n\n").unwrap_or((info.as_str(), ""));
    let head: Vec<&str> = head.lines().filter(|l| !l.starts_with("version:")).collect();
    std::fs::write(to, format!("{}\nversion: {version}\n\n{desc}", head.join("\n")))?;
    Ok(())
}
