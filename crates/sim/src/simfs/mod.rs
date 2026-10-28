//! `SimFs`: a seeded, in-memory file tree that implements the engine's
//! [`Fs`], counts every listing and stat, supports deletes during a scan,
//! and knows its own ground-truth totals.

mod builder;
mod fs_impl;
mod random;
mod sdcard;
mod tree;
mod truth;

pub use builder::{Node, TreeBuilder};
pub use random::RandomShape;
pub use tree::{SimFs, Truth};

pub type NodeId = u32;
