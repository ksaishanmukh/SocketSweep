//! Content categories: a file's from its extension, a folder's from the bytes
//! below it.
//!
//! Each walked directory keeps the bytes below it per category
//! ([`CatSizes`]), kept in step with its sub totals: listings and stats add,
//! deletes and prunes subtract. Directories' own sizes count as
//! [`Category::Other`], so a directory's categories add up to its sub totals
//! as the host has seen them. `SubtreeDone` changes nothing here: it brings
//! the daemon's totals, which differ from the host's sum only where the
//! daemon counted a hardlinked file once.

use socketsweep_gui_contract::{Category, Size, SizeMetric};
use socketsweep_wire::Kind;

use super::metric;

/// Bytes per category, indexed by [`Category::index`].
pub(crate) type CatSizes = [Size; 6];

pub(crate) fn add(to: &mut CatSizes, delta: &CatSizes) {
    for (t, d) in to.iter_mut().zip(delta) {
        t.apparent = t.apparent.saturating_add(d.apparent);
        t.disk = t.disk.saturating_add(d.disk);
    }
}

pub(crate) fn subtract(from: &mut CatSizes, delta: &CatSizes) {
    for (f, d) in from.iter_mut().zip(delta) {
        f.apparent = f.apparent.saturating_sub(d.apparent);
        f.disk = f.disk.saturating_sub(d.disk);
    }
}

/// Count `size` under `category` in `to`.
pub(crate) fn put(to: &mut CatSizes, category: Category, size: Size) {
    let slot = &mut to[category.index()];
    slot.apparent = slot.apparent.saturating_add(size.apparent);
    slot.disk = slot.disk.saturating_add(size.disk);
}

/// The category holding the most bytes by `by`; [`Category::Other`] when
/// nothing is known yet. Ties go to the earlier category in
/// [`Category::ALL`].
pub(crate) fn dominant(sizes: &CatSizes, by: SizeMetric) -> Category {
    let mut best = Category::Other;
    let mut best_bytes = 0;
    for c in Category::ALL {
        let bytes = metric(sizes[c.index()], by);
        if bytes > best_bytes {
            best = c;
            best_bytes = bytes;
        }
    }
    best
}

/// The category an entry's own size counts under.
pub(crate) fn own_category(kind: Kind, name: &[u8]) -> Category {
    match kind {
        Kind::File => category_of(name),
        Kind::Dir | Kind::Symlink | Kind::Other => Category::Other,
    }
}

/// How the GUI's type breakdown names each category.
pub(crate) fn label(category: Category) -> &'static str {
    match category {
        Category::Audio => "Audio",
        Category::Video => "Video",
        Category::Photo => "Photos",
        Category::Archive => "Archives & APKs",
        Category::Doc => "Documents",
        Category::Other => "Other",
    }
}

const PHOTO: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "heic", "heif", "bmp", "tiff", "tif", "dng", "raw",
    "avif", "svg",
];
const VIDEO: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "3gp", "webm", "flv", "m4v", "wmv", "ts", "mpg", "mpeg",
];
const AUDIO: &[&str] = &[
    "mp3", "aac", "flac", "wav", "ogg", "m4a", "opus", "wma", "amr", "mid",
];
/// App packages are archives on disk, and are cleaned up the same way.
const ARCHIVE: &[&str] = &[
    "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso", "img", "apk", "obb", "xapk", "apks", "aab",
];
const DOC: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "md", "epub", "mobi", "csv", "rtf",
    "odt",
];

/// A file's category from its extension, case-insensitively. A name with no
/// extension, or only a leading dot (`.nomedia`), is [`Category::Other`].
pub fn category_of(name: &[u8]) -> Category {
    let Some(dot) = name.iter().rposition(|b| *b == b'.') else {
        return Category::Other;
    };
    if dot == 0 || dot + 1 >= name.len() {
        return Category::Other;
    }
    let ext = String::from_utf8_lossy(&name[dot + 1..]).to_ascii_lowercase();
    let lists = [
        (Category::Photo, PHOTO),
        (Category::Video, VIDEO),
        (Category::Audio, AUDIO),
        (Category::Archive, ARCHIVE),
        (Category::Doc, DOC),
    ];
    lists
        .iter()
        .find(|(_, list)| list.contains(&ext.as_str()))
        .map_or(Category::Other, |(c, _)| *c)
}
