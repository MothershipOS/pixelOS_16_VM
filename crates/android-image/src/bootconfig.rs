//! The bootconfig an Android initrd ends with: `key=value` lines, NUL-padded to 4 bytes, then its size and
//! checksum (u32 each) and "#BOOTCONFIG\n".

use anyhow::{Result, ensure};

use crate::u32le;

const MAGIC: &[u8] = b"#BOOTCONFIG\n";

/// The bootconfig's lines, in order.
pub type Lines = Vec<(String, String)>;

/// (initrd without the bootconfig, the bootconfig's lines) of an initrd; no bootconfig: (all, empty).
pub fn split(initrd: &[u8]) -> Result<(&[u8], Lines)> {
    if !initrd.ends_with(MAGIC) {
        return Ok((initrd, vec![]));
    }
    let t = initrd.len() - MAGIC.len() - 8;
    let size = u32le(initrd, t) as usize;
    ensure!(size <= t, "bootconfig size {size} beyond the initrd");
    let start = t - size;
    let text = String::from_utf8_lossy(&initrd[start..t]);
    let kv = text.trim_end_matches('\0').lines().filter_map(|l| l.split_once('=').map(|(k, v)| (k.to_string(), v.to_string()))).collect();
    Ok((&initrd[..start], kv))
}

/// The bootconfig section for these lines.
pub fn encode(kv: &[(String, String)]) -> Vec<u8> {
    let mut data: Vec<u8> = kv.iter().flat_map(|(k, v)| format!("{k}={v}\n").into_bytes()).collect();
    data.push(0);
    while !data.len().is_multiple_of(4) {
        data.push(0);
    }
    let sum = data.iter().fold(0u32, |s, b| s.wrapping_add(*b as u32));
    let mut out = data.clone();
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(sum.to_le_bytes());
    out.extend(MAGIC);
    out
}

/// Set keys (an empty value removes the key), keeping the order of the others.
pub fn set(kv: &mut Vec<(String, String)>, key: &str, value: &str) {
    match kv.iter().position(|(k, _)| k == key) {
        Some(i) if value.is_empty() => {
            kv.remove(i);
        }
        Some(i) => kv[i].1 = value.to_string(),
        None if value.is_empty() => {}
        None => kv.push((key.to_string(), value.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_set() {
        let mut kv = vec![("a".to_string(), "1".to_string()), ("b".to_string(), "2".to_string())];
        let mut img = b"RAMDISK".to_vec();
        img.extend(encode(&kv));
        let (rd, parsed) = split(&img).unwrap();
        assert_eq!(rd, b"RAMDISK");
        assert_eq!(parsed, kv);
        set(&mut kv, "a", "");
        set(&mut kv, "c", "3");
        set(&mut kv, "b", "x");
        assert_eq!(kv, vec![("b".to_string(), "x".to_string()), ("c".to_string(), "3".to_string())]);
    }
}
