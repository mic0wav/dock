mod niri;

use niri_ipc::{Event, Request};
use std::future::Future;
use std::time::Duration;
use tokio;

/// helper that keeps things from failing quietly
pub fn spawn_supervised<Fut>(name: &'static str, task: Fut)
where
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    tokio::spawn(async move {
        match tokio::spawn(task).await {
            Ok(Ok(())) => log::info!("[{name}] exited cleanly"),
            Ok(Err(e)) => log::warn!("[{name}] task error {e:#}"),
            Err(join_err) => log::warn!("[{name}] task panicked: {join_err}"),
        }
    });
}

/// helper for restarting and logging repeating tasks
pub fn spawn_supervised_restarting<F, Fut>(name: &'static str, mut make_task: F)
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    tokio::spawn(async move {
        loop {
            match tokio::spawn(make_task()).await {
                Ok(Ok(())) => {
                    log::info!("[{name}] exited cleanly, not restarting");
                    break;
                }
                Ok(Err(e)) => log::warn!("[{name}] task error: {e:#}, restarting in 1s"),
                Err(join_err) => log::warn!("[{name}] task panicked: {join_err}, restarting in 1s"),
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let commands = niri::CommandClient::connect().await?;

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<Event>(64);
    niri::spawn_event_listener(event_tx);

    let windows = commands.send(Request::Windows).await?;
    log::info!("initial windows: {windows:?}");

    while let Some(event) = event_rx.recv().await {
        log::info!("event: {event:?}");
    }

    Ok(())
}
