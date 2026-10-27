//! newc cpio archives: read the files out of one, write one.

use anyhow::{Result, ensure};

fn hex(b: &[u8]) -> Result<usize> {
    Ok(usize::from_str_radix(std::str::from_utf8(b)?, 16)?)
}

fn align4(n: usize) -> usize {
    n.div_ceil(4) * 4
}

pub struct Entry {
    pub name: String,
    pub mode: u32,
    pub data: Vec<u8>,
}

/// The entries of concatenated newc archives (later entries of the same name come later in the list).
pub fn read(mut data: &[u8]) -> Result<Vec<Entry>> {
    let mut out = vec![];
    while data.len() >= 110 {
        if data[0] == 0 {
            // padding between concatenated archives
            data = &data[1..];
            continue;
        }
        ensure!(&data[..6] == b"070701" || &data[..6] == b"070702", "not a newc cpio archive");
        let field = |i: usize| hex(&data[6 + i * 8..14 + i * 8]);
        let (mode, size, namesize) = (field(1)? as u32, field(6)?, field(11)?);
        let name = String::from_utf8_lossy(&data[110..110 + namesize - 1]).into_owned();
        let body = align4(110 + namesize);
        ensure!(body + size <= data.len(), "cpio entry {name} beyond the archive");
        let content = data[body..body + size].to_vec();
        data = &data[align4(body + size)..];
        if name == "TRAILER!!!" {
            continue;
        }
        out.push(Entry { name, mode, data: content });
    }
    Ok(out)
}

/// A newc archive of these entries (owner root, one inode each).
pub fn write(entries: &[Entry]) -> Vec<u8> {
    let mut out = vec![];
    let mut put = |ino: usize, name: &str, mode: u32, data: &[u8]| {
        let fields = [ino as u32, mode, 0, 0, 1, 0, data.len() as u32, 0, 0, 0, 0, name.len() as u32 + 1, 0];
        out.extend(b"070701");
        for f in fields {
            out.extend(format!("{f:08x}").as_bytes());
        }
        out.extend(name.as_bytes());
        out.push(0);
        out.resize(align4(out.len()), 0);
        out.extend(data);
        out.resize(align4(out.len()), 0);
    };
    for (i, e) in entries.iter().enumerate() {
        put(i + 1, &e.name, e.mode, &e.data);
    }
    put(0, "TRAILER!!!", 0, &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let a = write(&[Entry { name: "lib".into(), mode: 0o040755, data: vec![] }, Entry { name: "lib/x".into(), mode: 0o100644, data: b"hello".to_vec() }]);
        let e = read(&a).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(e[1].name, "lib/x");
        assert_eq!(e[1].data, b"hello");
    }
}
