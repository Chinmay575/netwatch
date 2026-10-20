//! Watches NetworkManager's connectivity and publishes debounced state changes.
//!
//! The library never prints and never owns the process: the caller starts it
//! with [`watch`], reads [`Update`]s from the returned channel, and stops it by
//! dropping the receiver.

use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tokio::time::sleep_until;

mod connectivity_state;
mod debouncer;
mod network_manager;

pub use connectivity_state::ConnectivityState;
use debouncer::Debouncer;

const OUTPUT_CHANNEL_CAPACITY: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConnectivityError {
    /// A D-Bus call or subscription failed.
    #[error("could not talk to NetworkManager: {0}")]
    Bus(String),
    /// NetworkManager reported a connectivity value we do not know.
    #[error("unrecognized connectivity value {0}")]
    UnknownValue(u32),
    /// The subscription ended; no further updates will arrive.
    #[error("lost connection to NetworkManager")]
    Disconnected,
}

/// A debounced state, or an error. Errors are not debounced: they are passed
/// on as soon as they happen.
pub type Update = Result<ConnectivityState, ConnectivityError>;

/// Starts watching. The first update is the current state, delivered once it
/// has stayed unchanged for `debounce`. Must be called inside a tokio runtime.
pub async fn watch(debounce: Duration) -> Result<mpsc::Receiver<Update>, ConnectivityError> {
    let raw = network_manager::subscribe().await?;
    let (tx, rx) = mpsc::channel(OUTPUT_CHANNEL_CAPACITY);
    tokio::spawn(debounce_loop(raw, tx, debounce));
    Ok(rx)
}

/// Takes the clock from tokio so tests can pause it.
fn now() -> Instant {
    tokio::time::Instant::now().into_std()
}

async fn debounce_loop(
    mut raw: mpsc::Receiver<Update>,
    tx: mpsc::Sender<Update>,
    window: Duration,
) {
    let mut debouncer = Debouncer::new(window);

    loop {
        let deadline = debouncer.deadline();

        tokio::select! {
            update = raw.recv() => match update {
                Some(Ok(state)) => debouncer.on_reading(state, now()),
                Some(Err(e)) => {
                    if tx.send(Err(e)).await.is_err() {
                        return;
                    }
                }
                None => {
                    // if the receiver is already gone there is nobody to tell
                    tx.send(Err(ConnectivityError::Disconnected)).await.ok();
                    return;
                }
            },
            () = sleep_until(tokio::time::Instant::from_std(deadline.unwrap_or_else(now))),
                if deadline.is_some() =>
            {
                if let Some(stable) = debouncer.on_timeout(now())
                    && tx.send(Ok(stable)).await.is_err()
                {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    const WINDOW: Duration = Duration::from_secs(3);

    struct Harness {
        raw: mpsc::Sender<Update>,
        out: mpsc::Receiver<Update>,
    }

    fn harness() -> Harness {
        let (raw_tx, raw_rx) = mpsc::channel(16);
        let (out_tx, out_rx) = mpsc::channel(16);
        tokio::spawn(debounce_loop(raw_rx, out_tx, WINDOW));
        Harness {
            raw: raw_tx,
            out: out_rx,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn reports_state_after_window() {
        let mut h = harness();
        h.raw.send(Ok(ConnectivityState::Offline)).await.unwrap();

        sleep(Duration::from_secs(2)).await;
        assert!(h.out.try_recv().is_err());

        sleep(Duration::from_millis(1100)).await;
        assert_eq!(h.out.try_recv(), Ok(Ok(ConnectivityState::Offline)));
    }

    #[tokio::test(start_paused = true)]
    async fn flapping_back_reports_nothing_new() {
        let mut h = harness();
        h.raw.send(Ok(ConnectivityState::Full)).await.unwrap();
        sleep(Duration::from_millis(3100)).await;
        assert_eq!(h.out.try_recv(), Ok(Ok(ConnectivityState::Full)));

        h.raw.send(Ok(ConnectivityState::Offline)).await.unwrap();
        sleep(Duration::from_secs(1)).await;
        h.raw.send(Ok(ConnectivityState::Full)).await.unwrap();
        sleep(Duration::from_secs(10)).await;

        assert!(h.out.try_recv().is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn errors_pass_through_immediately() {
        let mut h = harness();
        h.raw
            .send(Err(ConnectivityError::UnknownValue(9)))
            .await
            .unwrap();

        sleep(Duration::from_millis(10)).await;
        assert_eq!(h.out.try_recv(), Ok(Err(ConnectivityError::UnknownValue(9))));
    }

    #[tokio::test(start_paused = true)]
    async fn an_error_does_not_cancel_a_pending_state() {
        let mut h = harness();
        h.raw.send(Ok(ConnectivityState::Offline)).await.unwrap();
        h.raw
            .send(Err(ConnectivityError::Bus("boom".into())))
            .await
            .unwrap();

        sleep(Duration::from_millis(3100)).await;
        assert_eq!(
            h.out.try_recv(),
            Ok(Err(ConnectivityError::Bus("boom".into())))
        );
        assert_eq!(h.out.try_recv(), Ok(Ok(ConnectivityState::Offline)));
    }

    #[tokio::test(start_paused = true)]
    async fn closed_source_reports_disconnected() {
        let mut h = harness();
        drop(h.raw);

        sleep(Duration::from_millis(10)).await;
        assert_eq!(h.out.try_recv(), Ok(Err(ConnectivityError::Disconnected)));
    }
}
