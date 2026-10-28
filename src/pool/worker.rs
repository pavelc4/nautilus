use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Manages concurrent worker permits using a bounded `tokio::sync::Semaphore`.
///
/// Prevents network / MTProto saturation by capping concurrent heavy download &
/// upload tasks. Tracks active and queued tasks in real time for observability.
#[derive(Clone, Debug)]
pub struct WorkerPool {
    semaphore: Arc<Semaphore>,
    active_workers: Arc<AtomicUsize>,
    queued_tasks: Arc<AtomicUsize>,
    total_slots: usize,
}

/// RAII permit for an active worker slot. Releasing this permit (upon drop)
/// decrements active workers and returns the slot back to the Semaphore.
#[derive(Debug)]
pub struct WorkerPermit {
    _permit: OwnedSemaphorePermit,
    active_workers: Arc<AtomicUsize>,
}

impl Drop for WorkerPermit {
    fn drop(&mut self) {
        self.active_workers.fetch_sub(1, Ordering::SeqCst);
    }
}

impl WorkerPool {
    /// Creates a new worker pool with a fixed number of concurrent execution slots.
    pub fn new(slots: usize) -> Self {
        let slots = slots.max(1);
        Self {
            semaphore: Arc::new(Semaphore::new(slots)),
            active_workers: Arc::new(AtomicUsize::new(0)),
            queued_tasks: Arc::new(AtomicUsize::new(0)),
            total_slots: slots,
        }
    }

    /// Total configured concurrent slots.
    pub fn total_slots(&self) -> usize {
        self.total_slots
    }

    /// Number of tasks currently executing with an acquired permit.
    pub fn active_workers(&self) -> usize {
        self.active_workers.load(Ordering::SeqCst)
    }

    /// Number of tasks currently waiting in queue for an available permit.
    pub fn queued_tasks(&self) -> usize {
        self.queued_tasks.load(Ordering::SeqCst)
    }

    /// Number of immediately available permits.
    pub fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// Attempt to acquire a slot immediately. Returns `Some(WorkerPermit)` if
    /// available, or `None` if all slots are saturated.
    pub fn try_acquire(&self) -> Option<WorkerPermit> {
        match self.semaphore.clone().try_acquire_owned() {
            Ok(permit) => {
                self.active_workers.fetch_add(1, Ordering::SeqCst);
                Some(WorkerPermit {
                    _permit: permit,
                    active_workers: self.active_workers.clone(),
                })
            }
            Err(_) => None,
        }
    }

    /// Acquires a worker permit asynchronously.
    ///
    /// If all slots are currently busy, invokes `on_queued(queue_position)` once
    /// before waiting for a permit to become available.
    pub async fn acquire<F>(&self, on_queued: F) -> anyhow::Result<WorkerPermit>
    where
        F: FnOnce(usize),
    {
        // Fast path: immediate acquisition without queue notification
        if let Some(permit) = self.try_acquire() {
            return Ok(permit);
        }

        // Slow path: register in queue and notify callback
        let pos = self.queued_tasks.fetch_add(1, Ordering::SeqCst) + 1;
        on_queued(pos);

        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| anyhow::anyhow!("worker semaphore closed: {e}"))?;

        self.queued_tasks.fetch_sub(1, Ordering::SeqCst);
        self.active_workers.fetch_add(1, Ordering::SeqCst);

        Ok(WorkerPermit {
            _permit: permit,
            active_workers: self.active_workers.clone(),
        })
    }
}
