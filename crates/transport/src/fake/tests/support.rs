use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::super::*;

/// A fresh directory for one test's local files.
pub(super) fn scratch(test: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "socketsweep-transport-{}-{test}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub(super) fn phone() -> FakeTransport {
    FakeTransport::new().with_device(FakeDevice::ready("P1"))
}
