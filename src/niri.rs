use niri_ipc::{Event, Reply, Request, Response};
use std::env;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, oneshot};

/// checks runtime niri socket
fn socket_path() -> anyhow::Result<String> {
    env::var(niri_ipc::socket::SOCKET_PATH_ENV).map_err(|_| {
        anyhow::anyhow!(
            "${} is not set - is niri running ?",
            niri_ipc::socket::SOCKET_PATH_ENV
        )
    })
}

async fn connect() -> anyhow::Result<UnixStream> {
    let path = socket_path()?;
    Ok(UnixStream::connect(path).await?)
}

/// one line in one line out communication
async fn request_once(stream: &mut UnixStream, req: &Request) -> anyhow::Result<Reply> {
    let mut line = serde_json::to_string(req)?;
    line.push('\n');
    stream.write_all(line.as_bytes()).await?;
    stream.flush().await?;

    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    reader.read_line(&mut buf).await?;
    Ok(serde_json::from_str(&buf)?)
}

/// provides a client which is cheap and safe to copy
#[derive(Clone)]
pub struct CommandClient {
    tx: mpsc::Sender<(Request, oneshot::Sender<anyhow::Result<Response>>)>,
}

impl CommandClient {
    /// connects and spaws a task that owns the command socket
    pub async fn connect() -> anyhow::Result<Self> {
        let stream = connect().await?;
        let (tx, rx) = mpsc::channel::<(Request, oneshot::Sender<anyhow::Result<Response>>)>(32);

        crate::spawn_supervised("niri-command", command_loop(stream, rx));
        Ok(Self { tx })
    }

    /// sends request and waits for reply
    pub async fn send(&self, req: Request) -> anyhow::Result<Response> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send((req, reply_tx))
            .await
            .map_err(|_| anyhow::anyhow!("niri command task is not running"))?;
        reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("niri command task dropped the reply"))?
    }
}

/// distinguishes a small failure from a niri disconnect
async fn command_loop(
    mut stream: UnixStream,
    mut rx: mpsc::Receiver<(Request, oneshot::Sender<anyhow::Result<Response>>)>,
) -> anyhow::Result<()> {
    while let Some((req, reply_tx)) = rx.recv().await {
        let result = match request_once(&mut stream, &req).await {
            Ok(reply) => reply.map_err(|e| anyhow::anyhow!(e)),
            Err(broken) => {
                log::warn!("niri command connection lost ({broken:#}), reconnecting");
                match connect().await {
                    Ok(new_stream) => {
                        stream = new_stream;
                        request_once(&mut stream, &req)
                            .await
                            .and_then(|reply| reply.map_err(|e| anyhow::anyhow!(e)))
                    }
                    Err(reconnect_failed) => Err(anyhow::anyhow!(
                        "niri command connection lost and reconnect failed: {reconnect_failed:#}"
                    )),
                }
            }
        };
        let _ = reply_tx.send(result);
    }
    Ok(())
}

/// connects to the event stream
pub fn spawn_event_listener(on_event: mpsc::Sender<Event>) {
    crate::spawn_supervised_restarting("niri-events", move || {
        let on_event = on_event.clone();
        async move {
            let mut stream = connect().await?;
            let reply = request_once(&mut stream, &Request::EventStream).await?;
            reply.map_err(|e| anyhow::anyhow!(e))?;

            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            loop {
                line.clear();
                let n = reader.read_line(&mut line).await?;
                if n == 0 {
                    anyhow::bail!("niri event stream closed");
                }
                let event: Event = serde_json::from_str(&line)?;
                if on_event.send(event).await.is_err() {
                    // this only happens during shutdown, cleanly
                    return Ok(());
                }
            }
        }
    });
}
