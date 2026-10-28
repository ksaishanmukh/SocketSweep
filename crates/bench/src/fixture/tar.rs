//! The fixture as a ustar archive.

use std::io::{self, Write};

use socketsweep_sim::{NodeId, SimFs};
use socketsweep_wire::Kind;

use super::{content, rel_path};

/// Every timestamp in the archive (the files' own mtimes are irrelevant to
/// the tests, and a fixed one keeps the archive reproducible).
const MTIME: u64 = 1_700_000_000;

/// Write `fs` as a ustar archive whose top folder is the root's basename,
/// parents before children. Toybox `tar -x` on the phone extracts it.
pub fn write_tar(fs: &SimFs, seed: u64, out: &mut impl Write) -> io::Result<u64> {
    let root = fs.root_path();
    let top = root.rsplit(|&b| b == b'/').next().unwrap_or(root).to_vec();
    let mut written = 0u64;
    let mut stack: Vec<NodeId> = vec![0];
    while let Some(id) = stack.pop() {
        let n = fs.node(id);
        let rel = rel_path(fs, id);
        let mut name = top.clone();
        if !rel.is_empty() {
            name.push(b'/');
            name.extend_from_slice(&rel);
        }
        if n.kind == Kind::Dir {
            name.push(b'/');
            out.write_all(&header(&name, 0o775, 0, b'5')?)?;
            written += 512;
            let mut kids: Vec<NodeId> = fs.children(id).collect();
            kids.reverse();
            stack.extend(kids);
        } else {
            out.write_all(&header(&name, 0o664, n.apparent, b'0')?)?;
            let body = content(seed, &rel, n.apparent);
            out.write_all(&body)?;
            let pad = (512 - body.len() % 512) % 512;
            out.write_all(&vec![0u8; pad])?;
            written += 512 + (body.len() + pad) as u64;
        }
    }
    out.write_all(&[0u8; 1024])?;
    Ok(written + 1024)
}

fn octal(field: &mut [u8], v: u64) {
    let digits = field.len() - 1;
    let s = format!("{v:0digits$o}");
    field[..digits].copy_from_slice(s.as_bytes());
    field[digits] = 0;
}

/// One ustar header block. Names longer than 100 bytes are split into the
/// prefix field at a `/`.
fn header(name: &[u8], mode: u32, size: u64, kind: u8) -> io::Result<[u8; 512]> {
    let mut h = [0u8; 512];
    let (prefix, base) = if name.len() <= 100 {
        (&name[..0], name)
    } else {
        let split = name[..name.len() - 1]
            .iter()
            .enumerate()
            .rev()
            .filter(|&(i, &b)| b == b'/' && i <= 155 && name.len() - i - 1 <= 100)
            .map(|(i, _)| i)
            .next()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("path too long for ustar: {}", String::from_utf8_lossy(name)),
                )
            })?;
        (&name[..split], &name[split + 1..])
    };
    h[..base.len()].copy_from_slice(base);
    octal(&mut h[100..108], u64::from(mode));
    octal(&mut h[108..116], 0);
    octal(&mut h[116..124], 0);
    octal(&mut h[124..136], size);
    octal(&mut h[136..148], MTIME);
    h[156] = kind;
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    h[345..345 + prefix.len()].copy_from_slice(prefix);
    h[148..156].copy_from_slice(b"        ");
    let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
    let s = format!("{sum:06o}\0 ");
    h[148..156].copy_from_slice(s.as_bytes());
    Ok(h)
}
