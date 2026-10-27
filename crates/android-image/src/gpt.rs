//! GUID partition tables: write a new one (protective MBR, primary and backup header and entries), read one.

use std::io::{Read, Seek, SeekFrom, Write};

use anyhow::{Context, Result, bail, ensure};

use crate::{u32le, u64le};

const SECTOR: u64 = 512;
const ENTRIES: u64 = 128;
const ENTRY_SIZE: u64 = 128;
/// sectors of the entry array
const ENTRY_SECTORS: u64 = ENTRIES * ENTRY_SIZE / SECTOR;
/// Linux filesystem data (sgdisk's 8300)
pub const LINUX_DATA: [u8; 16] = guid("0FC63DAF-8483-4772-8E79-3D69D8477DE4");

/// A GUID in its on-disk byte order (mixed endian), from the text form.
pub const fn guid(s: &str) -> [u8; 16] {
    let b = s.as_bytes();
    const fn hex(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => c - b'A' + 10,
        }
    }
    let mut v = [0u8; 16];
    let mut i = 0;
    let mut j = 0;
    while i < b.len() {
        if b[i] != b'-' {
            v[j / 2] |= hex(b[i]) << (if j % 2 == 0 { 4 } else { 0 });
            j += 1;
        }
        i += 1;
    }
    // the first three groups are little endian on disk
    [v[3], v[2], v[1], v[0], v[5], v[4], v[7], v[6], v[8], v[9], v[10], v[11], v[12], v[13], v[14], v[15]]
}

fn random_guid() -> [u8; 16] {
    let mut g = [0u8; 16];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut g)).expect("/dev/urandom");
    g[7] = (g[7] & 0x0f) | 0x40; // version 4 (byte 7: the high byte of the little-endian third group)
    g[8] = (g[8] & 0x3f) | 0x80;
    g
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partition {
    pub name: String,
    /// first and last sector (inclusive)
    pub first: u64,
    pub last: u64,
}

impl Partition {
    pub fn offset(&self) -> u64 {
        self.first * SECTOR
    }

    pub fn size(&self) -> u64 {
        (self.last + 1 - self.first) * SECTOR
    }
}

fn header(current: u64, backup: u64, first_usable: u64, last_usable: u64, disk_guid: &[u8; 16], entries_lba: u64, entries_crc: u32) -> Vec<u8> {
    let mut h = vec![0u8; SECTOR as usize];
    h[..8].copy_from_slice(b"EFI PART");
    h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
    h[12..16].copy_from_slice(&92u32.to_le_bytes());
    h[24..32].copy_from_slice(&current.to_le_bytes());
    h[32..40].copy_from_slice(&backup.to_le_bytes());
    h[40..48].copy_from_slice(&first_usable.to_le_bytes());
    h[48..56].copy_from_slice(&last_usable.to_le_bytes());
    h[56..72].copy_from_slice(disk_guid);
    h[72..80].copy_from_slice(&entries_lba.to_le_bytes());
    h[80..84].copy_from_slice(&(ENTRIES as u32).to_le_bytes());
    h[84..88].copy_from_slice(&(ENTRY_SIZE as u32).to_le_bytes());
    h[88..92].copy_from_slice(&entries_crc.to_le_bytes());
    let crc = crc32fast::hash(&h[..92]);
    h[16..20].copy_from_slice(&crc.to_le_bytes());
    h
}

/// Write a GPT for a disk of `sectors` sectors with these partitions ((name, first sector, sectors)).
pub fn write<W: Write + Seek>(disk: &mut W, sectors: u64, parts: &[(&str, u64, u64)]) -> Result<Vec<Partition>> {
    ensure!(parts.len() as u64 <= ENTRIES, "too many partitions");
    let first_usable = 2 + ENTRY_SECTORS;
    let last_usable = sectors - 2 - ENTRY_SECTORS;
    let mut entries = vec![0u8; (ENTRIES * ENTRY_SIZE) as usize];
    let mut out = vec![];
    for (i, (name, first, n)) in parts.iter().enumerate() {
        let last = first + n - 1;
        ensure!(*first >= first_usable && last <= last_usable, "{name}: outside the usable sectors");
        let e = &mut entries[i * ENTRY_SIZE as usize..(i + 1) * ENTRY_SIZE as usize];
        e[..16].copy_from_slice(&LINUX_DATA);
        e[16..32].copy_from_slice(&random_guid());
        e[32..40].copy_from_slice(&first.to_le_bytes());
        e[40..48].copy_from_slice(&last.to_le_bytes());
        for (k, u) in name.encode_utf16().take(36).enumerate() {
            e[56 + 2 * k..58 + 2 * k].copy_from_slice(&u.to_le_bytes());
        }
        out.push(Partition { name: name.to_string(), first: *first, last });
    }
    let crc = crc32fast::hash(&entries);
    let disk_guid = random_guid();
    // protective MBR
    let mut mbr = vec![0u8; SECTOR as usize];
    mbr[446 + 4] = 0xee;
    mbr[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
    mbr[446 + 12..446 + 16].copy_from_slice(&((sectors - 1).min(u32::MAX as u64) as u32).to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xaa;
    let at = |d: &mut W, lba: u64, data: &[u8]| -> Result<()> {
        d.seek(SeekFrom::Start(lba * SECTOR))?;
        d.write_all(data)?;
        Ok(())
    };
    at(disk, 0, &mbr)?;
    at(disk, 1, &header(1, sectors - 1, first_usable, last_usable, &disk_guid, 2, crc))?;
    at(disk, 2, &entries)?;
    at(disk, sectors - 1 - ENTRY_SECTORS, &entries)?;
    at(disk, sectors - 1, &header(sectors - 1, 1, first_usable, last_usable, &disk_guid, sectors - 1 - ENTRY_SECTORS, crc))?;
    Ok(out)
}

/// The partitions of a disk with a GPT (the primary table, checked).
pub fn read<R: Read + Seek>(disk: &mut R) -> Result<Vec<Partition>> {
    let mut h = vec![0u8; SECTOR as usize];
    disk.seek(SeekFrom::Start(SECTOR))?;
    disk.read_exact(&mut h)?;
    ensure!(&h[..8] == b"EFI PART", "no GPT");
    let size = u32le(&h, 12) as usize;
    let mut check = h[..size].to_vec();
    check[16..20].fill(0);
    ensure!(crc32fast::hash(&check) == u32le(&h, 16), "GPT header checksum");
    let (lba, n, esz) = (u64le(&h, 72), u32le(&h, 80) as usize, u32le(&h, 84) as usize);
    let mut entries = vec![0u8; n * esz];
    disk.seek(SeekFrom::Start(lba * SECTOR))?;
    disk.read_exact(&mut entries)?;
    ensure!(crc32fast::hash(&entries) == u32le(&h, 88), "GPT entries checksum");
    let mut out = vec![];
    for e in entries.chunks(esz) {
        if e[..16].iter().all(|b| *b == 0) {
            continue;
        }
        let name: Vec<u16> = e[56..128].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|u| *u != 0).collect();
        out.push(Partition { name: String::from_utf16_lossy(&name), first: u64le(e, 32), last: u64le(e, 40) });
    }
    Ok(out)
}

/// A partition by name.
pub fn find<R: Read + Seek>(disk: &mut R, name: &str) -> Result<Partition> {
    read(disk)?.into_iter().find(|p| p.name == name).with_context(|| format!("no partition {name}"))
}

/// Sectors of a disk image file.
pub fn sectors(len: u64) -> Result<u64> {
    if !len.is_multiple_of(SECTOR) {
        bail!("disk size {len} is not a multiple of {SECTOR}");
    }
    Ok(len / SECTOR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guid_bytes() {
        assert_eq!(LINUX_DATA[..4], [0xaf, 0x3d, 0xc6, 0x0f]);
        assert_eq!(LINUX_DATA[8..], [0x8e, 0x79, 0x3d, 0x69, 0xd8, 0x47, 0x7d, 0xe4]);
    }

    #[test]
    fn write_then_read() {
        let sectors = 8192;
        let mut d = std::io::Cursor::new(vec![0u8; sectors as usize * 512]);
        let parts = write(&mut d, sectors, &[("misc", 2048, 2048), ("super", 4096, 2048)]).unwrap();
        assert_eq!(read(&mut d).unwrap(), parts);
        assert_eq!(find(&mut d, "super").unwrap().offset(), 4096 * 512);
    }
}
