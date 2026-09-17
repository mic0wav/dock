mod niri;
mod ui;

use niri_ipc::{Event, Window};
use relm4::RelmApp;
use std::future::Future;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::runtime::Runtime;

/// builds the tokio runtime explicitly
pub(crate) fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| Runtime::new().expect("failed to start tokio runtime"))
}

/// helper that keeps things from failing quietly
pub fn spawn_supervised<Fut>(name: &'static str, task: Fut)
where
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    runtime().spawn(async move {
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
    runtime().spawn(async move {
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

/// maps one on one
fn map_event(event: Event) -> Vec<ui::DockMsg> {
    fn upsert(w: Window) -> ui::DockMsg {
        ui::DockMsg::WindowUpserted {
            id: w.id,
            app_id: w.app_id,
            title: w.title.unwrap_or_default(),
        }
    }

    match event {
        Event::WindowsChanged { windows } => windows.into_iter().map(upsert).collect(),
        Event::WindowOpenedOrChanged { window } => vec![upsert(window)],
        Event::WindowClosed { id } => vec![ui::DockMsg::WindowClosed { id }],
        Event::WindowFocusChanged { id } => vec![ui::DockMsg::WindowFocusedChanged { id }],

        _ => vec![],
    }
}

/// starts event listener and forwards events into dock components
pub fn start_niri_events(sender: relm4::Sender<ui::DockMsg>) {
    spawn_supervised("niri-events-bridge", async move {
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<Event>(64);
        niri::spawn_event_listener(event_tx);

        while let Some(event) = event_rx.recv().await {
            for msg in map_event(event) {
                if sender.send(msg).is_err() {
                    return Ok(());
                }
            }
        }
        Ok(())
    });
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let commands = runtime().block_on(niri::CommandClient::connect())?;

    let app = RelmApp::new("dev.example.niri-dock");
    app.run::<ui::DockModel>(commands);

    Ok(())
}
