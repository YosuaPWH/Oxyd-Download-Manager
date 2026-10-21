//! Download engine for Oxyd Download Manager.
//!
//! The [`Manager`] owns the queue and persistence; each download is probed, split into
//! [`Segment`]s and fetched by parallel Range requests written straight to their offsets
//! in a pre-allocated `.oxydpart` file.

pub mod checksum;
mod engine;
pub mod error;
pub mod events;
pub mod filename;
pub mod limiter;
mod manager;
pub mod paths;
pub mod probe;
pub mod segment;
pub mod settings;
pub mod store;
pub mod types;
pub mod writer;

pub use error::{Error, Result};
pub use events::Event;
pub use filename::Category;
pub use manager::Manager;
pub use probe::ProbeInfo;
pub use segment::Segment;
pub use settings::{AfterQueue, DuplicatePolicy, Schedule, Settings};
pub use types::{DownloadInfo, DownloadRecord, NewDownload, RequestHeaders, Status};
