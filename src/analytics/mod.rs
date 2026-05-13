//! Analytics: event capture, storage, aggregation, presentation.
//!
//! ## Layers
//!
//! - [`event`] — the on-disk schema. Every record fits one JSONL line.
//! - [`store`] — append-only writer (best-effort) and tolerant reader.
//! - [`aggregate`] — pure `Vec<Event>` → `Stats` reducer.
//! - [`render`] — `Renderer` trait + concrete implementations. Isolated so
//!   we can iterate on the visual presentation without touching the data
//!   pipeline.
//!
//! ## Concurrency
//!
//! Multiple `sidekick hook` processes may write concurrently. The store relies
//! on `O_APPEND` atomicity plus single-syscall writes of small lines. See
//! [`store`] for details.

pub mod aggregate;
pub mod event;
pub mod render;
pub mod store;

pub use aggregate::{TimeRange, aggregate};
