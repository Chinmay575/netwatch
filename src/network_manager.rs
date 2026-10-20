//! The only module that talks to D-Bus. Everything else is testable without it.

use futures_lite::stream::StreamExt;
use tokio::sync::mpsc;
use zbus::Connection;

use crate::{ConnectivityError, ConnectivityState, Update};

const RAW_CHANNEL_CAPACITY: usize = 16;

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager"
)]
trait NetworkManager {
    #[zbus(property)]
    fn connectivity(&self) -> zbus::Result<u32>;
}

fn bus_error(e: zbus::Error) -> ConnectivityError {
    ConnectivityError::Bus(e.to_string())
}

/// Connects to NetworkManager and returns a channel of raw (undebounced)
/// updates. The first item is the current state. The channel closes when the
/// D-Bus subscription ends.
pub(crate) async fn subscribe() -> Result<mpsc::Receiver<Update>, ConnectivityError> {
    let system = Connection::system().await.map_err(bus_error)?;
    let proxy = NetworkManagerProxy::new(&system).await.map_err(bus_error)?;

    // subscribe before the first read so no change can slip in between
    let mut changes = proxy.receive_connectivity_changed().await;
    let initial = proxy.connectivity().await.map_err(bus_error)?;

    let (tx, rx) = mpsc::channel(RAW_CHANNEL_CAPACITY);

    tokio::spawn(async move {
        if tx.send(ConnectivityState::try_from(initial)).await.is_err() {
            return;
        }
        while let Some(change) = changes.next().await {
            let update = match change.get().await {
                Ok(raw) => ConnectivityState::try_from(raw),
                Err(e) => Err(bus_error(e)),
            };
            if tx.send(update).await.is_err() {
                return;
            }
        }
        // keep the proxy (and its connection) alive for the whole subscription
        drop(proxy);
    });

    Ok(rx)
}
