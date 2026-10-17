use std::process::ExitCode;
use std::time::{Duration, Instant};

use futures_lite::stream::StreamExt;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};
use tokio::time::sleep_until;
use zbus::Connection;

mod connectivity_state;
mod debouncer;

use connectivity_state::ConnectivityState;
use debouncer::Debouncer;

type Reading = Result<ConnectivityState, String>;

const DEBOUNCE_WINDOW: Duration = Duration::from_secs(3);

#[zbus::proxy(
    interface = "org.freedesktop.NetworkManager",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager"
)]
trait NetworkManager {
    #[zbus(property)]
    fn connectivity(&self) -> zbus::Result<u32>;
}

fn main() -> ExitCode {
    // must run before the runtime starts: reading the local offset is only
    // sound while the process is single-threaded
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("netwatch: could not start async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };

    match runtime.block_on(monitor_connectivity(offset)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("netwatch: {e}");
            ExitCode::FAILURE
        }
    }
}

fn timestamp(offset: UtcOffset) -> String {
    let format = format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    OffsetDateTime::now_utc()
        .to_offset(offset)
        .format(&format)
        .unwrap_or_else(|_| "??".to_string())
}

fn describe(reading: &Reading) -> String {
    match reading {
        Ok(state) => format!("{state:?}"),
        Err(e) => format!("error: {e}"),
    }
}

async fn monitor_connectivity(offset: UtcOffset) -> Result<(), Box<dyn std::error::Error>> {
    let mut debouncer: Debouncer<Reading> = Debouncer::new(DEBOUNCE_WINDOW);
    let mut reported: Option<Reading> = None;

    let system = Connection::system().await?;
    let proxy = NetworkManagerProxy::new(&system).await?;

    // subscribe before the first read so no change can slip in between
    let mut stream = proxy.receive_connectivity_changed().await;

    let initial = proxy.connectivity().await?;
    debouncer.on_reading(ConnectivityState::try_from(initial), Instant::now());

    loop {
        let deadline = debouncer.deadline();

        tokio::select! {
            Some(change) = stream.next() => {
                let reading = match change.get().await {
                    Ok(raw) => ConnectivityState::try_from(raw),
                    Err(e) => Err(format!("could not read Connectivity: {e}")),
                };
                debouncer.on_reading(reading, Instant::now());
            }
            _ = sleep_until(tokio::time::Instant::from_std(deadline.unwrap_or_else(Instant::now))),
                if deadline.is_some() =>
            {
                if let Some(stable) = debouncer.on_timeout(Instant::now()) {
                    let from = reported.as_ref().map_or("start".to_string(), describe);
                    println!("[{}] {} -> {}", timestamp(offset), from, describe(&stable));
                    reported = Some(stable);
                }
            }
            else => break,
        }
    }

    Err("lost connection to NetworkManager".into())
}
