//! The pending-delete queue: what `stagedSummary` answers and what
//! `staged-changed` carries.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{EntryKind, Measure, NodeRef, SessionId, Size};

/// What the queue would free, counted over the outermost staged nodes only:
/// a node staged inside another staged one is part of it, not extra.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StagedTotals {
    /// Outermost staged nodes.
    pub items: u32,
    /// Both ways of counting, so the GUI shows the metric the user chose.
    pub size: Size,
    /// Files inside them, a staged file counting as one.
    pub files: u32,
    /// `final` only when every staged node is fully measured; otherwise
    /// `partial`, and `size` is a lower bound.
    pub measure: Measure,
}

impl Default for StagedTotals {
    /// An empty queue, which has nothing left to measure.
    fn default() -> Self {
        StagedTotals {
            items: 0,
            size: Size::default(),
            files: 0,
            measure: Measure::Final,
        }
    }
}

/// One staged node, for the review list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StagedItem {
    pub node: NodeRef,
    pub name: String,
    /// Display path.
    pub path: String,
    pub kind: EntryKind,
    pub size: Size,
    pub measure: Measure,
    /// Files in the subtree; 1 for a file.
    pub files: u32,
}

/// The queue: its outermost nodes, largest first by disk usage, and totals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StagedSummary {
    pub items: Vec<StagedItem>,
    pub totals: StagedTotals,
}

/// Emitted as `staged-changed` whenever the queue changes, including when a
/// scan or a delete removes a staged node from the tree. Views fetched before
/// it carry stale `staged` and `projected` values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[cfg_attr(feature = "tauri", derive(tauri_specta::Event))]
#[serde(rename_all = "camelCase")]
pub struct StagedChanged {
    pub session: SessionId,
    pub totals: StagedTotals,
}
