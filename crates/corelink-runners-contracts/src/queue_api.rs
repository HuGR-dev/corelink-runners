// Transcribed from hugit-contracts @ 7736d02 (frozen WP-00) — wire-contract seam, no git dep.

//! QueueApi — landing-queue API surface (decomposition §1, item 9).
//!
//! Transcribed from hugit-contracts @ 7736d02 (frozen WP-00) per the
//! wire-contract rule — no git dep.

use serde::{Deserialize, Serialize};

/// A single entry in the landable queue (PR / change ready to land).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LandableEntry {
    /// Unique identifier of the queue item.
    pub item_id: String,

    /// Identifier of the intent that produced this item.
    pub intent_id: String,

    /// Merkle tree root hash of the item's workspace snapshot (lowercase
    /// hex).
    pub tree_hash: String,

    /// Position of this item in the landing order.
    pub order_index: u64,
}

/// Union merge result for a batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnionResult {
    /// Identifier of the batch.
    pub batch_id: String,

    /// Tree hash of the union merge of all batch items (lowercase hex).
    pub union_tree: String,

    /// Whether the union merge was conflict-free.
    pub conflict_free: bool,
}

/// The minimal failing pair in a batch (two items whose union fails).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MinimalFailingPair {
    /// First item of the failing pair.
    pub item_a: String,

    /// Second item of the failing pair.
    pub item_b: String,
}

/// Seal record for a completed landing batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchSeal {
    /// Identifier of the batch.
    pub batch_id: String,

    /// Tree hash of the union merge of all batch items (lowercase hex).
    pub union_tree: String,

    /// Position of this batch in the landing order.
    pub order_index: u64,

    /// Terminal state of the batch.
    pub state: String,

    /// The minimal failing pair if the batch failed; `None` otherwise.
    pub minimal_failing_pair: Option<MinimalFailingPair>,
}

/// Landing-queue API surface — compound root type aggregating all
/// landing-queue sub-types (decomposition §1, item 9). The committed JSON
/// Schema is the schema of this root type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueApi {
    /// Current landable queue entries.
    pub landable: Vec<LandableEntry>,

    /// Identifier of the current batch.
    pub batch_id: String,

    /// Union merge result for the current batch.
    pub union_result: UnionResult,

    /// Seal record for the completed batch.
    pub seal: BatchSeal,
}
