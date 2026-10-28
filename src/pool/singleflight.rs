use std::sync::Arc;
use dashmap::DashMap;
use tokio::sync::broadcast;

/// Manages in-flight deduplication of concurrent identical requests.
///
/// If multiple tasks request the same key concurrently, only one (the leader)
/// executes the heavy operation while others wait and share the result.
#[derive(Clone, Default, Debug)]
pub struct SingleFlight<T: Clone> {
    in_flight: Arc<DashMap<String, broadcast::Sender<T>>>,
}

/// The status of an in-flight request.
pub enum FlightStatus<T: Clone> {
    /// This task is the first to request the key and must perform the work.
    Leader(FlightLeader<T>),
    /// This task waited for an in-flight request and received the shared result.
    Follower(T),
}

/// RAII token for the leader task. Ensures the in-flight key is cleaned up
/// even if the task is cancelled, errors, or panics.
pub struct FlightLeader<T: Clone> {
    key: String,
    tx: broadcast::Sender<T>,
    in_flight: Arc<DashMap<String, broadcast::Sender<T>>>,
    completed: bool,
}

impl<T: Clone> Drop for FlightLeader<T> {
    fn drop(&mut self) {
        if !self.completed {
            self.in_flight.remove(&self.key);
        }
    }
}

impl<T: Clone> FlightLeader<T> {
    /// Broadcast the successful result to all waiting followers and remove the entry.
    pub fn complete(mut self, val: T) {
        self.completed = true;
        self.in_flight.remove(&self.key);
        let _ = self.tx.send(val);
    }
}

impl<T: Clone + Send + 'static> SingleFlight<T> {
    pub fn new() -> Self {
        Self {
            in_flight: Arc::new(DashMap::new()),
        }
    }

    /// Number of distinct keys currently in-flight.
    pub fn active_flights(&self) -> usize {
        self.in_flight.len()
    }

    /// Checks if a task is already executing for `key`.
    ///
    /// - If vacant: becomes the `Leader` and must call `leader.complete(result)`.
    /// - If occupied: awaits the in-flight execution and returns as a `Follower`.
    pub async fn wait_or_lead(&self, key: &str) -> anyhow::Result<FlightStatus<T>> {
        let mut rx = match self.in_flight.entry(key.to_string()) {
            dashmap::mapref::entry::Entry::Occupied(entry) => entry.get().subscribe(),
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                let (tx, _) = broadcast::channel(1);
                entry.insert(tx.clone());
                return Ok(FlightStatus::Leader(FlightLeader {
                    key: key.to_string(),
                    tx,
                    in_flight: self.in_flight.clone(),
                    completed: false,
                }));
            }
        };

        // Follower: wait for leader's broadcast
        match rx.recv().await {
            Ok(val) => Ok(FlightStatus::Follower(val)),
            Err(_) => anyhow::bail!("leader execution was cancelled or failed"),
        }
    }
}
