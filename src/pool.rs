//! A bounded pool that runs ordered work items through `jobs` concurrent
//! workers and forwards every outcome to one writer, in item order.
//!
//! The render stage uses it for per-clip work (PRD §12 keeps each clip
//! isolated): workers do the expensive part — decode, filter, encode — while
//! the writer alone mutates state, so status flips, manifest saves, and
//! sidecar copies stay serialized. A clip that finishes early still waits
//! for the items ahead of it, so the manifest changes in rank order even
//! when renders finish out of order.
//!
//! Cancellation is the caller's own token: workers observe it inside their
//! work (e.g. [`crate::util::run_streaming`] kills the child process), a
//! killed worker reports the error as its outcome, and claiming new items
//! can be refused inside `work`. Outcomes that did complete still reach the
//! writer — nothing already finished is thrown away.

use futures::stream::{FuturesUnordered, StreamExt};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

/// An event for the single writer. `Done` arrives strictly in item order:
/// outcomes that finish early sit in a reorder buffer until the items ahead
/// of them have been delivered.
pub enum WriteEvent<T> {
    /// A worker passed the point of real work for this item (sent by
    /// [`WorkSlot::started`]; items that short-circuit never produce one).
    Started(usize),
    /// The item's `work` future returned — success or failure is `T`.
    Done(usize, T),
}

/// A claimed item's channel back to the writer. Cheap paths that return
/// early (e.g. already-rendered skips) simply never call `started`.
pub struct WorkSlot<T> {
    index: usize,
    out: mpsc::UnboundedSender<WriteEvent<T>>,
}

impl<T> WorkSlot<T> {
    /// Mark this item as actually in progress.
    pub fn started(&self) {
        let _ = self.out.send(WriteEvent::Started(self.index));
    }
}

/// Run `work` for each index in `0..items` on `jobs` workers, sending
/// [`WriteEvent`]s to `out`. Workers claim indices in order via a shared
/// counter; each claimed index produces exactly one `Done`. The writer's
/// channel closes once every worker has exited and every completed outcome
/// has been forwarded — including results whose predecessors never ran
/// (e.g. after cancellation), which are flushed last, still in index order.
///
/// Everything runs inside the caller's `.await` point — no tasks are
/// spawned, so `work`/`Fut` need only the lifetimes and captures they
/// already have.
pub async fn run<F, Fut, T>(
    items: usize,
    jobs: usize,
    work: F,
    out: mpsc::UnboundedSender<WriteEvent<T>>,
) where
    F: Fn(usize, WorkSlot<T>) -> Fut,
    Fut: std::future::Future<Output = T>,
{
    let next = Arc::new(AtomicUsize::new(0));
    let (done_tx, mut done_rx) = mpsc::unbounded_channel::<(usize, T)>();

    let workers = FuturesUnordered::new();
    for _ in 0..jobs.max(1).min(items) {
        let next = next.clone();
        let done_tx = done_tx.clone();
        let out = out.clone();
        let work = &work;
        workers.push(async move {
            loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= items {
                    return;
                }
                let outcome = work(
                    i,
                    WorkSlot {
                        index: i,
                        out: out.clone(),
                    },
                )
                .await;
                if done_tx.send((i, outcome)).is_err() {
                    return;
                }
            }
        });
    }
    drop(done_tx);

    let drain = async move {
        let mut pending: BTreeMap<usize, T> = BTreeMap::new();
        let mut next_done = 0usize;
        while let Some((i, outcome)) = done_rx.recv().await {
            pending.insert(i, outcome);
            while let Some(outcome) = pending.remove(&next_done) {
                let _ = out.send(WriteEvent::Done(next_done, outcome));
                next_done += 1;
            }
        }
        // Workers exited with outcomes still behind a gap — the missing
        // predecessors were never claimed (nothing is lost silently: every
        // claimed index always produces a Done). Flush what finished, in
        // item order, so completed work is never dropped.
        for (i, outcome) in pending {
            let _ = out.send(WriteEvent::Done(i, outcome));
        }
    };

    tokio::join!(workers.collect::<Vec<()>>(), drain);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    /// Collect every `Done` index the writer sees.
    async fn collect<T>(mut rx: mpsc::UnboundedReceiver<WriteEvent<T>>) -> Vec<usize> {
        let mut seen = Vec::new();
        while let Some(ev) = rx.recv().await {
            if let WriteEvent::Done(i, _) = ev {
                seen.push(i);
            }
        }
        seen
    }

    #[tokio::test]
    async fn done_events_arrive_in_item_order_when_finishes_scramble() {
        // Later items finish first (reverse-delay sleeps), so the reorder
        // buffer must hold them until earlier items land.
        let n = 6usize;
        let (tx, rx) = mpsc::unbounded_channel();
        let pool = run(
            n,
            3,
            |i, _slot| async move {
                tokio::time::sleep(Duration::from_millis((n - i) as u64 * 15)).await;
                i
            },
            tx,
        );
        let (_, seen) = tokio::join!(pool, collect(rx));
        assert_eq!(seen, (0..n).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn a_failed_item_does_not_stop_the_others() {
        let (tx, rx) = mpsc::unbounded_channel();
        let pool = run(
            5,
            3,
            |i, _slot| async move {
                tokio::time::sleep(Duration::from_millis((i % 3) as u64 * 10)).await;
                if i == 1 {
                    Err("boom".to_string())
                } else {
                    Ok(i)
                }
            },
            tx,
        );
        let gather = async {
            let mut out = Vec::new();
            let mut rx = rx;
            while let Some(ev) = rx.recv().await {
                if let WriteEvent::Done(i, r) = ev {
                    out.push((i, r));
                }
            }
            out
        };
        let (_, out) = tokio::join!(pool, gather);
        assert_eq!(out.len(), 5);
        for (pos, (i, r)) in out.iter().enumerate() {
            assert_eq!(*i, pos);
            if pos == 1 {
                assert!(r.is_err());
            } else {
                assert_eq!(r.as_ref().unwrap(), &pos);
            }
        }
    }

    #[tokio::test]
    async fn concurrency_never_exceeds_jobs() {
        let inflight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = mpsc::unbounded_channel();
        let pool = run(
            9,
            3,
            |_i, _slot| {
                let inflight = inflight.clone();
                let peak = peak.clone();
                async move {
                    let now = inflight.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(15)).await;
                    inflight.fetch_sub(1, Ordering::SeqCst);
                    now
                }
            },
            tx,
        );
        let _ = tokio::join!(pool, collect(rx));
        assert!(peak.load(Ordering::SeqCst) <= 3);
    }

    #[tokio::test]
    async fn skipped_items_never_signal_started() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let pool = run(
            4,
            2,
            |i, slot| async move {
                if i % 2 == 0 {
                    // A skip path returns without touching the slot.
                    return i;
                }
                slot.started();
                tokio::time::sleep(Duration::from_millis(5)).await;
                i
            },
            tx,
        );
        let started = async {
            let mut starts = Vec::new();
            while let Some(ev) = rx.recv().await {
                if let WriteEvent::Started(i) = ev {
                    starts.push(i);
                }
            }
            starts
        };
        let (_, mut starts) = tokio::join!(pool, started);
        starts.sort_unstable();
        assert_eq!(starts, vec![1, 3]);
    }

    #[tokio::test]
    async fn cancel_kills_children_and_finished_work_is_kept() {
        // A token-cancelled run still delivers every claimed outcome in
        // order, leaves no stray child processes, and keeps item 0's
        // completed result.
        let cancel = CancellationToken::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let pool = run(
            5,
            3,
            |i, slot| {
                let cancel = cancel.clone();
                async move {
                    if i == 0 {
                        return Ok::<usize, anyhow::Error>(0);
                    }
                    if cancel.is_cancelled() {
                        return Err(crate::util::cancelled());
                    }
                    slot.started();
                    crate::util::run_streaming("sleep", &["31337".to_string()], &cancel, |_, _| {})
                        .await
                        .map(|_| i)
                }
            },
            tx,
        );
        let writer_cancel = cancel.clone();
        let gather = async {
            let mut done = Vec::new();
            while let Some(ev) = rx.recv().await {
                if let WriteEvent::Done(i, r) = ev {
                    done.push((i, r));
                    writer_cancel.cancel();
                }
            }
            done
        };
        let (_, done) = tokio::join!(pool, gather);
        assert_eq!(done.len(), 5, "every claimed item reported back");
        for (pos, (i, _)) in done.iter().enumerate() {
            assert_eq!(*i, pos, "outcomes stayed in item order");
        }
        assert_eq!(done[0].1.as_ref().ok(), Some(&0));
        for (_, r) in &done[1..] {
            assert!(crate::util::is_cancelled(r.as_ref().unwrap_err()));
        }
        // No `sleep 31337` process may outlive the pool.
        tokio::time::sleep(Duration::from_millis(100)).await;
        let status = std::process::Command::new("pgrep")
            .args(["-f", "sleep 31337"])
            .status()
            .expect("pgrep available");
        assert!(!status.success(), "a cancelled child survived");
    }
}
