//! The hub: one WebSocket server on localhost that every running app and every editor window
//! connects to. Apps report their tree and screens; the hub keeps the latest of each and forwards
//! them to every editor, and forwards each editor request to the app it names.
//!
//! An editor's [`EditorMsg::SetCode`] is answered by the hub itself: it reads the app's project
//! sources, finds the code that creates the control and writes the property there.
//!
//! The hub also remembers the apps that connected ([`HubOptions::recent_file`]) and runs or restarts
//! them on request ([`Launcher`]).
//!
//! The logic is designer-hubcore's [`HubCore`] (no IO, also run inside a browser page); this crate is
//! its native shell: sockets, the MCP server, running apps, phones on a cable, files on disk.
//!
//! The desktop editor starts a hub inside its own process ([`spawn`]); the VS Code extension
//! starts the same binary as a sidecar. A second hub on the same port fails to bind, and the
//! editor then just connects to the one already running.

use designer_protocol::Refusal;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};

use designer_hubcore::{CodeStore, HubCore, Out, Stamp, project_dir};
use designer_protocol::*;

mod mcp;
mod run;
pub mod usb;
mod adb;
pub use run::{DefaultLauncher, HubOptions, LaunchCommand, Launcher};
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;
use tokio_tungstenite::tungstenite::Message;

/// Someone in the hub (the MCP server) waiting for one message of one app.
struct Waiter {
    app: AppId,
    matches: Box<dyn Fn(&AppMsg) -> bool + Send>,
    tx: oneshot::Sender<AppMsg>,
}

struct Hub {
    core: HubCore,
    /// How to reach each connected app and editor.
    apps: HashMap<AppId, UnboundedSender<Message>>,
    editors: HashMap<u32, UnboundedSender<Message>>,
    waiters: Vec<Waiter>,
    /// Request ids of the hub's own requests, from 2^30 up: apart from the editors' small ones.
    next_request: u32,
    runs: run::Runs,
    /// The hub's port; apps on other devices connect to `port + 2`.
    port: u16,
    /// The secret apps on other devices put in their address (kept across hub restarts, so apps
    /// built with it keep connecting).
    token: String,
    /// While apps on other devices may connect: the task accepting them (their address is the core's).
    devices: Option<tokio::task::JoinHandle<()>>,
    /// Where the device port's accept loop hands its connections (served like any other, with
    /// the token check); a channel so that loop does not name `connection` (no recursive future).
    device_streams: Option<UnboundedSender<TcpStream>>,
}

impl Default for Hub {
    fn default() -> Self {
        Hub {
            core: HubCore::new(Arc::new(FsStore)),
            apps: HashMap::new(),
            editors: HashMap::new(),
            waiters: vec![],
            next_request: 0,
            runs: run::Runs::default(),
            port: 0,
            token: String::new(),
            devices: None,
            device_streams: None,
        }
    }
}

/// The project files on disk.
struct FsStore;

impl CodeStore for FsStore {
    fn list(&self, dir: &Path) -> Vec<(String, Stamp)> {
        let mut out = vec![];
        list_sources(dir, &mut out);
        out
    }

    fn read(&self, uri: &str) -> Result<Vec<u8>, String> {
        std::fs::read(uri).map_err(|e| e.to_string())
    }

    /// Through a temp file renamed over the original, so a crash never leaves half a file.
    fn write(&self, uri: &str, bytes: &[u8]) -> Result<(), String> {
        let tmp = format!("{uri}.designer-tmp");
        std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, uri)).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e.to_string()
        })
    }

    fn dir_exists(&self, dir: &Path) -> bool {
        dir.is_dir()
    }
}

/// Every `*.cs`, `*.tsx`, `*.jsx` and `*.rs` under `dir` with its size and modified time, without
/// build output and packages (`bin`, `obj`, `node_modules`, `dist`, `target`) and hidden folders. Linked folders
/// are followed (shared C# code), each folder once (a link back up never loops).
fn list_sources(dir: &Path, out: &mut Vec<(String, Stamp)>) {
    list_sources_in(dir, out, &mut std::collections::HashSet::new());
}

fn list_sources_in(dir: &Path, out: &mut Vec<(String, Stamp)>, seen: &mut std::collections::HashSet<std::path::PathBuf>) {
    if !seen.insert(std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf())) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        // The entry's own metadata costs no extra call; a link is followed to what it points at.
        let Some(meta) = e.metadata().ok().and_then(|m| if m.file_type().is_symlink() { std::fs::metadata(&path).ok() } else { Some(m) }) else { continue };
        if meta.is_dir() {
            if !matches!(name.as_str(), "bin" | "obj" | "node_modules" | "dist" | "target") && !name.starts_with('.') {
                list_sources_in(&path, out, seen);
            }
        } else if name.ends_with(".cs") || designer_sources::is_tsx(&name) || designer_sources::is_rust(&name) {
            let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as u64);
            out.push((path.to_string_lossy().to_string(), (meta.len(), modified)));
        }
    }
}

impl Hub {
    /// A hub with its runs, and the recent apps from their file.
    fn with_runs(runs: run::Runs) -> Hub {
        let mut h = Hub::default();
        h.core.recent = runs.load_recent();
        h.runs = runs;
        h
    }

    fn request_id(&mut self) -> u32 {
        self.next_request = self.next_request.max(1 << 30) + 1;
        self.next_request
    }

    fn send_to_app(&self, app: AppId, msg: &HubToApp) -> Result<(), String> {
        let tx = self.apps.get(&app).ok_or_else(|| format!("no app with id {app} is connected (see list_apps)"))?;
        tx.send(Message::text(serde_json::to_string(msg).expect("serializable"))).map_err(|_| format!("app {app} is disconnecting"))
    }

    fn broadcast(&self, msg: &HubToEditor) {
        let text = Message::text(serde_json::to_string(msg).expect("serializable"));
        for tx in self.editors.values() {
            let _ = tx.send(text.clone());
        }
    }

    /// Sends what the core produced. Its work runs off the async threads, and what that work
    /// produces is sent the same way.
    fn send_out(&self, hub: &Shared, outs: Vec<Out>) {
        for out in outs {
            match out {
                Out::ToApp(app, msg) => {
                    let _ = self.send_to_app(app, &msg);
                }
                Out::ToEditor(None, msg) => self.broadcast(&msg),
                Out::ToEditor(Some(editor), msg) => {
                    if let Some(tx) = self.editors.get(&editor) {
                        let _ = tx.send(Message::text(serde_json::to_string(&msg).expect("serializable")));
                    }
                }
                Out::Work(work) => {
                    let hub = hub.clone();
                    tokio::task::spawn_blocking(move || {
                        let done = work.run();
                        let mut h = hub.lock().unwrap();
                        let outs = h.core.done(done);
                        h.send_out(&hub, outs);
                    });
                }
            }
        }
    }

    /// An app's connection closed: whoever waits on it hears so now, editors get the app list.
    fn app_gone(&mut self, hub: &Shared, id: AppId) {
        self.apps.remove(&id);
        // A dropped waiter's channel closes: its `ask` answers "disconnected" at once.
        self.waiters.retain(|w| w.app != id);
        let outs = self.core.app_closed(id);
        self.send_out(hub, outs);
    }
}

type Shared = Arc<Mutex<Hub>>;

/// A phone was plugged in or out: the count for that kind changes and every editor hears it.
fn set_cables(hub: &Shared, android: Option<u32>, ios: Option<u32>) {
    let mut h = hub.lock().unwrap();
    let outs = h.core.set_cables(android, ios);
    h.send_out(hub, outs);
}

/// Sends the messages `make` builds (given a fresh request id) to an app, and waits up to `timeout`
/// for the first message of that app `matches` accepts.
async fn ask(
    hub: &Shared,
    app: AppId,
    make: impl FnOnce(u32) -> Vec<HubToApp>,
    matches: impl Fn(u32, &AppMsg) -> bool + Send + 'static,
    timeout: std::time::Duration,
) -> Result<AppMsg, String> {
    let (tx, rx) = oneshot::channel();
    {
        let mut h = hub.lock().unwrap();
        let request = h.request_id();
        h.waiters.push(Waiter { app, matches: Box::new(move |m| matches(request, m)), tx });
        // The app's record of live changes follows what the hub sends on its own too.
        if let Err(e) = make(request).iter().try_for_each(|m| {
            h.core.to_app(app, m);
            h.send_to_app(app, m)
        }) {
            h.waiters.pop();
            return Err(e);
        }
    }
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(msg)) => Ok(msg),
        Ok(Err(_)) => Err(format!("app {app} disconnected")),
        Err(_) => Err(format!("app {app} did not answer within {} s", timeout.as_secs_f32())),
    }
}

/// Binds the hub on `127.0.0.1:port`. Fails when the port is taken (another hub runs there).
pub async fn bind(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await
}

/// Serves connections on a bound listener until the process ends.
pub async fn serve(listener: TcpListener) {
    serve_with_mcp(listener, None).await
}

/// Serves the hub, and when `mcp` is given its MCP server for AI agents on that listener.
pub async fn serve_with_mcp(listener: TcpListener, mcp: Option<TcpListener>) {
    serve_with(listener, mcp, HubOptions::default()).await
}

/// [`serve_with_mcp`] with where recent apps and run logs are kept and what runs apps.
pub async fn serve_with(listener: TcpListener, mcp: Option<TcpListener>, options: HubOptions) {
    let url = listener.local_addr().map(|a| format!("ws://{a}")).unwrap_or_default();
    let port = listener.local_addr().map_or(designer_protocol::DEFAULT_PORT, |a| a.port());
    let token = device_token(options.recent_file.as_deref().and_then(std::path::Path::parent));
    let usbmux = options.usbmux.clone();
    let hub = Shared::new(Mutex::new(Hub { port, token, ..Hub::with_runs(run::Runs::new(options, url)) }));
    if let Some(addr) = usbmux {
        tokio::spawn(usb::watch(hub.clone(), addr));
        // Android phones on the cable: only the designer's own hub (see adb.rs).
        if port == designer_protocol::DEFAULT_PORT {
            tokio::spawn(adb::watch(hub.clone(), port));
        }
    }
    serve_hub(hub, listener, mcp).await
}

async fn serve_hub(hub: Shared, listener: TcpListener, mcp: Option<TcpListener>) {
    if let Some(mcp) = mcp {
        tokio::spawn(mcp::serve(hub.clone(), mcp));
    }
    let (streams, mut incoming) = unbounded_channel::<TcpStream>();
    let token = {
        let mut h = hub.lock().unwrap();
        h.device_streams = Some(streams);
        h.token.clone()
    };
    let devices = hub.clone();
    tokio::spawn(async move {
        while let Some(stream) = incoming.recv().await {
            tokio::spawn(connection(devices.clone(), stream, Some(token.clone())));
        }
    });
    while let Ok((stream, _)) = listener.accept().await {
        tokio::spawn(connection(hub.clone(), stream, None));
    }
}

fn device_token(dir: Option<&std::path::Path>) -> String {
    let file = dir.map(|d| d.join("device-token"));
    if let Some(token) = file.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).map(|t| t.trim().to_owned()).filter(|t| t.len() >= 32) {
        return token;
    }
    // ponytail: SipHash keyed by RandomState's OS-seeded keys, 128 bits; switch to getrandom if a dependency brings it.
    use std::hash::{BuildHasher, Hasher};
    let part = |n: u64| {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(n ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64));
        h.finish()
    };
    let token = format!("{:016x}{:016x}", part(1), part(2));
    if let Some(f) = file {
        let _ = std::fs::create_dir_all(f.parent().unwrap_or(std::path::Path::new("."))).and_then(|_| std::fs::write(&f, &token));
    }
    token
}

/// This computer's address on its network: the one the system would send from to the internet
/// (a UDP socket only picks the route; nothing is sent).
fn lan_ip() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|a| a.ip()).filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
}

/// Opens or closes the port for apps on other devices; tells every editor the result and returns
/// it: the address apps connect to, or why it could not be opened.
pub(crate) async fn allow_devices(hub: &Shared, on: bool) -> (Option<String>, Option<Refusal>) {
    let (port, token, open, streams) = {
        let mut h = hub.lock().unwrap();
        if !on {
            if let Some(task) = h.devices.take() {
                task.abort();
            }
            h.core.devices = None;
        }
        (h.port, h.token.clone(), h.devices.is_some(), h.device_streams.clone())
    };
    let mut refusal = None;
    if on && !open {
        let device_port = port.wrapping_add(2);
        match (lan_ip(), TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], device_port))).await) {
            (Some(ip), Ok(listener)) => {
                let url = format!("ws://{ip}:{device_port}/?token={token}");
                let task = tokio::spawn(async move {
                    while let Ok((stream, _)) = listener.accept().await {
                        if streams.as_ref().is_none_or(|s| s.send(stream).is_err()) {
                            break;
                        }
                    }
                });
                let mut h = hub.lock().unwrap();
                h.devices = Some(task);
                h.core.devices = Some(url);
            }
            (None, _) => refusal = Some(Refusal::new("hub-no-network", &[])),
            (_, Err(e)) => refusal = Some(Refusal::new("hub-port-taken", &[("port", &device_port), ("error", &e)])),
        }
    }
    let h = hub.lock().unwrap();
    let url = h.core.devices.clone();
    h.broadcast(&HubToEditor::Devices { url: url.clone(), refusal: refusal.clone() });
    (url, refusal)
}

/// The `token` value of a request's query.
fn query_token(uri: &str) -> Option<&str> {
    uri.split_once('?')?.1.split('&').find_map(|pair| pair.strip_prefix("token="))
}

/// Starts a hub on its own thread, with its MCP server on `port + 1`
/// ([`DEFAULT_MCP_PORT`] for the default port). Returns `false` when the hub port is already
/// taken, which means a hub is running there already. A taken MCP port leaves the hub without MCP.
pub fn spawn(port: u16) -> bool {
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("tokio runtime");
    let Ok(listener) = runtime.block_on(bind(port)) else { return false };
    let mcp = runtime.block_on(bind(port.wrapping_add(1))).inspect_err(|e| eprintln!("designer hub: no MCP server on port {}: {e}", port.wrapping_add(1))).ok();
    std::thread::Builder::new()
        .name("designer-hub".into())
        .spawn(move || runtime.block_on(serve_with_mcp(listener, mcp)))
        .expect("hub thread");
    true
}

/// One connection. `token`: it came to the port for other devices and must carry this token; such
/// connections may only be apps.
async fn connection(hub: Shared, stream: TcpStream, token: Option<String>) {
    use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
    let device = token.is_some();
    let ws = match token {
        None => tokio_tungstenite::accept_async(stream).await,
        Some(token) => {
            tokio_tungstenite::accept_hdr_async(stream, move |req: &Request, resp: Response| {
                if query_token(&req.uri().to_string()) == Some(token.as_str()) {
                    Ok(resp)
                } else {
                    let mut refused = ErrorResponse::new(Some("wrong or missing token".into()));
                    *refused.status_mut() = tokio_tungstenite::tungstenite::http::StatusCode::UNAUTHORIZED;
                    Err(refused)
                }
            })
            .await
        }
    };
    let Ok(ws) = ws else { return };
    session(hub, ws, device).await
}

/// A connection after its WebSocket handshake, whichever side made it (an app or editor that
/// connected to the hub, or an app on a phone the hub reached over the cable). `device`: it is not
/// from this computer, so it may only be an app.
pub(crate) async fn session<S>(hub: Shared, ws: tokio_tungstenite::WebSocketStream<S>, device: bool)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut sink, mut source) = ws.split();
    let (tx, mut rx) = unbounded_channel::<Message>();
    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sink.send(msg).await.is_err() {
                break;
            }
        }
    });

    let hello = match source.next().await {
        Some(Ok(Message::Text(text))) => serde_json::from_str::<Hello>(&text).ok(),
        _ => None,
    };
    match hello {
        Some(Hello::App(info)) => {
            let id = {
                let mut h = hub.lock().unwrap();
                match h.core.app_hello(info) {
                    Some((id, outs)) => {
                        h.apps.insert(id, tx);
                        h.app_connected(id);
                        h.send_out(&hub, outs);
                        Some(id)
                    }
                    None => None,
                }
            };
            if let Some(id) = id {
                while let Some(Ok(frame)) = source.next().await {
                    let Message::Text(text) = frame else { continue };
                    let Ok(msg) = serde_json::from_str::<AppMsg>(&text) else { continue };
                    let mut h = hub.lock().unwrap();
                    // Hand the message to the hub's own waiters it answers.
                    let (done, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut h.waiters).into_iter().partition(|w| w.app == id && (w.matches)(&msg));
                    h.waiters = waiting;
                    for w in done {
                        let _ = w.tx.send(msg.clone());
                    }
                    let outs = h.core.app_msg(id, msg);
                    h.send_out(&hub, outs);
                }
                hub.lock().unwrap().app_gone(&hub, id);
            }
        }
        Some(Hello::Editor { protocol }) if !device => {
            let id = {
                let mut h = hub.lock().unwrap();
                match h.core.editor_hello(&protocol) {
                    Some((id, outs)) => {
                        h.editors.insert(id, tx.clone());
                        h.send_out(&hub, outs);
                        Some(id)
                    }
                    None => None,
                }
            };
            if let Some(id) = id {
                while let Some(Ok(frame)) = source.next().await {
                    let Message::Text(text) = frame else { continue };
                    let Ok(msg) = serde_json::from_str::<EditorMsg>(&text) else { continue };
                    match msg {
                        EditorMsg::RunApp { project } => {
                            if let Err(refusal) = run::run_app(&hub, &project) {
                                let msg = HubToEditor::AppRunState { project, state: RunState::Failed, message: refusal.to_string(), refusal: Some(refusal) };
                                let _ = tx.send(Message::text(serde_json::to_string(&msg).expect("serializable")));
                            }
                        }
                        EditorMsg::AllowDevices { on } => {
                            allow_devices(&hub, on).await;
                        }
                        EditorMsg::RestartApp { app } => {
                            let project = hub.lock().unwrap().core.apps.get(&app).and_then(|a| a.info.project.clone()).unwrap_or_default();
                            if let Err(refusal) = run::restart_app(&hub, app) {
                                let msg = HubToEditor::AppRunState { project, state: RunState::Failed, message: refusal.to_string(), refusal: Some(refusal) };
                                let _ = tx.send(Message::text(serde_json::to_string(&msg).expect("serializable")));
                            }
                        }
                        msg => {
                            let mut h = hub.lock().unwrap();
                            let outs = h.core.editor_msg(id, msg);
                            h.send_out(&hub, outs);
                        }
                    }
                }
                let mut h = hub.lock().unwrap();
                h.editors.remove(&id);
                h.core.editor_closed(id);
            }
        }
        _ => {}
    }
    writer.abort();
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A hub that keeps recent apps in memory only: tests never touch the user's recent file.
    async fn serve_test(listener: TcpListener, mcp: Option<TcpListener>) {
        serve_with(listener, mcp, HubOptions { recent_file: None, log_dir: None, launcher: std::sync::Arc::new(DefaultLauncher), run_timeout: run::RUN_TIMEOUT, usbmux: None }).await
    }
    use tokio_tungstenite::connect_async;

    async fn send<T: serde::Serialize>(ws: &mut (impl SinkExt<Message> + Unpin), value: &T) {
        let _ = ws.send(Message::text(serde_json::to_string(value).unwrap())).await;
    }

    async fn next_editor_msg(ws: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin)) -> HubToEditor {
        loop {
            if let Some(Ok(Message::Text(text))) = ws.next().await {
                // The phones-on-cable count comes on connect; these tests are about other messages.
                match serde_json::from_str(&text).unwrap() {
                    HubToEditor::Cables { .. } => continue,
                    msg => return msg,
                }
            }
        }
    }

    #[test]
    fn the_device_token_comes_from_the_query() {
        assert_eq!(query_token("/?token=abc"), Some("abc"));
        assert_eq!(query_token("/?x=1&token=abc"), Some("abc"));
        assert_eq!(query_token("/"), None);
        let dir = std::env::temp_dir().join(format!("designer-token-test-{}", std::process::id()));
        let first = device_token(Some(&dir));
        assert_eq!(first.len(), 32);
        assert_eq!(device_token(Some(&dir)), first, "kept across hub restarts");
        assert_ne!(device_token(None), first);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The device port's connections (served here on loopback, as the device task hands them over):
    /// no token or a wrong one is refused, an app with the token connects, an editor never does.
    #[tokio::test]
    async fn apps_on_other_devices_need_the_token() {
        let token = "0123456789abcdef0123456789abcdef".to_owned();
        let hub = Shared::new(Mutex::new(Hub { token: token.clone(), ..Default::default() }));
        let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let devices = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (local_url, device_addr) = (format!("ws://{}", local.local_addr().unwrap()), devices.local_addr().unwrap());
        let shared = hub.clone();
        tokio::spawn(async move {
            while let Ok((s, _)) = local.accept().await {
                tokio::spawn(connection(shared.clone(), s, None));
            }
        });
        let shared = hub.clone();
        let accept_token = token.clone();
        tokio::spawn(async move {
            while let Ok((s, _)) = devices.accept().await {
                tokio::spawn(connection(shared.clone(), s, Some(accept_token.clone())));
            }
        });
        assert!(connect_async(format!("ws://{device_addr}/")).await.is_err(), "no token");
        assert!(connect_async(format!("ws://{device_addr}/?token=nope")).await.is_err(), "wrong token");

        let (mut editor, _) = connect_async(&local_url).await.unwrap();
        send(&mut editor, &Hello::Editor { protocol: PROTOCOL_VERSION.into() }).await;
        let HubToEditor::Apps { apps, .. } = next_editor_msg(&mut editor).await else { panic!("apps first") };
        assert!(apps.is_empty());

        let (mut phone, _) = connect_async(format!("ws://{device_addr}/?token={token}")).await.unwrap();
        let info = AppInfo { protocol: PROTOCOL_VERSION.into(), name: "Phone".into(), engine: "dotnet-maui".into(), pid: 9, project: None, agent: None, platform: None };
        send(&mut phone, &Hello::App(info)).await;
        let HubToEditor::Apps { apps, .. } = next_editor_msg(&mut editor).await else { panic!("apps") };
        assert_eq!(apps.iter().map(|a| a.info.name.as_str()).collect::<Vec<_>>(), ["Phone"]);

        // An editor on the device port is dropped, even with the token.
        let (mut stranger, _) = connect_async(format!("ws://{device_addr}/?token={token}")).await.unwrap();
        send(&mut stranger, &Hello::Editor { protocol: PROTOCOL_VERSION.into() }).await;
        let got = tokio::time::timeout(std::time::Duration::from_secs(2), stranger.next()).await;
        assert!(matches!(got, Ok(None) | Ok(Some(Err(_))) | Ok(Some(Ok(Message::Close(_))))), "{got:?}");
        assert_eq!(hub.lock().unwrap().editors.len(), 1);
    }

    /// An app that comes back (same name, engine, pid, platform) gets its old id again; two alike
    /// apps at once keep apart.
    #[tokio::test]
    async fn a_reconnecting_app_keeps_its_id() {
        let listener = bind(0).await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        tokio::spawn(serve_test(listener, None));
        let (mut editor, _) = connect_async(&url).await.unwrap();
        send(&mut editor, &Hello::Editor { protocol: PROTOCOL_VERSION.into() }).await;
        macro_rules! ids {
            () => {
                loop {
                    if let HubToEditor::Apps { apps, .. } = next_editor_msg(&mut editor).await {
                        break apps.iter().map(|a| a.id).collect::<Vec<_>>();
                    }
                }
            };
        }
        assert!(ids!().is_empty());
        let info = AppInfo { protocol: PROTOCOL_VERSION.into(), name: "Phone".into(), engine: "dotnet-maui".into(), pid: 4181, project: None, agent: None, platform: Some("ios".into()) };
        let (mut app, _) = connect_async(&url).await.unwrap();
        send(&mut app, &Hello::App(info.clone())).await;
        let first = ids!();
        assert_eq!(first.len(), 1);
        app.close(None).await.unwrap();
        assert!(ids!().is_empty());
        let (mut back, _) = connect_async(&url).await.unwrap();
        send(&mut back, &Hello::App(info.clone())).await;
        assert_eq!(ids!(), first, "the same id again");
        let (mut twin, _) = connect_async(&url).await.unwrap();
        send(&mut twin, &Hello::App(info)).await;
        let both = ids!();
        assert_eq!(both.len(), 2, "{both:?}");
        assert_ne!(both[0], both[1]);
    }

    /// An MCP call waiting on an app that goes away answers at once, not after its full wait.
    #[tokio::test]
    async fn a_closing_app_drops_its_waiters() {
        let hub = Shared::new(Mutex::new(Hub::default()));
        let (tx, _rx) = unbounded_channel();
        let id = {
            let mut h = hub.lock().unwrap();
            let info = AppInfo { protocol: PROTOCOL_VERSION.into(), name: "Demo".into(), engine: "test".into(), pid: 1, project: None, agent: None, platform: None };
            let (id, _) = h.core.app_hello(info).unwrap();
            h.apps.insert(id, tx);
            id
        };
        let shared = hub.clone();
        let asking = tokio::spawn(async move { ask(&shared, id, |_| vec![HubToApp::GetTree], |_, m| matches!(m, AppMsg::Tree(_)), std::time::Duration::from_secs(5)).await });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(hub.lock().unwrap().waiters.len(), 1);
        let started = std::time::Instant::now();
        hub.lock().unwrap().app_gone(&hub, id);
        assert_eq!(asking.await.unwrap(), Err(format!("app {id} disconnected")));
        assert!(started.elapsed() < std::time::Duration::from_secs(1), "{:?}", started.elapsed());
    }

    #[tokio::test]
    async fn an_app_tree_reaches_the_editor_and_an_editor_request_reaches_the_app() {
        let listener = bind(0).await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        tokio::spawn(serve_test(listener, None));

        let (mut app, _) = connect_async(&url).await.unwrap();
        let info = AppInfo { protocol: PROTOCOL_VERSION.into(), name: "Test".into(), engine: "test".into(), pid: 1, project: None, agent: None, platform: None };
        send(&mut app, &Hello::App(info)).await;
        let tree = Tree { revision: 1, width: 100.0, height: 50.0, nodes: vec![Node { id: 7, type_name: "SkiaLabel".into(), ..Default::default() }] };
        send(&mut app, &AppMsg::Tree(tree.clone())).await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // An editor that connects later still gets the app and its last tree.
        let (mut editor, _) = connect_async(&url).await.unwrap();
        send(&mut editor, &Hello::Editor { protocol: PROTOCOL_VERSION.into() }).await;
        let HubToEditor::Apps { apps, .. } = next_editor_msg(&mut editor).await else { panic!("apps first") };
        assert_eq!(apps.len(), 1);
        let app_id = apps[0].id;
        assert_eq!(next_editor_msg(&mut editor).await, HubToEditor::FromApp { app: app_id, msg: AppMsg::Tree(tree) });

        send(&mut editor, &EditorMsg::ToApp { app: app_id, msg: HubToApp::Pick { request: 3, x: 10.0, y: 20.0 } }).await;
        let got = loop {
            if let Some(Ok(Message::Text(text))) = app.next().await {
                break serde_json::from_str::<HubToApp>(&text).unwrap();
            }
        };
        assert_eq!(got, HubToApp::Pick { request: 3, x: 10.0, y: 20.0 });

        // The app leaves: the editor hears an empty app list.
        drop(app);
        assert_eq!(next_editor_msg(&mut editor).await, HubToEditor::Apps { apps: vec![], recent: vec![] });
    }

    async fn set_code_answer(editor: &mut (impl SinkExt<Message> + StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin), msg: EditorMsg) -> HubToEditor {
        send(editor, &msg).await;
        loop {
            if let m @ (HubToEditor::CodeEdited { .. } | HubToEditor::CodeRefused { .. } | HubToEditor::CodeUndone { .. }) = next_editor_msg(editor).await {
                break m;
            }
        }
    }

    /// A React app's folder: its own sources are read, packages and build output are not.
    #[test]
    fn sources_leave_out_packages_and_build_output() {
        let dir = std::env::temp_dir().join(format!("designer-list-test-{}", std::process::id()));
        for sub in ["src", "node_modules/drawnui-react/src", "dist", "obj"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        std::fs::write(dir.join("src/App.tsx"), "export default <SkiaLabel/>;").unwrap();
        std::fs::write(dir.join("node_modules/drawnui-react/src/SkiaShell.tsx"), "x").unwrap();
        std::fs::write(dir.join("dist/App.tsx"), "x").unwrap();
        std::fs::write(dir.join("obj/G.cs"), "x").unwrap();
        let mut out = vec![];
        super::list_sources(&dir, &mut out);
        let names: Vec<String> = out.iter().map(|(p, _)| p.replace('\\', "/")).collect();
        assert_eq!(names.len(), 1, "{names:?}");
        assert!(names[0].ends_with("src/App.tsx"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn set_code_writes_one_literal_and_keeps_bom_and_line_endings() {
        let dir = std::env::temp_dir().join(format!("designer-hub-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("obj")).unwrap();
        std::fs::write(dir.join("Demo.csproj"), "<Project />").unwrap();
        let page = "namespace Demo;\r\npublic class Page : SkiaLayer\r\n{\r\n    public Page()\r\n    {\r\n        Children = new List<SkiaControl>\r\n        {\r\n            new SkiaLabel { Text = \"Hé\", FontSize = 18 },\r\n        };\r\n    }\r\n}\r\n";
        let path = dir.join("Page.cs");
        std::fs::write(&path, [&[0xEF, 0xBB, 0xBF][..], page.as_bytes()].concat()).unwrap();
        // Generated code under obj is not read: a second `Page` there would make the match ambiguous.
        std::fs::write(dir.join("obj").join("Page.g.cs"), "namespace Demo; public class Page { public Page() { X = 1; } }").unwrap();

        let listener = bind(0).await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        tokio::spawn(serve_test(listener, None));
        let (mut app, _) = connect_async(&url).await.unwrap();
        let project = dir.join("Demo.csproj").to_string_lossy().to_string();
        let info = AppInfo { protocol: PROTOCOL_VERSION.into(), name: "Demo".into(), engine: "test".into(), pid: 1, project: Some(project), agent: None, platform: None };
        send(&mut app, &Hello::App(info)).await;
        let nodes = vec![
            Node { id: 1, type_name: "Demo.Page".into(), ..Default::default() },
            Node { id: 2, parent: Some(1), type_name: "DrawnUi.Draw.SkiaLabel".into(), ..Default::default() },
        ];
        send(&mut app, &AppMsg::Tree(Tree { revision: 1, width: 100.0, height: 100.0, nodes })).await;
        let prop = |name: &str, t: &str, v: &str| Prop { name: name.into(), type_name: t.into(), value: v.into(), is_set: true, ..Default::default() };
        let props = vec![
            prop("FontSize", "System.Double", "18"),
            prop("TextColor", "DrawnUi.Color", "#FFFFFFFF"),
            prop("FillGradient", "DrawnUi.Draw.SkiaGradient", "SkiaGradient"),
        ];
        send(&mut app, &AppMsg::Props { node: 2, props }).await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let (mut editor, _) = connect_async(&url).await.unwrap();
        send(&mut editor, &Hello::Editor { protocol: PROTOCOL_VERSION.into() }).await;
        let HubToEditor::Apps { apps, .. } = next_editor_msg(&mut editor).await else { panic!("apps first") };
        let set = |request: u32, prop: &str, value: &str| EditorMsg::SetCode { request, app: apps[0].id, node: 2, prop: prop.into(), value: value.into() };

        let answer = set_code_answer(&mut editor, set(1, "FontSize", "20")).await;
        let HubToEditor::CodeEdited { request: 1, node: 2, uri, range } = answer else { panic!("{answer:?}") };
        assert_eq!(uri, path.to_string_lossy());
        assert_eq!((range.start.line, range.start.character, range.end.line, range.end.character), (7, 52, 7, 54));
        let expected = [&[0xEF, 0xBB, 0xBF][..], page.replace("FontSize = 18", "FontSize = 20").as_bytes()].concat();
        assert_eq!(std::fs::read(&path).unwrap(), expected);

        let HubToEditor::CodeRefused { reason, .. } = set_code_answer(&mut editor, set(2, "FillGradient", "x")).await else { panic!("gradient written") };
        assert!(reason.contains("SkiaGradient"), "{reason}");
        let HubToEditor::CodeRefused { reason, .. } = set_code_answer(&mut editor, set(3, "Opacity", "0.5")).await else { panic!("untyped written") };
        assert!(reason.contains("Opacity"), "{reason}");
        assert_eq!(std::fs::read(&path).unwrap(), expected);

        // A color the label does not set yet: a new assignment in its initializer.
        let answer = set_code_answer(&mut editor, set(4, "TextColor", "#FF0000")).await;
        assert!(matches!(answer, HubToEditor::CodeEdited { request: 4, .. }), "{answer:?}");
        let expected = [&[0xEF, 0xBB, 0xBF][..], page.replace("FontSize = 18", "FontSize = 20, TextColor = Color.Parse(\"#FF0000\")").as_bytes()].concat();
        assert_eq!(std::fs::read(&path).unwrap(), expected);

        // Undo, newest first: the added assignment goes, then the literal is 18 again; the file is
        // byte for byte what it was. A second undo of the same request has nothing to take back.
        let undo = |request: u32| EditorMsg::UndoCode { request };
        let answer = set_code_answer(&mut editor, undo(4)).await;
        assert!(matches!(answer, HubToEditor::CodeUndone { request: 4, .. }), "{answer:?}");
        let answer = set_code_answer(&mut editor, undo(1)).await;
        assert!(matches!(answer, HubToEditor::CodeUndone { request: 1, .. }), "{answer:?}");
        assert_eq!(std::fs::read(&path).unwrap(), [&[0xEF, 0xBB, 0xBF][..], page.as_bytes()].concat());
        let answer = set_code_answer(&mut editor, undo(1)).await;
        assert!(matches!(answer, HubToEditor::CodeRefused { .. }), "{answer:?}");
        // The file changed at the written span since: the undo refuses and keeps the file.
        let answer = set_code_answer(&mut editor, set(5, "FontSize", "30")).await;
        assert!(matches!(answer, HubToEditor::CodeEdited { .. }), "{answer:?}");
        let edited = std::fs::read_to_string(&path).unwrap().replace("FontSize = 30", "FontSize = 31");
        std::fs::write(&path, &edited).unwrap();
        let answer = set_code_answer(&mut editor, undo(5)).await;
        assert!(matches!(answer, HubToEditor::CodeRefused { .. }), "{answer:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The byte offset of a line and UTF-16 column.
    fn byte_at(text: &str, p: source::TextPosition) -> usize {
        let start: usize = text.split_inclusive('\n').take(p.line as usize).map(str::len).sum();
        let mut units = 0;
        for (i, c) in text[start..].char_indices() {
            if units >= p.character {
                return start + i;
            }
            units += c.len_utf16() as u32;
        }
        text.len()
    }

    fn copy_sources(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let (p, name) = (e.path(), e.file_name().to_string_lossy().to_string());
            if p.is_dir() && name != "bin" && name != "obj" {
                copy_sources(&p, &to.join(&name));
            } else if name.ends_with(".cs") || name.ends_with(".csproj") {
                std::fs::copy(&p, to.join(&name)).unwrap();
            }
        }
    }

    /// A temp copy of HelloWpf and its recorded LayoutsPage tree: a helper-body color, a helper
    /// `Color.Parse` and a `new Thickness(16, 12)` change in place, nothing else in the file moves.
    #[tokio::test]
    async fn set_code_round_trips_on_a_hello_wpf_copy() {
        // The DrawnUi repository: DRAWNUI_REPO, or a DrawnUi folder next to this repository.
        let repo = std::env::var_os("DRAWNUI_REPO").map(std::path::PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join(r"..\..\..\DrawnUi"));
        let hello = &repo.join(r"src\Wpf\Samples\HelloWpf");
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join(r"..\..\fixtures\hellowpf\layouts.json");
        let (true, Ok(json)) = (hello.exists(), std::fs::read_to_string(&fixture)) else {
            eprintln!("skipped: HelloWpf or the layouts fixture is missing");
            return;
        };
        let dir = std::env::temp_dir().join(format!("designer-hub-hello-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_sources(hello, &dir);
        let tree: Tree = serde_json::from_str(&json).unwrap();
        let files = designer_hubcore::read_sources(&FsStore, &dir);
        let texts: Vec<(String, String)> = files.iter().map(|f| (f.uri.clone(), f.text.clone())).collect();
        let matches = designer_sources::match_runtime(&texts, &tree);
        let node_of = |key: &str| matches.iter().find(|m| m.key == key).unwrap_or_else(|| panic!("no node for {key}")).node;
        let heading = node_of("LayoutsPage.Heading/0");
        let card = node_of("LayoutsPage.Card/0");
        let stack = node_of("LayoutsPage.Card/0/Children/0");

        let listener = bind(0).await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        tokio::spawn(serve_test(listener, None));
        let (mut app, _) = connect_async(&url).await.unwrap();
        let project = dir.join("HelloWpf.csproj").to_string_lossy().to_string();
        let info = AppInfo { protocol: PROTOCOL_VERSION.into(), name: "HelloWpf".into(), engine: "dotnet-wpf".into(), pid: 1, project: Some(project), agent: None, platform: None };
        send(&mut app, &Hello::App(info)).await;
        send(&mut app, &AppMsg::Tree(tree)).await;
        let prop = |name: &str, t: &str, v: &str| Prop { name: name.into(), type_name: t.into(), value: v.into(), is_set: true, ..Default::default() };
        send(&mut app, &AppMsg::Props { node: heading, props: vec![prop("TextColor", "DrawnUi.Color", "#FFFFFFFF")] }).await;
        send(&mut app, &AppMsg::Props { node: card, props: vec![prop("BackgroundColor", "DrawnUi.Color", "#FF2B3035")] }).await;
        send(&mut app, &AppMsg::Props { node: stack, props: vec![prop("Padding", "DrawnUi.Views.Thickness", "16,12,16,12")] }).await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let (mut editor, _) = connect_async(&url).await.unwrap();
        send(&mut editor, &Hello::Editor { protocol: PROTOCOL_VERSION.into() }).await;
        let HubToEditor::Apps { apps, .. } = next_editor_msg(&mut editor).await else { panic!("apps first") };
        let page = dir.join("Pages").join("LayoutsPage.cs");
        // Heading and Card are helpers used many times: the value goes on the edited call, never into
        // the helper.
        for (request, (node, prop, value, old, new)) in [
            (heading, "TextColor", "#FFFF0000", "", ".Adapt(me => me.TextColor = Color.Parse(\"#FF0000\"))"),
            (card, "BackgroundColor", "#FF112233", "", ".Adapt(me => ((SkiaShape)me).BackgroundColor = Color.Parse(\"#112233\"))"),
        ]
        .into_iter()
        .enumerate()
        {
            let before = std::fs::read(&page).unwrap();
            let msg = EditorMsg::SetCode { request: request as u32, app: apps[0].id, node, prop: prop.into(), value: value.into() };
            let answer = set_code_answer(&mut editor, msg).await;
            let HubToEditor::CodeEdited { uri, range, .. } = answer else { panic!("{prop}: {answer:?}") };
            assert_eq!(uri, page.to_string_lossy());
            let after = std::fs::read(&page).unwrap();
            let (b_text, a_text) = (String::from_utf8_lossy(&before).to_string(), String::from_utf8_lossy(&after).to_string());
            let (s, e) = (byte_at(&a_text, range.start), byte_at(&a_text, range.end));
            assert_eq!(&a_text[s..e], new, "{prop}");
            // Everything else is byte-identical (BOM and line endings included).
            assert_eq!(&after[..s], &before[..s], "{prop}");
            assert_eq!(&after[e..], &before[s + old.len()..], "{prop}");
            assert_eq!(&b_text[s..s + old.len()], old, "{prop}");
        }
        // Inside Card, used many times: refused, the file untouched.
        let before = std::fs::read(&page).unwrap();
        let answer = set_code_answer(&mut editor, EditorMsg::SetCode { request: 9, app: apps[0].id, node: stack, prop: "Padding".into(), value: "8".into() }).await;
        assert!(matches!(&answer, HubToEditor::CodeRefused { refusal: Some(r), .. } if r.key == "src-shared-helper"), "{answer:?}");
        assert_eq!(std::fs::read(&page).unwrap(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A fake app on the hub: answers getTree, getProps and setProperty (fails for `Bogus`) like the
    /// .NET agent, from a temp project with one page and one label.
    async fn fake_app(url: &str, dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("Demo.csproj"), "<Project />").unwrap();
        let page = "namespace Demo;\npublic class Page : SkiaLayer\n{\n    public Page()\n    {\n        Children = new List<SkiaControl>\n        {\n            new SkiaLabel { Text = \"Hi\", FontSize = 18 },\n        };\n    }\n}\n";
        std::fs::write(dir.join("Page.cs"), page).unwrap();
        let (mut ws, _) = connect_async(url).await.unwrap();
        let project = dir.join("Demo.csproj").to_string_lossy().to_string();
        send(&mut ws, &Hello::App(AppInfo { protocol: PROTOCOL_VERSION.into(), name: "Demo".into(), engine: "test".into(), pid: 7, project: Some(project), agent: None, platform: None })).await;
        let tree = Tree {
            revision: 1,
            width: 200.0,
            height: 100.0,
            nodes: vec![
                Node { id: 1, type_name: "Demo.Page".into(), rect: [0.0, 0.0, 200.0, 100.0], visible: true, ..Default::default() },
                Node { id: 2, parent: Some(1), type_name: "DrawnUi.Draw.SkiaLabel".into(), rect: [0.0, 0.0, 50.0, 20.0], visible: true, ..Default::default() },
                Node { id: 3, parent: Some(2), type_name: "DrawnUi.Draw.SkiaLayout".into(), internal: true, templated: true, ..Default::default() },
            ],
        };
        send(&mut ws, &AppMsg::Tree(tree.clone())).await;
        tokio::spawn(async move {
            let mut font = "18".to_string();
            while let Some(Ok(frame)) = ws.next().await {
                let Message::Text(text) = frame else { continue };
                let reply = match serde_json::from_str::<HubToApp>(&text).unwrap() {
                    HubToApp::GetTree => AppMsg::Tree(tree.clone()),
                    HubToApp::GetProps { node } => {
                        let p = |name: &str, t: &str, v: &str| Prop { name: name.into(), type_name: t.into(), value: v.into(), is_set: true, ..Default::default() };
                        AppMsg::Props { node, props: vec![p("FontSize", "System.Double", &font), p("Text", "System.String", "Hi")] }
                    }
                    HubToApp::SetProperty { request, name, .. } if name == "Bogus" => {
                        AppMsg::Error { request: Some(request), message: format!("SkiaLabel has no bindable property {name}") }
                    }
                    HubToApp::SetProperty { value, .. } => {
                        font = value;
                        continue;
                    }
                    _ => continue,
                };
                send(&mut ws, &reply).await;
            }
        });
    }

    async fn start_hub() -> (String, String) {
        let listener = bind(0).await.unwrap();
        let mcp = bind(0).await.unwrap();
        let urls = (format!("ws://{}", listener.local_addr().unwrap()), format!("http://{}/mcp", mcp.local_addr().unwrap()));
        tokio::spawn(serve_test(listener, Some(mcp)));
        urls
    }

    fn tool_json(result: &rmcp::model::CallToolResult) -> serde_json::Value {
        assert_ne!(result.is_error, Some(true), "{result:?}");
        serde_json::from_str(&result.content[0].as_text().expect("text").text).unwrap()
    }

    #[tokio::test]
    async fn mcp_client_lists_apps_reads_the_tree_and_writes_a_property() {
        use rmcp::ServiceExt;
        use rmcp::model::{CallToolRequestParams, ClientConfig};
        use rmcp::transport::{StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig};

        let (ws, mcp) = start_hub().await;
        let dir = std::env::temp_dir().join(format!("designer-mcp-test-{}", std::process::id()));
        fake_app(&ws, &dir).await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let transport = StreamableHttpClientTransport::from_config(StreamableHttpClientTransportConfig::with_uri(mcp));
        let client = ClientConfig::default().serve(transport).await.unwrap();
        let call = async |name: &str, args: serde_json::Value| {
            let mut p = CallToolRequestParams::new(name.to_string());
            if let serde_json::Value::Object(m) = args {
                p = p.with_arguments(m);
            }
            client.call_tool(p).await.unwrap()
        };
        let tools: Vec<String> = client.list_all_tools().await.unwrap().into_iter().map(|t| t.name.to_string()).collect();
        for t in ["list_apps", "list_screens", "open_screen", "get_tree", "get_properties", "select", "pick", "set_property", "write_property", "snapshot", "focus", "run_app", "restart_app"] {
            assert!(tools.iter().any(|x| x == t), "{t} missing from {tools:?}");
        }

        let apps = tool_json(&call("list_apps", json!({})).await);
        assert_eq!(apps[0]["name"], "Demo");
        let app = apps[0]["id"].clone();

        let tree = tool_json(&call("get_tree", json!({"app": app})).await);
        let nodes = tree["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 2, "internal node left out: {nodes:?}");
        assert_eq!(nodes[1]["type"], "SkiaLabel");
        let source = nodes[1]["source"].as_str().unwrap();
        assert!(source.ends_with("Page.cs:8"), "{source}");
        assert!(nodes[0]["source"].as_str().unwrap().ends_with("Page.cs:4"), "{nodes:?}");
        assert!(nodes.iter().all(|n| n.get("templated").is_none()), "templated only where true: {nodes:?}");
        let all = tool_json(&call("get_tree", json!({"app": app, "include_internal": true})).await);
        assert_eq!(all["nodes"].as_array().unwrap().len(), 3);
        assert_eq!(all["nodes"][2]["templated"], true);

        let refused = call("set_property", json!({"app": app, "node": 2, "name": "Bogus", "value": "1"})).await;
        assert_eq!(refused.is_error, Some(true));
        assert!(refused.content[0].as_text().unwrap().text.contains("no bindable property"), "{refused:?}");

        let written = tool_json(&call("write_property", json!({"app": app, "node": 2, "name": "FontSize", "value": "20"})).await);
        assert_eq!(written["live"], "set");
        assert!(written["code"]["edited"]["uri"].as_str().unwrap().ends_with("Page.cs"), "{written}");
        let text = std::fs::read_to_string(dir.join("Page.cs")).unwrap();
        assert!(text.contains("new SkiaLabel { Text = \"Hi\", FontSize = 20 },"), "{text}");
        let props = tool_json(&call("get_properties", json!({"app": app, "node": 2})).await);
        assert_eq!(props[0]["value"], "20");

        let snap = call("snapshot", json!({"app": app})).await;
        assert_eq!(snap.is_error, Some(true), "the fake app has no snapshots");
        let _ = client.cancel().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The raw streamable-HTTP exchange a client such as Claude Code makes, on a hub of its own.
    #[tokio::test]
    async fn raw_http_initialize_list_and_call() {
        let (ws, mcp) = start_hub().await;
        let dir = std::env::temp_dir().join(format!("designer-mcp-raw-{}", std::process::id()));
        fake_app(&ws, &dir).await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let http = reqwest::Client::new();
        let mut session: Option<String> = None;
        let mut post = async |body: serde_json::Value| -> (u16, Option<serde_json::Value>) {
            let mut req = http
                .post(&mcp)
                .header("accept", "application/json, text/event-stream")
                .header("content-type", "application/json")
                .header("mcp-protocol-version", "2025-06-18");
            if let Some(s) = &session {
                req = req.header("mcp-session-id", s);
            }
            let resp = req.body(body.to_string()).send().await.unwrap();
            if let Some(s) = resp.headers().get("mcp-session-id") {
                session = Some(s.to_str().unwrap().to_string());
            }
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap();
            // SSE: the JSON-RPC message is the `data:` line that carries a result or an error.
            let msg = text
                .lines()
                .filter_map(|l| l.strip_prefix("data:"))
                .filter_map(|d| serde_json::from_str::<serde_json::Value>(d.trim()).ok())
                .find(|v| v.get("result").is_some() || v.get("error").is_some())
                .or_else(|| serde_json::from_str(&text).ok());
            (status, msg)
        };
        let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "raw-test", "version": "0"}}});
        let (status, msg) = post(init).await;
        assert_eq!(status, 200);
        let msg = msg.unwrap();
        assert!(msg["result"]["capabilities"]["tools"].is_object(), "{msg}");
        let (status, _) = post(json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await;
        assert_eq!(status, 202);
        let (_, msg) = post(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
        assert_eq!(msg.unwrap()["result"]["tools"].as_array().unwrap().len(), 23);
        let (_, msg) = post(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "list_apps", "arguments": {}}})).await;
        let msg = msg.unwrap();
        let apps: serde_json::Value = serde_json::from_str(msg["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(apps[0]["name"], "Demo", "{msg}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recent_file_round_trips_and_projects_compare_loosely() {
        let dir = std::env::temp_dir().join(format!("designer-recent-{}", std::process::id()));
        let path = dir.join("recent.json");
        let recent = vec![RecentApp { name: "Demo".into(), engine: "dotnet-wpf".into(), project: r"C:\A\Demo".into(), agent: Some(r"C:\agent.dll".into()), last_seen: 42, platform: None }];
        run::save_recent(&path, &recent).unwrap();
        assert_eq!(run::load_recent(&path), recent);
        assert_eq!(run::load_recent(&dir.join("missing.json")), vec![]);
        assert_eq!(run::project_key(r"C:\A\Demo"), run::project_key("C:/A/Demo/"));
        assert_eq!(run::project_key(r"C:\A\Demo"), run::project_key(r"C:\A\Demo\Demo.csproj"));
        assert_eq!(run::project_key(r"C:\A\Demo"), run::project_key(r"C:\A\B\..\.\Demo"));
        assert_eq!(run::project_key("/home/a/../a/demo/"), run::project_key("/home/a/demo"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `cmd /c <script>`: a run that prints like a build and then stays alive.
    struct FakeLauncher(Mutex<String>);

    impl Launcher for FakeLauncher {
        fn command(&self, _app: &RecentApp, hub_url: &str) -> Result<LaunchCommand, Refusal> {
            assert!(hub_url.starts_with("ws://127.0.0.1:"), "{hub_url}");
            Ok(LaunchCommand { program: "cmd".into(), args: vec!["/c".into(), self.0.lock().unwrap().clone()], env: vec![], cwd: std::env::temp_dir() })
        }
    }

    async fn connect_app(url: &str, project: &str, pid: u32) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>> {
        let (mut ws, _) = connect_async(url).await.unwrap();
        let info = AppInfo { protocol: PROTOCOL_VERSION.into(), name: "Demo".into(), engine: "dotnet-wpf".into(), pid, project: Some(project.into()), agent: Some(r"C:\agent.dll".into()), platform: None };
        send(&mut ws, &Hello::App(info)).await;
        ws
    }

    async fn next_run_state(ws: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin)) -> (RunState, String) {
        loop {
            if let HubToEditor::AppRunState { state, message, .. } = next_editor_msg(ws).await {
                return (state, message);
            }
        }
    }

    fn alive(pid: u32) -> bool {
        let out = std::process::Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/NH"]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn recent_apps_run_restart_and_report_build_errors() {
        use rmcp::ServiceExt;
        use rmcp::model::{CallToolRequestParams, ClientConfig};
        use rmcp::transport::{StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig};

        let dir = std::env::temp_dir().join(format!("designer-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let launcher = std::sync::Arc::new(FakeLauncher(Mutex::new("echo Build succeeded& ping -n 120 127.0.0.1 >nul".into())));
        let options = HubOptions { recent_file: Some(dir.join("recent.json")), log_dir: Some(dir.join("logs")), launcher: launcher.clone(), run_timeout: std::time::Duration::from_secs(8), usbmux: None };
        let listener = bind(0).await.unwrap();
        let mcp = bind(0).await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let mcp_url = format!("http://{}/mcp", mcp.local_addr().unwrap());
        let hub = Shared::new(Mutex::new(Hub::with_runs(run::Runs::new(options, url.clone()))));
        tokio::spawn(serve_hub(hub.clone(), listener, Some(mcp)));

        // An app connects once: it is saved as recent, with its agent. Its project folder exists
        // (recent apps whose folder is gone are not offered).
        let project_folder = dir.join("Demo");
        std::fs::create_dir_all(&project_folder).unwrap();
        let project_text = project_folder.to_string_lossy().into_owned();
        let project = project_text.as_str();
        let first = connect_app(&url, project, 4_000_000_001).await;
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let saved = run::load_recent(&dir.join("recent.json"));
        assert_eq!((saved[0].project.as_str(), saved[0].agent.as_deref()), (project, Some(r"C:\agent.dll")));
        drop(first);
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        // A new editor sees it among the recent apps, and runs it.
        let (mut editor, _) = connect_async(&url).await.unwrap();
        send(&mut editor, &Hello::Editor { protocol: PROTOCOL_VERSION.into() }).await;
        let HubToEditor::Apps { apps, recent } = next_editor_msg(&mut editor).await else { panic!("apps first") };
        assert!(apps.is_empty());
        assert_eq!(recent[0].project, project);
        send(&mut editor, &EditorMsg::RunApp { project: project.into() }).await;
        let (state, message) = next_run_state(&mut editor).await;
        assert_eq!(state, RunState::Building, "{message}");
        assert!(message.contains("Demo.log"), "{message}");
        assert_eq!(next_run_state(&mut editor).await.0, RunState::Starting);
        // The started app connects (the project written another way): the run is done.
        let loose = format!("{}/", project.replace('\\', "/").to_lowercase());
        let second = connect_app(&url, &loose, 4_000_000_003).await;
        assert_eq!(next_run_state(&mut editor).await.0, RunState::Running);
        let old_pid = hub.lock().unwrap().runs_pid(project).unwrap();
        assert!(alive(old_pid));

        // restart_app through MCP: the started tree is killed, a new run reports, the new app connects.
        let transport = StreamableHttpClientTransport::from_config(StreamableHttpClientTransportConfig::with_uri(mcp_url));
        let client = ClientConfig::default().serve(transport).await.unwrap();
        let second_id = hub.lock().unwrap().core.apps.iter().find(|(_, a)| a.info.pid == 4_000_000_003).map(|(&id, _)| id).unwrap();
        let args = json!({"app": second_id}).as_object().unwrap().clone();
        let call = tokio::spawn(async move { client.call_tool(CallToolRequestParams::new("restart_app").with_arguments(args)).await.map(|r| (r, client)) });
        assert_eq!(next_run_state(&mut editor).await.0, RunState::Building);
        assert!(!alive(old_pid), "the first run's tree was killed");
        let third = connect_app(&url, project, 4_000_000_005).await;
        let (result, client) = call.await.unwrap().unwrap();
        let answer = tool_json(&result);
        assert_eq!(answer["pid"], 4_000_000_005u32, "{answer}");
        assert_ne!(answer["app"], json!(second_id));

        // Run refuses while the project's app is connected; once it is gone, Run starts it.
        let args = json!({}).as_object().unwrap().clone();
        let result = client.call_tool(CallToolRequestParams::new("run_app").with_arguments(args)).await.unwrap();
        assert!(result.content[0].as_text().unwrap().text.contains("running already"), "{result:?}");
        drop((second, third));
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // A failing build: run_app answers with the compiler errors.
        *launcher.0.lock().unwrap() = "echo Page.cs(3,1): error CS1002: ; expected& echo Build failed& ping -n 120 127.0.0.1 >nul".into();
        let args = json!({}).as_object().unwrap().clone();
        let result = client.call_tool(CallToolRequestParams::new("run_app").with_arguments(args)).await.unwrap();
        assert_eq!(result.is_error, Some(true));
        let text = &result.content[0].as_text().unwrap().text;
        assert!(text.contains("the build failed") && text.contains("error CS1002"), "{text}");

        // A run no app of the project answers: after the timeout its tree is killed and the hub lives on.
        *launcher.0.lock().unwrap() = "echo Build succeeded& ping -n 120 127.0.0.1 >nul".into();
        let args = json!({}).as_object().unwrap().clone();
        let result = client.call_tool(CallToolRequestParams::new("run_app").with_arguments(args)).await.unwrap();
        let text = &result.content[0].as_text().unwrap().text;
        assert!(text.contains("no app connected within 8 s"), "{text}");
        assert!(hub.lock().unwrap().runs_pids().is_empty());
        let apps = tool_json(&client.call_tool(CallToolRequestParams::new("list_apps")).await.unwrap());
        assert_eq!(apps[0]["connected"], false, "the hub still serves: {apps}");

        let pids: Vec<u32> = hub.lock().unwrap().runs_pids();
        for pid in pids {
            run::kill_tree(pid);
        }
        let _ = client.cancel().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real thing on a hub of its own: a copy of DesignerSample under `target\runcheck` (same
    /// depth, so its DrawnUi reference resolves) runs with `dotnet watch`, connects, restarts with a
    /// new pid. Opens a window for a minute or two.
    /// `cargo test -p designer-core real_run -- --ignored --nocapture`
    #[cfg(windows)]
    #[tokio::test]
    #[ignore]
    async fn real_run_and_restart_designer_sample() {
        use rmcp::ServiceExt;
        use rmcp::model::{CallToolRequestParams, ClientConfig};
        use rmcp::transport::{StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig};

        /// Kills every process of the run and removes the copy, also when the test fails.
        struct Cleanup {
            hub: Shared,
            project: String,
            dir: std::path::PathBuf,
        }
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let pids = self.hub.lock().map(|h| h.runs_pids()).unwrap_or_default();
                for pid in pids.into_iter().chain(run::project_processes(&self.project)) {
                    run::kill_tree(pid);
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
                let _ = std::fs::remove_dir_all(&self.dir);
            }
        }

        let root = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join(r"..\..")).unwrap();
        let root = Path::new(root.to_str().unwrap().trim_start_matches(r"\\?\")).to_path_buf();
        let source = root.join(r"samples\wpf\DesignerSample");
        let agent = root.join(r"agents\dotnet\Agent\bin\Release\net8.0-windows\DrawnUi.Designer.Agent.dll");
        // A copy in the temp folder, never the sample itself: its DrawnUi reference made absolute.
        let check = std::env::temp_dir().join(format!("designer-runcheck-{}", std::process::id()));
        let copy = check.join("DesignerSample");
        let _ = std::fs::remove_dir_all(&check);
        copy_sources(&source, &copy);
        let csproj = copy.join("DesignerSample.csproj");
        let drawnui = root.join(r"..\DrawnUi\src\Wpf\Drawnui.Wpf\DrawnUi.Wpf.csproj");
        let text = std::fs::read_to_string(&csproj).unwrap().replace(r"..\..\..\..\DrawnUi\src\Wpf\Drawnui.Wpf\DrawnUi.Wpf.csproj", drawnui.to_str().unwrap());
        assert!(!text.contains(r"..\..\..\..\"), "the copy still points outside itself");
        std::fs::write(&csproj, text).unwrap();
        for extra in ["fonts", "App.xaml", "MainWindow.xaml", "icon.ico"] {
            let from = source.join(extra);
            if from.is_dir() {
                for f in std::fs::read_dir(&from).unwrap().flatten() {
                    std::fs::create_dir_all(copy.join(extra)).unwrap();
                    std::fs::copy(f.path(), copy.join(extra).join(f.file_name())).unwrap();
                }
            } else {
                std::fs::copy(&from, copy.join(extra)).unwrap();
            }
        }
        let project = copy.to_string_lossy().to_string();
        let recent = vec![RecentApp { name: "DesignerSample".into(), engine: "dotnet-wpf".into(), project: project.clone(), agent: Some(agent.to_string_lossy().into()), last_seen: 1, platform: None }];
        run::save_recent(&check.join("recent.json"), &recent).unwrap();
        let options = HubOptions { recent_file: Some(check.join("recent.json")), log_dir: Some(check.join("logs")), launcher: std::sync::Arc::new(DefaultLauncher), run_timeout: run::RUN_TIMEOUT, usbmux: None };
        let listener = bind(0).await.unwrap();
        let mcp = bind(0).await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let mcp_url = format!("http://{}/mcp", mcp.local_addr().unwrap());
        let hub = Shared::new(Mutex::new(Hub::with_runs(run::Runs::new(options, url))));
        let _cleanup = Cleanup { hub: hub.clone(), project: project.clone(), dir: check.clone() };
        tokio::spawn(serve_hub(hub.clone(), listener, Some(mcp)));

        let transport = StreamableHttpClientTransport::from_config(StreamableHttpClientTransportConfig::with_uri(mcp_url));
        let client = ClientConfig::default().serve(transport).await.unwrap();
        let started = std::time::Instant::now();
        let result = client.call_tool(CallToolRequestParams::new("run_app").with_arguments(json!({}).as_object().unwrap().clone())).await.unwrap();
        let first = tool_json(&result);
        println!("run_app after {:.1} s: {first}", started.elapsed().as_secs_f32());
        let started = std::time::Instant::now();
        let args = json!({"app": first["app"]}).as_object().unwrap().clone();
        let result = client.call_tool(CallToolRequestParams::new("restart_app").with_arguments(args)).await.unwrap();
        let second = tool_json(&result);
        println!("restart_app after {:.1} s: {second}", started.elapsed().as_secs_f32());
        assert_ne!(first["pid"], second["pid"]);
        assert!(!alive(first["pid"].as_u64().unwrap() as u32), "the first app is gone");

        // Restart left exactly one tree running the copy: the hub's own.
        let running = run::project_processes(&project);
        assert_eq!(running, hub.lock().unwrap().runs_pids(), "one dotnet watch for the copy");
        println!("log: {}", std::fs::read_to_string(check.join(r"logs\DesignerSample.log")).unwrap_or_default().lines().take(12).collect::<Vec<_>>().join("\n"));
        let _ = client.cancel().await;
        drop(_cleanup);
        assert!(!alive(second["pid"].as_u64().unwrap() as u32) && running.iter().all(|p| !alive(*p)), "everything stopped");
        assert!(run::project_processes(&project).is_empty());
    }

    /// After an app's Props for a control, the hub tells editors what the code sets on it.
    #[tokio::test]
    async fn props_are_followed_by_what_the_code_sets() {
        let (ws, _mcp) = start_hub().await;
        let dir = std::env::temp_dir().join(format!("designer-codeset-{}", std::process::id()));
        fake_app(&ws, &dir).await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let (mut editor, _) = connect_async(&ws).await.unwrap();
        send(&mut editor, &Hello::Editor { protocol: PROTOCOL_VERSION.into() }).await;
        let HubToEditor::Apps { apps, .. } = next_editor_msg(&mut editor).await else { panic!("apps first") };
        let app = apps[0].id;
        let code_set = async |editor: &mut _, node: NodeId| {
            send(editor, &EditorMsg::ToApp { app, msg: HubToApp::GetProps { node } }).await;
            loop {
                if let HubToEditor::CodeSet { node: n, names, .. } = next_editor_msg(editor).await
                    && n == node
                {
                    break names;
                }
            }
        };
        assert_eq!(code_set(&mut editor, 2).await, Some(vec!["Text".to_string(), "FontSize".to_string()]));
        // A part the library builds: no code.
        assert_eq!(code_set(&mut editor, 3).await, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// CodeSet on the MAUI Tutorials project with the recorded firstappc tree: the first answer
    /// parses the project and matches the tree, later ones are lookups.
    #[test]
    fn code_set_timing_on_tutorials() {
        // A copy of the DrawnUi Tutorials sample, named by DESIGNER_TUTORIALS_DIR.
        let project = std::env::var("DESIGNER_TUTORIALS_DIR").unwrap_or_default();
        let project = project.as_str();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join(r"..\..\fixtures\maui-android\firstappc.json");
        let (true, Ok(json)) = (Path::new(project).exists(), std::fs::read_to_string(fixture)) else {
            eprintln!("skipped: the Tutorials project or the fixture is missing");
            return;
        };
        let tree: Tree = serde_json::from_str(&json).unwrap();
        let mut cache = designer_hubcore::CodeCache::default();
        let t = std::time::Instant::now();
        let first = cache.names(&FsStore, 1, project, &tree, 32);
        let cold = t.elapsed();
        let t = std::time::Instant::now();
        let second = cache.names(&FsStore, 1, project, &tree, 34);
        let warm = t.elapsed();
        let t = std::time::Instant::now();
        let _ = cache.names(&FsStore, 1, project, &tree, 31);
        let warm2 = t.elapsed();
        println!("CodeSet on Tutorials: first {:.1} ms, then {:.1} ms and {:.1} ms", cold.as_secs_f64() * 1e3, warm.as_secs_f64() * 1e3, warm2.as_secs_f64() * 1e3);
        assert!(first.unwrap().contains(&"FontSize".to_string()));
        assert!(second.unwrap().contains(&"CornerRadius".to_string()));
    }
}
