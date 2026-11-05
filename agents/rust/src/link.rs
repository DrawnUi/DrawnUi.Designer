//! The connection to the hub: one background thread, blocking sockets. While no hub answers it
//! tries again every 2 s and does nothing else; the UI thread never waits on it.

use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use designer_protocol::{AppInfo, AppMsg, Hello, HubToApp};
use drawnui::Waker;
use tungstenite::Message;

use crate::agent::Out;

/// The UI thread's end of the link: requests in, answers out.
pub struct Wire {
    pub inbox: Receiver<HubToApp>,
    /// Set once a request is queued: the one check a frame makes while nothing is asked.
    pub queued: Arc<AtomicBool>,
    pub out: Sender<Out>,
}

impl Wire {
    /// Starts the link thread for the hub at `url`.
    pub fn start(waker: Waker, url: String, hello: AppInfo) -> Wire {
        let (inbox_tx, inbox) = mpsc::channel();
        let (out, out_rx) = mpsc::channel();
        let queued = Arc::new(AtomicBool::new(false));
        let to_ui = ToUi { inbox: inbox_tx, waker, queued: queued.clone() };
        let started = std::thread::Builder::new().name("drawnui-designer-agent".into()).spawn(move || run(url, hello, to_ui, out_rx));
        if let Err(e) = started {
            eprintln!("DrawnUI Designer agent: not started: {e}");
        }
        Wire { inbox, queued, out }
    }

    /// Requests are waiting.
    pub fn ready(&self) -> bool {
        if !self.queued.load(Ordering::Acquire) {
            return false;
        }
        self.queued.store(false, Ordering::Relaxed);
        true
    }

    pub fn next(&self) -> Option<HubToApp> {
        self.inbox.try_recv().ok()
    }

    pub fn send(&self, out: Out) {
        let _ = self.out.send(out);
    }
}

/// Where requests go: queued for the UI thread, which a frame then drains.
struct ToUi {
    inbox: Sender<HubToApp>,
    waker: Waker,
    queued: Arc<AtomicBool>,
}

impl ToUi {
    fn send(&self, msg: HubToApp) {
        if self.inbox.send(msg).is_ok() {
            self.queued.store(true, Ordering::Release);
            // A frame now; while the window is minimized or covered the host draws it off screen,
            // so the request is answered then too.
            self.waker.request_hidden_frame();
        }
    }
}

fn run(url: String, hello: AppInfo, to_ui: ToUi, out: Receiver<Out>) {
    println!("DrawnUI Designer agent: waiting for a designer at {url}");
    let hello = serde_json::to_string(&Hello::App(hello)).unwrap_or_default();
    loop {
        if let Some(stream) = connect(&url) {
            // Answers left from an earlier session are for nobody.
            while out.try_recv().is_ok() {}
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session(stream, &url, &hello, &to_ui, &out)));
            // The designer is gone: nothing of it stays drawn in the app.
            to_ui.send(HubToApp::PickMode { on: false });
            to_ui.send(HubToApp::Highlight { node: None, color: None });
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// A TCP connection to the hub's `host:port`, or None while nothing listens there.
fn connect(url: &str) -> Option<TcpStream> {
    let authority = url.split_once("://").map_or(url, |(_, rest)| rest).split('/').next()?;
    let addr = authority.to_socket_addrs().ok()?.next()?;
    TcpStream::connect_timeout(&addr, Duration::from_secs(1)).ok()
}

fn session(stream: TcpStream, url: &str, hello: &str, to_ui: &ToUi, out: &Receiver<Out>) {
    let Ok((mut ws, _)) = tungstenite::client(url, stream) else { return };
    // Reads wake every 20 ms to send what the UI thread answered (only while a designer is connected).
    let _ = ws.get_mut().set_read_timeout(Some(Duration::from_millis(20)));
    let _ = ws.get_mut().set_nodelay(true);
    if ws.send(Message::text(hello)).is_err() {
        return;
    }
    to_ui.send(HubToApp::GetTree);
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => match serde_json::from_str::<HubToApp>(&text) {
                Ok(msg) => to_ui.send(msg),
                Err(e) => {
                    let error = AppMsg::Error { request: None, message: format!("unknown message {text}: {e}") };
                    let _ = ws.send(Message::text(serde_json::to_string(&error).unwrap_or_default()));
                }
            },
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => return,
        }
        while let Ok(msg) = out.try_recv() {
            // PNG encoding here, off the UI thread.
            let msg = msg.into_msg();
            if ws.write(Message::text(serde_json::to_string(&msg).unwrap_or_default())).is_err() {
                return;
            }
        }
        match ws.flush() {
            Ok(()) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => return,
        }
    }
}
