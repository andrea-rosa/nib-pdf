//! Bounded priority queue for tile jobs, shared between callers and the
//! worker thread via `Mutex<State>` + `Condvar`.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use futures_channel::oneshot;

use crate::types::{RenderError, TileRequest, TileResult};

pub(crate) const QUEUE_CAPACITY: usize = 256;

pub(crate) type TileReply = oneshot::Sender<Result<TileResult, RenderError>>;

/// A queued tile job. Ordering: higher `priority` first, then smaller
/// `distance`, then smaller `seq` (FIFO within a class).
pub(crate) struct Job {
    pub seq: u64,
    /// Target document handle (the cache-level `TileRequest` has no doc id).
    pub doc: u64,
    pub req: TileRequest,
    pub reply: TileReply,
}

impl PartialEq for Job {
    fn eq(&self, other: &Self) -> bool {
        self.seq == other.seq
    }
}
impl Eq for Job {}

impl PartialOrd for Job {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Job {
    fn cmp(&self, other: &Self) -> Ordering {
        self.req
            .priority
            .cmp(&other.req.priority)
            .then_with(|| other.req.distance.cmp(&self.req.distance))
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

pub(crate) struct State {
    pub heap: BinaryHeap<Job>,
    pub generation: u64,
    pub closed: bool,
    /// Set by callers when a control message is queued on the mpsc channel;
    /// lives in the same lock so the worker never misses it while waiting.
    pub ctrl_pending: bool,
    seq: u64,
}

impl State {
    pub fn new() -> Self {
        State {
            heap: BinaryHeap::with_capacity(QUEUE_CAPACITY),
            generation: 0,
            closed: false,
            ctrl_pending: false,
            seq: 0,
        }
    }

    /// Pushes a job, evicting the lowest-priority queued job when the queue
    /// is full. Returns the sender of the job that lost (the evicted one, or
    /// the new job itself when it is worse than the worst queued) — the
    /// caller must reply `Cancelled` to it **after** releasing the queue
    /// lock: `oneshot::send` runs the receiver's waker inline, and the
    /// UniFFI waker can synchronously resume foreign code that re-enters
    /// this mutex.
    pub fn push(&mut self, doc: u64, req: TileRequest, reply: TileReply) -> Option<TileReply> {
        self.seq += 1;
        let job = Job {
            seq: self.seq,
            doc,
            req,
            reply,
        };
        if self.heap.len() < QUEUE_CAPACITY {
            self.heap.push(job);
            return None;
        }
        // Queue full: keep the better of (new job, current worst).
        let worst_seq = self.heap.iter().min().map(|w| w.seq);
        let evict_worst = self
            .heap
            .iter()
            .min()
            .is_some_and(|worst| job.cmp(worst) == Ordering::Greater);
        if evict_worst {
            // Rebuild without the worst element; O(n), rare.
            let mut kept = BinaryHeap::with_capacity(QUEUE_CAPACITY);
            let mut evicted: Option<Job> = None;
            for j in self.heap.drain() {
                if evicted.is_none() && Some(j.seq) == worst_seq {
                    evicted = Some(j);
                } else {
                    kept.push(j);
                }
            }
            kept.push(job);
            self.heap = kept;
            evicted.map(|ev| ev.reply)
        } else {
            Some(job.reply)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{IRect, ISize, Priority, TileKey};

    fn req(priority: Priority, distance: u32) -> TileRequest {
        TileRequest {
            key: TileKey {
                page: 0,
                bucket: 0,
                tx: 0,
                ty: 0,
                revision: 0,
                flags: 0,
            },
            device_rect: IRect {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            page_device_size: ISize {
                width: 8,
                height: 8,
            },
            generation: 0,
            priority,
            distance,
        }
    }

    /// Ordering: priority desc, then distance asc, then seq asc.
    #[test]
    fn heap_ordering() {
        let mut s = State::new();
        let (t1, _r1) = oneshot::channel();
        let (t2, _r2) = oneshot::channel();
        let (t3, _r3) = oneshot::channel();
        let _ = s.push(0, req(Priority::Prefetch, 5), t1);
        let _ = s.push(0, req(Priority::Visible, 99), t2);
        let _ = s.push(0, req(Priority::Prefetch, 1), t3);
        assert_eq!(s.heap.pop().unwrap().req.priority, Priority::Visible);
        assert_eq!(s.heap.pop().unwrap().req.distance, 1);
        assert_eq!(s.heap.pop().unwrap().req.distance, 5);
    }

    /// At capacity, a better job evicts the worst queued job (Cancelled);
    /// a worse job is rejected outright.
    #[test]
    fn queue_bounded_evicts_lowest() {
        let mut s = State::new();
        let mut rxs = Vec::new();
        for _ in 0..QUEUE_CAPACITY {
            let (tx, rx) = oneshot::channel();
            let _ = s.push(0, req(Priority::Thumbnail, 1000), tx);
            rxs.push(rx);
        }
        assert_eq!(s.heap.len(), QUEUE_CAPACITY);

        // A Visible job evicts one Thumbnail.
        let (tx, mut rx_vis) = oneshot::channel();
        let evicted = s.push(0, req(Priority::Visible, 0), tx).unwrap();
        let _ = evicted.send(Err(RenderError::Cancelled));
        assert_eq!(s.heap.len(), QUEUE_CAPACITY);
        let mut cancelled = 0;
        for rx in &mut rxs {
            if matches!(rx.try_recv(), Ok(Some(Err(RenderError::Cancelled)))) {
                cancelled += 1;
            }
        }
        assert_eq!(cancelled, 1);

        // A worse-than-worst job is itself cancelled.
        let (tx, mut rx_low) = oneshot::channel();
        let rejected = s.push(0, req(Priority::Thumbnail, 1000), tx).unwrap();
        let _ = rejected.send(Err(RenderError::Cancelled));
        assert!(matches!(
            rx_low.try_recv(),
            Ok(Some(Err(RenderError::Cancelled)))
        ));
        assert_eq!(s.heap.len(), QUEUE_CAPACITY);
        assert!(rx_vis.try_recv().unwrap().is_none());
    }
}
