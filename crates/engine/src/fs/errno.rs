//! `errno` values the engine itself produces or that implementations commonly
//! report. Linux numbering, which is also what the wire carries.

pub const ENOENT: i32 = 2;
pub const EINTR: i32 = 4;
pub const EIO: i32 = 5;
pub const EACCES: i32 = 13;
pub const ENOTDIR: i32 = 20;
