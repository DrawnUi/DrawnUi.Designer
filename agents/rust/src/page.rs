//! The browser transport (wasm32-unknown-emscripten): no socket and no thread, the page carries the
//! messages between the app and the designer, the way the .NET agent's `Attach.AttachPage` does.
//!
//! Page side (JavaScript, `module` = the emscripten module, `requestFrame` from `DrawnUi.start`):
//!
//! ```js
//! const sink = module.addFunction((ptr, len) => send(module.UTF8ToString(ptr, len)), "vii");
//! attach(module, sink, "My app");             // dui_designer_attach: hello + tree go out at the next frame
//! receive(module, '{"type":"getTree"}');      // dui_designer_receive, then requestFrame()
//! ```
//!
//! The app's messages (`AppMsg`, the hello first) reach `sink` as UTF-8 JSON during frames; the
//! designer's (`HubToApp`, plus `{"type":"rebuilt"}`) go in with `dui_designer_receive` and are
//! handled at the next frame, so the page asks for one after each.
//!
//! Hot reload (`dui_hot_swap`): the link's state is kept by the functions below, which are not
//! generic, so they live in the module that links this crate's code: the engine's main module when
//! it links this crate. A swapped-in app wraps itself again (`in_page`) and finds the page still
//! attached: it sends its tree at its first frame, with no call from the page.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

use designer_protocol::{AppInfo, AppMsg, Hello, HubToApp, PROTOCOL_VERSION};

use crate::agent::Out;

/// A JS function the page put in the wasm table (`addFunction(f, "vii")`): UTF-8 bytes, length.
type Sink = extern "C" fn(*const u8, usize);

thread_local! {
    static SINK: Cell<Option<Sink>> = const { Cell::new(None) };
    /// Rises with every attach; 0 while no page is attached.
    static SESSION: Cell<u32> = const { Cell::new(0) };
    static LAST: Cell<u32> = const { Cell::new(0) };
    /// The hello of this session went out.
    static GREETED: Cell<bool> = const { Cell::new(false) };
    static NAME: RefCell<String> = const { RefCell::new(String::new()) };
    static INBOX: RefCell<VecDeque<String>> = const { RefCell::new(VecDeque::new()) };
    /// The next designer id of a control: never given twice, whatever app module asks.
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
}

/// Attaches the page (`sink`: a table index from `addFunction(f, "vii")`, 0 detaches) with the app
/// name the designer shows (UTF-8; empty: "app"). The hello and the tree go out at the next frame.
/// Attaching again starts a new session (a new hello). Messages from the previous one are dropped.
#[unsafe(no_mangle)]
pub extern "C" fn dui_designer_attach(sink: usize, name: *const u8, name_len: usize) {
    let name = if name.is_null() || name_len == 0 { "" } else { std::str::from_utf8(unsafe { std::slice::from_raw_parts(name, name_len) }).unwrap_or("") };
    NAME.with_borrow_mut(|n| name.clone_into(n));
    // On wasm a function pointer is its index in the table.
    SINK.set((sink != 0).then(|| unsafe { std::mem::transmute::<usize, Sink>(sink) }));
    INBOX.with_borrow_mut(VecDeque::clear);
    GREETED.set(false);
    SESSION.set(if sink == 0 {
        0
    } else {
        LAST.set(LAST.get().wrapping_add(1).max(1));
        LAST.get()
    });
}

/// One message from the designer (UTF-8 JSON, the page's buffer: copied here), handled at the next
/// frame. Ignored while no page is attached.
#[unsafe(no_mangle)]
pub extern "C" fn dui_designer_receive(text: *const u8, len: usize) {
    if SESSION.get() == 0 || text.is_null() {
        return;
    }
    let text = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(text, len) }).into_owned();
    INBOX.with_borrow_mut(|q| q.push_back(text));
}

// The agent's calls, one symbol each (never inlined into a hot-swapped app module).

/// The attached session, 0 for none.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn drawnui_designer_session() -> u32 {
    SESSION.get()
}

/// Messages are waiting.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn drawnui_designer_waiting() -> bool {
    INBOX.with_borrow(|q| !q.is_empty())
}

/// The next message of the designer.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn drawnui_designer_next() -> Option<String> {
    INBOX.with_borrow_mut(VecDeque::pop_front)
}

/// The app name for the hello, when it is still due in this session (then never again).
#[unsafe(no_mangle)]
#[inline(never)]
pub fn drawnui_designer_greet() -> Option<String> {
    if SESSION.get() == 0 || GREETED.replace(true) {
        return None;
    }
    Some(NAME.with_borrow(|n| if n.is_empty() { "app".to_owned() } else { n.clone() }))
}

/// A designer id no control got before: the counter outlives every swapped-in app module.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn drawnui_designer_next_id() -> u64 {
    NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}

/// Hands one message to the page.
#[unsafe(no_mangle)]
#[inline(never)]
pub fn drawnui_designer_post(text: &str) {
    if let Some(sink) = SINK.get() {
        sink(text.as_ptr(), text.len());
    }
}

/// Keeps the page link in a module whose own code never calls it: a hot reload shell (the engine
/// as the main module) calls this in its `main`, so the state above lives there and every
/// swapped-in app module uses the same one.
pub fn link_in() {
    std::hint::black_box([
        dui_designer_attach as *const (),
        dui_designer_receive as *const (),
        drawnui_designer_session as *const (),
        drawnui_designer_waiting as *const (),
        drawnui_designer_next as *const (),
        drawnui_designer_greet as *const (),
        drawnui_designer_post as *const (),
        drawnui_designer_next_id as *const (),
    ]);
}

/// The agent's end of the page link.
#[derive(Default)]
pub(crate) struct Wire {
    /// The session this agent has introduced itself to (its tree); 0 for none.
    session: Cell<u32>,
    /// The tree is due (a page attached, or this app was swapped in under an attached page).
    intro: Cell<bool>,
    /// The page left: the pick mode and the highlight go.
    left: Cell<u8>,
}

impl Wire {
    /// Something is to be done this frame. While no page is attached: one call, two numbers compared.
    #[inline]
    pub fn ready(&self) -> bool {
        let session = drawnui_designer_session();
        if session != self.session.get() {
            if self.session.get() != 0 {
                self.left.set(2);
            }
            self.session.set(session);
            self.intro.set(session != 0);
        }
        self.left.get() > 0 || self.intro.get() || (session != 0 && drawnui_designer_waiting())
    }

    #[inline]
    pub fn next(&self) -> Option<HubToApp> {
        match self.left.get() {
            2 => {
                self.left.set(1);
                return Some(HubToApp::PickMode { on: false });
            }
            1 => {
                self.left.set(0);
                return Some(HubToApp::Highlight { node: None, color: None });
            }
            _ => {}
        }
        if self.intro.take() {
            let greet = drawnui_designer_greet();
            // Swapped in under an attached page: the designer's tree is the old app's, its ids
            // are gone (or another control's now) until the new tree arrives.
            if greet.is_none() {
                self.send(Out::Msg(AppMsg::HotReload { stage: "reloaded".into() }));
            }
            if let Some(name) = greet {
                let hello = AppInfo {
                    protocol: PROTOCOL_VERSION.into(),
                    name,
                    engine: "rust".into(),
                    pid: 0,
                    project: None,
                    agent: Some(concat!("drawnui-designer-agent ", env!("CARGO_PKG_VERSION")).into()),
                    platform: Some("browser".into()),
                };
                drawnui_designer_post(&serde_json::to_string(&Hello::App(hello)).unwrap_or_default());
            }
            return Some(HubToApp::GetTree);
        }
        while let Some(text) = drawnui_designer_next() {
            match serde_json::from_str::<HubToApp>(&text) {
                Ok(msg) => return Some(msg),
                // The page's own word: the app rebuilt its controls in place; a new tree goes out.
                Err(_) if serde_json::from_str::<serde_json::Value>(&text).is_ok_and(|v| v["type"] == "rebuilt") => {
                    // The controls were built again: the designer's ids are old until the new tree.
                    self.send(Out::Msg(AppMsg::HotReload { stage: "reloaded".into() }));
                    return Some(HubToApp::GetTree);
                }
                Err(e) => self.send(Out::Msg(AppMsg::Error { request: None, message: format!("unknown message {text}: {e}") })),
            }
        }
        None
    }

    /// Sends at once (a snapshot is encoded here: there is no other thread).
    #[inline]
    pub fn send(&self, out: Out) {
        if self.session.get() != 0 {
            drawnui_designer_post(&serde_json::to_string(&out.into_msg()).unwrap_or_default());
        }
    }
}
