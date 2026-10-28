//! Paths as bytes, the way the wire carries them.

use std::path::{Path, PathBuf};

#[cfg(unix)]
pub fn bytes_to_path(p: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(p))
}

#[cfg(not(unix))]
pub fn bytes_to_path(p: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(p).into_owned())
}

#[cfg(unix)]
pub fn path_to_bytes(p: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    p.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
pub fn path_to_bytes(p: &Path) -> Vec<u8> {
    p.to_string_lossy().into_owned().into_bytes()
}

#[cfg(unix)]
pub(super) fn name_bytes(n: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    n.as_bytes().to_vec()
}

#[cfg(not(unix))]
pub(super) fn name_bytes(n: &std::ffi::OsStr) -> Vec<u8> {
    n.to_string_lossy().into_owned().into_bytes()
}
