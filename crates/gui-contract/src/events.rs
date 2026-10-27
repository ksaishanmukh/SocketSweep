//! The events the host emits to the GUI.

use serde::Serialize;
use specta::datatype::DataType;
use specta::{Type, Types};

use crate::{ConnectionLost, CopyProgress, ScanProgress};

/// A payload the host emits under a fixed name. The shell emits only types
/// that implement this, under [`Event::NAME`].
pub trait Event: Serialize + Type {
    /// The Tauri event name, kebab-case.
    const NAME: &'static str;
}

/// One event, for generating bindings.
#[derive(Debug, Clone)]
pub struct EventDef {
    /// The Tauri event name, kebab-case.
    pub name: &'static str,
    pub payload: DataType,
}

macro_rules! events {
    ($($ty:ident => $name:literal,)*) => {
        $(
            impl Event for $ty {
                const NAME: &'static str = $name;
            }
        )*

        /// Every event, registering its payload type in `types`.
        pub fn event_defs(types: &mut Types) -> Vec<EventDef> {
            vec![$(EventDef { name: $name, payload: <$ty as Type>::definition(types) },)*]
        }

        /// Every event's name.
        pub const EVENT_NAMES: &[&str] = &[$($name),*];
    };
}

events! {
    ScanProgress => "scan-progress",
    CopyProgress => "copy-progress",
    ConnectionLost => "connection-lost",
}
