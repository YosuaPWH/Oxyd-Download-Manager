//! Named queues: each runs its own number of downloads at once, may cap their combined
//! speed and has its own schedule.

use serde::{Deserialize, Serialize};

use crate::settings::Schedule;

/// The queue every download belongs to unless it was moved. It cannot be deleted; its
/// concurrency and schedule are the ones in [`Settings`](crate::Settings).
pub const MAIN_QUEUE: &str = "main";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Queue {
    /// Empty when creating a queue.
    pub id: String,
    /// Empty for the main queue: the UI shows its translated name.
    pub name: String,
    /// Downloads of this queue that run at the same time.
    pub max_active: u32,
    /// Combined speed of the queue in bytes/second.
    pub speed_limit: Option<u64>,
    pub schedule: Schedule,
}

impl Queue {
    pub fn is_main(&self) -> bool {
        self.id == MAIN_QUEUE
    }

    /// Clamp values into their valid ranges.
    pub fn normalized(mut self) -> Self {
        self.name = self.name.trim().chars().take(40).collect();
        self.max_active = self.max_active.clamp(1, 20);
        self.speed_limit = self.speed_limit.filter(|&s| s > 0);
        self.schedule.days.retain(|&d| d < 7);
        self.schedule.days.sort_unstable();
        self.schedule.days.dedup();
        self
    }
}
