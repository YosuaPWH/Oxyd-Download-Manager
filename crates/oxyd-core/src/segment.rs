//! Splitting a file into byte ranges and re-splitting them at runtime.

use serde::{Deserialize, Serialize};

/// Smallest segment created when planning a download.
pub const MIN_SEGMENT: u64 = 1024 * 1024;
/// A running segment is only split when at least this much remains on each half.
pub const MIN_SPLIT: u64 = 2 * 1024 * 1024;
/// `end` value for a single stream of unknown length.
pub const UNKNOWN_END: u64 = u64::MAX;

/// Inclusive byte range `start..=end`; `downloaded` bytes from `start` are on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
}

impl Segment {
    pub fn new(start: u64, end: u64) -> Self {
        Segment {
            start,
            end,
            downloaded: 0,
        }
    }

    /// Next byte offset to fetch.
    pub fn pos(&self) -> u64 {
        self.start + self.downloaded
    }

    pub fn len(&self) -> Option<u64> {
        (self.end != UNKNOWN_END).then(|| self.end + 1 - self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == Some(0)
    }

    pub fn remaining(&self) -> u64 {
        match self.len() {
            Some(l) => l.saturating_sub(self.downloaded),
            None => u64::MAX,
        }
    }

    pub fn is_done(&self) -> bool {
        self.remaining() == 0
    }
}

/// Divide `size` bytes into at most `connections` equal segments of at least [`MIN_SEGMENT`].
pub fn plan(size: u64, connections: u32) -> Vec<Segment> {
    if size == 0 {
        return vec![];
    }
    let by_size = size.div_ceil(MIN_SEGMENT);
    let n = (connections.max(1) as u64).min(by_size).max(1);
    let base = size / n;
    let extra = size % n;
    let mut out = Vec::with_capacity(n as usize);
    let mut start = 0;
    for i in 0..n {
        let len = base + u64::from(i < extra);
        out.push(Segment::new(start, start + len - 1));
        start += len;
    }
    out
}

/// One segment covering the whole file (no Range support or unknown size).
pub fn single(size: Option<u64>) -> Vec<Segment> {
    match size {
        Some(0) => vec![],
        Some(s) => vec![Segment::new(0, s - 1)],
        None => vec![Segment::new(0, UNKNOWN_END)],
    }
}

/// Dynamic segmentation: split the unfinished segment (among `candidates`) with the most
/// remaining bytes in half. The new back half is appended and its index returned.
pub fn split_largest(segments: &mut Vec<Segment>, candidates: &[usize]) -> Option<usize> {
    let (idx, rem) = candidates
        .iter()
        .map(|&i| (i, segments[i].remaining()))
        .filter(|&(i, _)| segments[i].end != UNKNOWN_END)
        .max_by_key(|&(_, r)| r)?;
    if rem < MIN_SPLIT * 2 {
        return None;
    }
    let seg = segments[idx];
    let mid = seg.pos() + rem / 2;
    segments[idx].end = mid - 1;
    segments.push(Segment::new(mid, seg.end));
    Some(segments.len() - 1)
}

/// Smallest part kept by either side of a speed-aware split. Covers what a worker may
/// already have buffered past its recorded position (one block in memory, one in flight).
pub const MIN_PART: u64 = 512 * 1024;
/// Segments expected to finish sooner than this are not worth a new connection.
pub const MIN_ETA_SECS: f64 = 1.0;

/// Speed-aware dynamic segmentation. `running` pairs each running segment with the
/// measured speed of its connection (bytes/s, `None` while unknown).
///
/// Picks the segment that will take longest to finish (remaining / speed) and splits it
/// so both parts finish together: the current connection keeps a share proportional to
/// its speed against a new connection assumed to run at the average speed. Without any
/// speed data it falls back to [`split_largest`].
pub fn split_slowest(
    segments: &mut Vec<Segment>,
    running: &[(usize, Option<f64>)],
) -> Option<usize> {
    let known: Vec<f64> = running
        .iter()
        .filter_map(|&(_, r)| r)
        .filter(|&r| r > 0.0)
        .collect();
    if known.is_empty() {
        let idxs: Vec<usize> = running.iter().map(|&(i, _)| i).collect();
        return split_largest(segments, &idxs);
    }
    let avg = known.iter().sum::<f64>() / known.len() as f64;
    let (idx, eta, speed) = running
        .iter()
        .filter(|&&(i, _)| {
            segments[i].end != UNKNOWN_END && segments[i].remaining() >= 2 * MIN_PART
        })
        .map(|&(i, r)| {
            let speed = r.filter(|&r| r > 0.0).unwrap_or(avg);
            (i, segments[i].remaining() as f64 / speed, speed)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    if eta < MIN_ETA_SECS {
        return None;
    }
    let seg = segments[idx];
    let rem = seg.remaining();
    let keep = ((rem as f64) * speed / (speed + avg)) as u64;
    let keep = keep.clamp(MIN_PART, rem - MIN_PART);
    let mid = seg.pos() + keep;
    segments[idx].end = mid - 1;
    segments.push(Segment::new(mid, seg.end));
    Some(segments.len() - 1)
}

pub fn total_downloaded(segments: &[Segment]) -> u64 {
    segments.iter().map(|s| s.downloaded).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn covers(segs: &[Segment], size: u64) {
        let mut sorted = segs.to_vec();
        sorted.sort_by_key(|s| s.start);
        let mut next = 0;
        for s in &sorted {
            assert_eq!(s.start, next, "gap or overlap in {sorted:?}");
            next = s.end + 1;
        }
        assert_eq!(next, size);
    }

    #[test]
    fn plans_even_segments() {
        let segs = plan(100 * MIN_SEGMENT + 3, 8);
        assert_eq!(segs.len(), 8);
        covers(&segs, 100 * MIN_SEGMENT + 3);
        let lens: Vec<_> = segs.iter().map(|s| s.len().unwrap()).collect();
        assert!(lens.iter().max().unwrap() - lens.iter().min().unwrap() <= 1);
    }

    #[test]
    fn small_files_use_fewer_segments() {
        assert_eq!(plan(500, 8).len(), 1);
        assert_eq!(plan(3 * MIN_SEGMENT, 8).len(), 3);
        assert_eq!(plan(3 * MIN_SEGMENT + 1, 8).len(), 4);
        assert!(plan(0, 8).is_empty());
        covers(&plan(3 * MIN_SEGMENT + 1, 8), 3 * MIN_SEGMENT + 1);
    }

    #[test]
    fn single_segment_shapes() {
        assert_eq!(single(Some(10)), vec![Segment::new(0, 9)]);
        assert!(single(Some(0)).is_empty());
        assert_eq!(single(None)[0].remaining(), u64::MAX);
    }

    #[test]
    fn splits_largest_remaining() {
        let size = 64 * MIN_SEGMENT;
        let mut segs = plan(size, 2);
        segs[0].downloaded = segs[0].len().unwrap(); // done
        segs[1].downloaded = 2 * MIN_SEGMENT;
        let new = split_largest(&mut segs, &[1]).unwrap();
        assert_eq!(new, 2);
        assert_eq!(segs[1].end + 1, segs[2].start);
        assert_eq!(segs[2].end, size - 1);
        covers(&segs, size);
        // Both halves keep their remaining bytes balanced.
        assert!(segs[1].remaining().abs_diff(segs[2].remaining()) <= 1);
    }

    #[test]
    fn splits_the_slowest_connection_proportionally() {
        let mut segs = plan(32 * MIN_SEGMENT, 2);
        segs[0].downloaded = 8 * MIN_SEGMENT; // 8 MiB left, fast connection
        segs[1].downloaded = 12 * MIN_SEGMENT; // 4 MiB left, very slow connection
        let fast = 4.0 * MIN_SEGMENT as f64;
        let slow = 0.25 * MIN_SEGMENT as f64;
        let new = split_slowest(&mut segs, &[(0, Some(fast)), (1, Some(slow))]).unwrap();
        // The slow connection (16 s left) is relieved, not the one with more bytes left.
        assert_eq!(segs[new].end, 32 * MIN_SEGMENT - 1);
        assert_eq!(segs[1].end + 1, segs[new].start);
        covers(&segs, 32 * MIN_SEGMENT);
        // It keeps only a small share: slow / (slow + average).
        let kept = segs[1].remaining();
        let given = segs[new].remaining();
        assert!(kept < given / 4, "kept {kept}, gave {given}");
    }

    #[test]
    fn does_not_split_what_finishes_soon() {
        let mut segs = plan(8 * MIN_SEGMENT, 1);
        segs[0].downloaded = 6 * MIN_SEGMENT;
        let fast = 10.0 * MIN_SEGMENT as f64; // 0.2 s left
        assert!(split_slowest(&mut segs, &[(0, Some(fast))]).is_none());
        // Small remainders are still split when they would take long.
        let slow = 0.1 * MIN_SEGMENT as f64; // 20 s left
        assert!(split_slowest(&mut segs, &[(0, Some(slow))]).is_some());
    }

    #[test]
    fn refuses_tiny_split() {
        let mut segs = plan(3 * MIN_SEGMENT, 1);
        assert!(split_largest(&mut segs, &[0]).is_none());
        let mut segs = single(None);
        assert!(split_largest(&mut segs, &[0]).is_none());
    }
}
