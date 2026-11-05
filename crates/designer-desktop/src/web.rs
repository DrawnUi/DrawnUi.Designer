//! The web build's link to the hub, in one of two modes the page chooses (web/designer.js):
//!
//! - **Socket** (the VS Code webview): the page script owns a WebSocket to a hub. It calls
//!   `designer_connected` when the socket opens or closes and `designer_receive` with every frame;
//!   messages to the hub leave through `designer_send`.
//! - **Page** (DrawFiddle, `designerHub: "page"`): the hub runs here, in this module ([`HubCore`]).
//!   The app and the code are in the parent page: `designer_page_start` starts the hub,
//!   `designer_page` takes each message of the parent, and messages to it leave through
//!   `designer_post`. The editor's own messages are handled in `designer_pump`, which the page runs
//!   right after the UI's turn (`designer_defer`), so the hub never runs inside the UI's update.
//!
//! The page functions come in with --js-library (web/designer_link.js). Nothing polls: each
//! delivery wakes the inbox, and the page requests a frame after every call.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use designer_hubcore::page::{self, PageCode, ToPage};
use designer_hubcore::{HubCore, Out};
use designer_protocol::{AppId, AppMsg, EditorMsg, Hello, HubToEditor, PROTOCOL_VERSION};
use serde_json::{Value, json};

use crate::ui::Incoming;

unsafe extern "C" {
    fn designer_send(text: *const u8, len: usize);
    fn designer_log(text: *const u8, len: usize);
    fn designer_post(text: *const u8, len: usize);
    fn designer_defer();
    fn designer_embedded() -> i32;
}

/// Whether the page hosts the editor inside itself (DrawFiddle): only Layers and Properties show.
pub fn embedded() -> bool {
    unsafe { designer_embedded() != 0 }
}

/// Every message to and from the parent page carries it.
const CHANNEL: &str = "drawnui-designer";

/// The hub of page mode.
struct PageHub {
    core: HubCore,
    code: Arc<PageCode>,
    editor: u32,
    app: Option<AppId>,
    /// The editor's messages, handled in `designer_pump`.
    queue: Vec<EditorMsg>,
}

thread_local! {
    /// Where deliveries go, and what wakes the UI after one.
    static LINK: RefCell<Option<(Sender<Incoming>, Box<dyn Fn()>)>> = const { RefCell::new(None) };
    static PAGE: RefCell<Option<PageHub>> = const { RefCell::new(None) };
}

pub fn start(to_ui: Sender<Incoming>, wake: impl Fn() + 'static) {
    LINK.with(|link| *link.borrow_mut() = Some((to_ui, Box::new(wake))));
}

pub fn send(msg: EditorMsg) {
    let unqueued = PAGE.with(|p| match p.borrow_mut().as_mut() {
        Some(p) => {
            p.queue.push(msg);
            None
        }
        None => Some(msg),
    });
    match unqueued {
        None => unsafe { designer_defer() },
        Some(msg) => send_text(&serde_json::to_string(&msg).expect("serializable")),
    }
}

pub fn log(text: &str) {
    unsafe { designer_log(text.as_ptr(), text.len()) }
}

fn send_text(text: &str) {
    unsafe { designer_send(text.as_ptr(), text.len()) }
}

fn post(msg: Value) {
    let text = msg.to_string();
    unsafe { designer_post(text.as_ptr(), text.len()) }
}

fn deliver(incoming: Incoming) {
    LINK.with(|link| {
        if let Some((to_ui, wake)) = &*link.borrow() {
            let _ = to_ui.send(incoming);
            wake();
        }
    });
}

/// The page's socket opened (1) or closed (0). On open the editor introduces itself to the hub.
#[unsafe(no_mangle)]
pub extern "C" fn designer_connected(open: i32) {
    if open != 0 {
        send_text(&serde_json::to_string(&Hello::Editor { protocol: PROTOCOL_VERSION.into() }).expect("serializable"));
    }
    deliver(Incoming::Connected(open != 0));
}

/// One text frame from the hub: UTF-8 JSON in a `dui_alloc` buffer the page frees after the call.
#[unsafe(no_mangle)]
pub extern "C" fn designer_receive(text: *const u8, len: usize) {
    let bytes = unsafe { std::slice::from_raw_parts(text, len) };
    match serde_json::from_slice::<HubToEditor>(bytes) {
        Ok(msg) => deliver(Incoming::Hub(msg)),
        Err(error) => log(&format!("unreadable frame from the hub: {error}")),
    }
}

/// Page mode: the hub starts here and the editor connects to it. (The page script asked for the
/// code already, before the wasm loaded: the page holds its messages until the iframe's first one.)
#[unsafe(no_mangle)]
pub extern "C" fn designer_page_start() {
    let code = Arc::new(PageCode::new(|m| {
        post(match m {
            ToPage::Edit { version, edits } => {
                let edits: Vec<Value> = edits.into_iter().map(|e| json!({ "start": e.start, "end": e.end, "text": e.text })).collect();
                json!({ "channel": CHANNEL, "type": "code.edit", "version": version, "edits": edits })
            }
            ToPage::Get => json!({ "channel": CHANNEL, "type": "code.get" }),
        })
    }));
    let mut core = HubCore::new(code.clone());
    core.page_project = Some(page::PROJECT.into());
    let (editor, outs) = core.editor_hello(PROTOCOL_VERSION).expect("same protocol");
    PAGE.with(|p| *p.borrow_mut() = Some(PageHub { core, code, editor, app: None, queue: vec![] }));
    deliver(Incoming::Connected(true));
    dispatch(outs);
}

/// Page mode: one message of the parent page (its `channel` checked by the page script), UTF-8 JSON
/// in a `dui_alloc` buffer the page frees after the call.
#[unsafe(no_mangle)]
pub extern "C" fn designer_page(text: *const u8, len: usize) {
    let bytes = unsafe { std::slice::from_raw_parts(text, len) };
    let Ok(m) = serde_json::from_slice::<Value>(bytes) else { return log("unreadable message from the page") };
    // The page's look: "dark", "light" or a designer theme id (assets/themes.json).
    if m["type"] == "theme" {
        if let Some(theme) = m["theme"].as_str() {
            deliver(Incoming::Theme(theme.to_owned()));
        }
        return;
    }
    let outs = PAGE.with(|p| {
        let mut p = p.borrow_mut();
        let Some(p) = p.as_mut() else { return vec![] };
        match m["type"].as_str() {
            Some("fromApp") if m["msg"].get("role").is_some() => match serde_json::from_value::<Hello>(m["msg"].clone()) {
                Ok(Hello::App(info)) => {
                    // A new hello replaces the app (the page attached its agent again).
                    let mut outs = p.app.take().map(|old| p.core.app_closed(old)).unwrap_or_default();
                    match p.core.app_hello(info) {
                        Some((id, more)) => {
                            p.app = Some(id);
                            outs.extend(more);
                        }
                        None => log("the app speaks another protocol version"),
                    }
                    p.core.run_inline(outs)
                }
                _ => vec![],
            },
            Some("fromApp") => match (p.app, serde_json::from_value::<AppMsg>(m["msg"].clone())) {
                (Some(app), Ok(msg)) => {
                    let outs = p.core.app_msg(app, msg);
                    p.core.run_inline(outs)
                }
                (_, Err(error)) => {
                    log(&format!("unreadable message from the app: {error}"));
                    vec![]
                }
                (None, _) => vec![],
            },
            Some("app.closed") => match p.app.take() {
                Some(app) => {
                    let outs = p.core.app_closed(app);
                    p.core.run_inline(outs)
                }
                None => vec![],
            },
            // The snippet in the page's language: fiddle:///snippet.cs (C#) or .tsx (React).
            Some("code") if m["uri"].as_str().is_none_or(page::is_snippet) => {
                if let (Some(text), Some(version)) = (m["text"].as_str(), m["version"].as_u64()) {
                    p.code.set_text_for(m["uri"].as_str().unwrap_or(page::SNIPPET), text.to_owned(), version);
                }
                vec![]
            }
            _ => vec![],
        }
    });
    dispatch(outs);
}

/// Page mode: the editor's messages queued since the last call.
#[unsafe(no_mangle)]
pub extern "C" fn designer_pump() {
    let outs = PAGE.with(|p| {
        let mut p = p.borrow_mut();
        let Some(p) = p.as_mut() else { return vec![] };
        let mut sent = vec![];
        for msg in std::mem::take(&mut p.queue) {
            let outs = p.core.editor_msg(p.editor, msg);
            sent.extend(p.core.run_inline(outs));
        }
        sent
    });
    dispatch(outs);
}

/// What the hub sends: the editor's into the UI, the app's to the parent page. (Outside the
/// hub's borrow: a delivery may wake the UI.)
fn dispatch(outs: Vec<Out>) {
    for out in outs {
        match out {
            Out::ToEditor(_, msg) => deliver(Incoming::Hub(msg)),
            Out::ToApp(_, msg) => post(json!({ "channel": CHANNEL, "type": "toApp", "msg": msg })),
            Out::Work(_) => {} // run_inline ran them all
        }
    }
}
