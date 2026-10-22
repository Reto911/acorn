//! Original partition ownership, invalidated before native revocation is applied.

use rdkafka::{
    client::ClientContext,
    consumer::{BaseConsumer, ConsumerContext, Rebalance},
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::watch;

/// Assigned partition keys and the lifetime of their original ownership.
type Leases = HashMap<(String, i32), Arc<AtomicBool>>;

/// Current local leases; a revoked lease is never made valid by reassignment.
#[derive(Clone)]
pub(super) struct Ownership {
    /// Only assignment changes and commit enqueue hold this short lock.
    pub current: Arc<Mutex<Leases>>,
    /// Wake the driver and retrying sender after assignment transitions.
    pub changed: watch::Sender<()>,
}

impl Ownership {
    /// Begin without assigned partitions.
    pub fn new() -> Self {
        Self {
            current: Default::default(),
            changed: watch::channel(()).0,
        }
    }

    /// Invalidate every original lease before closing the task consumer.
    pub fn revoke_all(&self) {
        let mut current = self.current.lock().expect("Kafka assignment lock poisoned");
        for lease in current.values() {
            lease.store(false, Ordering::Release);
        }
        current.clear();
        self.changed.send_replace(());
    }
}

/// SDK callbacks only record ownership and notify; they never wait for execution or I/O.
pub(super) struct Context {
    /// Shared original leases and assignment notifications.
    pub ownership: Ownership,
}

impl ClientContext for Context {}

impl ConsumerContext for Context {
    /// Invalidate original leases before librdkafka removes the partitions.
    fn pre_rebalance(&self, _: &BaseConsumer<Self>, rebalance: &Rebalance<'_>) {
        match rebalance {
            Rebalance::Revoke(partitions) => {
                let mut current = self
                    .ownership
                    .current
                    .lock()
                    .expect("Kafka assignment lock poisoned");
                for partition in partitions.elements() {
                    if let Some(lease) =
                        current.remove(&(partition.topic().into(), partition.partition()))
                    {
                        lease.store(false, Ordering::Release);
                    }
                }
                self.ownership.changed.send_replace(());
            }
            Rebalance::Error(_) => self.ownership.revoke_all(),
            Rebalance::Assign(_) => {}
        }
    }

    /// Allocate fresh leases after assignment; old executions retain their invalid leases.
    fn post_rebalance(&self, _: &BaseConsumer<Self>, rebalance: &Rebalance<'_>) {
        if let Rebalance::Assign(partitions) = rebalance {
            let mut current = self
                .ownership
                .current
                .lock()
                .expect("Kafka assignment lock poisoned");
            for partition in partitions.elements() {
                current.insert(
                    (partition.topic().into(), partition.partition()),
                    Arc::new(AtomicBool::new(true)),
                );
            }
            self.ownership.changed.send_replace(());
        }
    }
}
