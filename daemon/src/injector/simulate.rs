#[cfg(not(target_os = "linux"))]
use std::collections::VecDeque;
#[cfg(not(target_os = "linux"))]
use std::sync::atomic::Ordering;
#[cfg(not(target_os = "linux"))]
use std::sync::{Mutex, OnceLock};
#[cfg(not(target_os = "linux"))]
use std::time::{Duration, Instant};

#[cfg(not(target_os = "linux"))]
use rdev::{EventType, simulate};

#[cfg(not(target_os = "linux"))]
use super::gate::IS_SIMULATING;

#[cfg(not(target_os = "linux"))]
#[derive(Clone)]
pub(super) struct SimulatedEvent {
    pub(super) event: EventType,
    pub(super) queued_at: Instant,
}

#[cfg(not(target_os = "linux"))]
const SIMULATED_EVENT_TTL: Duration = Duration::from_millis(250);

#[cfg(not(target_os = "linux"))]
pub(super) fn simulated_events() -> &'static Mutex<VecDeque<SimulatedEvent>> {
    static Q: OnceLock<Mutex<VecDeque<SimulatedEvent>>> = OnceLock::new();
    Q.get_or_init(|| Mutex::new(VecDeque::new()))
}

#[cfg(not(target_os = "linux"))]
fn prune_expired_simulated_events(queue: &mut VecDeque<SimulatedEvent>) {
    while queue
        .front()
        .is_some_and(|entry| entry.queued_at.elapsed() > SIMULATED_EVENT_TTL)
    {
        queue.pop_front();
    }
}

#[cfg(not(target_os = "linux"))]
pub fn consume_simulated_event(event: &EventType) -> bool {
    let Ok(mut queue) = simulated_events().lock() else {
        return false;
    };

    prune_expired_simulated_events(&mut queue);

    if queue.front().is_some_and(|entry| entry.event == *event) {
        queue.pop_front();
        true
    } else {
        false
    }
}

/// Wrapped version of `rdev::simulate` that maintains the `IS_SIMULATING` flag.
#[cfg(not(target_os = "linux"))]
pub fn simulate_monitored(event: &EventType) -> Result<(), rdev::SimulateError> {
    if let Ok(mut queue) = simulated_events().lock() {
        prune_expired_simulated_events(&mut queue);
        queue.push_back(SimulatedEvent {
            event: *event,
            queued_at: Instant::now(),
        });
    }

    IS_SIMULATING.store(true, Ordering::SeqCst);
    let res = simulate(event);
    IS_SIMULATING.store(false, Ordering::SeqCst);

    if res.is_err()
        && let Ok(mut queue) = simulated_events().lock()
    {
        prune_expired_simulated_events(&mut queue);
        if queue.front().is_some_and(|entry| entry.event == *event) {
            queue.pop_front();
        }
    }

    res
}

/// Directly enqueue a simulated event without calling `rdev::simulate`.
///
/// Only intended for unit tests that run in headless environments where
/// `rdev::simulate` always fails and removes the event from the queue.
#[cfg(test)]
#[cfg(not(target_os = "linux"))]
pub fn enqueue_simulated_event_for_test(event: EventType) {
    if let Ok(mut queue) = simulated_events().lock() {
        prune_expired_simulated_events(&mut queue);
        queue.push_back(SimulatedEvent {
            event,
            queued_at: Instant::now(),
        });
    }
}

/// Clear all queued simulated events.
///
/// Only intended for unit tests that need to reset the global queue between
/// parallel test runs to avoid cross-test interference.
#[cfg(test)]
#[cfg(not(target_os = "linux"))]
pub fn clear_simulated_events_for_test() {
    if let Ok(mut queue) = simulated_events().lock() {
        queue.clear();
    }
}
