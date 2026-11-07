//! The UI-thread side: the app wrapped as an `App` that forwards everything to it. The wrapper is
//! there for Pick mode: a click that picks must never reach the app, and `Ui` has no hook that
//! keeps input from it (`Ui::on_after_frame` covers only the frame). Requests wait in the wire
//! (desktop: a channel from the link thread; browser: the page's queue) and are handled in the next
//! frame; while nothing is asked a frame costs one check, input one `bool` check.

use designer_protocol::{AppMsg, HubToApp, Node, Prop, Tree};
use drawnui::controls::button::SkiaButton;
use drawnui::controls::label::SkiaLabel;
use drawnui::skia::{self, Point, Rect};
use drawnui::{App, ControlId, Dirty, Frame, Host, KeyKind, MouseButton, PointerKind, PopupId, Ui, Waker};

use crate::controls;
#[cfg(not(target_os = "emscripten"))]
use crate::link::Wire;
#[cfg(target_os = "emscripten")]
use crate::page::Wire;

/// What the agent hands its wire to send.
pub enum Out {
    Msg(AppMsg),
    /// Canvas pixels (RGBA, unpremultiplied), PNG-encoded where it is sent (desktop: off the UI thread).
    Pixels { request: u32, width: u32, height: u32, rgba: Vec<u8> },
}

impl Out {
    pub fn into_msg(self) -> AppMsg {
        match self {
            Out::Msg(msg) => msg,
            Out::Pixels { request, width, height, rgba } => match png(width, height, rgba) {
                Some(png_base64) => AppMsg::Snapshot { request, png_base64, width, height },
                None => AppMsg::Error { request: Some(request), message: "the snapshot could not be encoded".into() },
            },
        }
    }
}

fn png(width: u32, height: u32, rgba: Vec<u8>) -> Option<String> {
    use drawnui::skia::{AlphaType, ColorType, Data, EncodedImageFormat, ImageInfo, images};
    let info = ImageInfo::new((width as i32, height as i32), ColorType::RGBA8888, AlphaType::Unpremul, None);
    let image = images::raster_from_data(&info, Data::new_copy(&rgba), width as usize * 4)?;
    let png = image.encode(None, EncodedImageFormat::PNG, None)?;
    Some(base64(png.as_bytes()))
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            s.push(if i <= chunk.len() { ABC[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    s
}

/// What the agent needs of the app's `Ui`, without its state type. The agent's work is in [`Core`],
/// compiled once in this crate; per app type there are only these few lines and the [`Agent`]
/// shim. In the browser's hot reload only the app's own module is built for each edit, and the
/// core stays in the engine's module, where every swapped-in app finds it.
pub(crate) trait AppUi: App {
    fn tree(&self) -> &drawnui::Tree;
    fn tree_mut(&mut self) -> &mut drawnui::Tree;
    fn hit_test_visual(&self, at: Point) -> Option<ControlId>;
    /// The app state's type, the host of the main tree's root.
    fn state_type(&self) -> &'static str;
    /// The app's designer actions (`Ui::designer_action`), in declaration order.
    fn designer_actions(&self) -> Vec<String>;
    /// Changes on every add, replace or remove of a designer action.
    fn designer_actions_revision(&self) -> u32;
    /// Runs a designer action with the app state; false: no such action.
    fn run_designer_action(&mut self, path: &str) -> bool;
}

impl<S: 'static> AppUi for Ui<S> {
    fn tree(&self) -> &drawnui::Tree {
        &self.tree
    }
    fn tree_mut(&mut self) -> &mut drawnui::Tree {
        &mut self.tree
    }
    fn hit_test_visual(&self, at: Point) -> Option<ControlId> {
        Ui::hit_test_visual(self, at)
    }
    fn state_type(&self) -> &'static str {
        std::any::type_name::<S>()
    }
    fn designer_actions(&self) -> Vec<String> {
        Ui::designer_actions(self).map(str::to_owned).collect()
    }
    fn designer_actions_revision(&self) -> u32 {
        Ui::designer_actions_revision(self)
    }
    fn run_designer_action(&mut self, path: &str) -> bool {
        Ui::run_designer_action(self, path)
    }
}

/// The app with the agent around it: forwards to the app, and to [`Core`] for the designer's work.
pub struct Agent<S: 'static> {
    pub(crate) ui: Ui<S>,
    core: Core,
}

/// The agent's state and work, the same for every app.
pub struct Core {
    wire: Wire,
    waker: Waker,
    revision: u64,
    highlight: Option<ControlId>,
    /// The number (`PopupId::get`) of the popup window that holds `highlight`.
    highlight_popup: Option<u32>,
    /// The box's color, from the last Highlight message (magenta without one).
    box_color: skia::Color,
    /// The box last painted into a popup window: that window's number, the control, its color.
    boxed: Option<(u32, ControlId, skia::Color)>,
    picking: bool,
    /// A tree was asked for before the app had built and laid out its controls (fonts still on
    /// their way after a hot swap): it goes out at the first frame that has them.
    tree_due: bool,
    /// Place mode: the type of the control a click inserts (pick mode's pointer, a container's box).
    placing: Option<String>,
    /// Place mode: the line drawn where the control would go in the boxed stack, row or wrap.
    place_line: Option<Rect>,
    /// Frames the tree has waited; past `TREE_WAIT` it goes out as it is.
    tree_waited: u32,
    /// The host draws the next frame off screen (`take_hidden_frame_request` said yes): the window is
    /// covered or minimized.
    hidden: bool,
    /// The designer's ids of the controls ([`Core::wire_id`]).
    ids: std::cell::RefCell<WireIds>,
    /// The designer-actions revision last sent (`None`: not sent yet).
    actions_sent: Option<u32>,
}

/// The designer's id of each control told to it, and back. A wire id is never given twice, also
/// across a hot swap or another app instance (`next_wire_id`): a ControlId of a new arena may be the
/// same bits as one of the old app (the same index and generation) for another control, its wire
/// id never is. An id this agent did not give, or whose control is gone, is refused.
#[derive(Default)]
struct WireIds {
    of: std::collections::HashMap<ControlId, u64>,
    back: std::collections::HashMap<u64, ControlId>,
}

/// A wire id no control of this process got before. In the browser the counter lives in the page
/// link's module, which a hot swap keeps (`page.rs`); on the desktop it starts at the clock's
/// milliseconds, so an app module swapped in later starts past the ids handed out before it.
#[cfg(target_os = "emscripten")]
fn next_wire_id() -> u64 {
    crate::page::drawnui_designer_next_id()
}

#[cfg(not(target_os = "emscripten"))]
fn next_wire_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    // ponytail: a swapped-in module allocating faster than one id per millisecond since the last
    // start could meet old ids; a counter in the host module would close that (as in page.rs).
    if NEXT.load(Ordering::Relaxed) == 0 {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_millis() as u64);
        let _ = NEXT.compare_exchange(0, now, Ordering::Relaxed, Ordering::Relaxed);
    }
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Frames a tree waits for the app's controls (about a second): an app may have none.
const TREE_WAIT: u32 = 60;

/// The highlight box's color when the designer names none, or a bad one.
const BOX_COLOR: skia::Color = skia::Color::from_rgb(255, 0, 255);

/// The designer's box color, `#RRGGBB` or `#AARRGGBB`, read once per Highlight message.
fn box_color(text: Option<&str>) -> skia::Color {
    use drawnui::designer::PropText;
    text.and_then(skia::Color::from_text).unwrap_or(BOX_COLOR)
}

impl<S: 'static> Agent<S> {
    /// Desktop: the link thread to the hub.
    #[cfg(not(target_os = "emscripten"))]
    pub fn start(ui: Ui<S>, project_dir: &str) -> Self {
        let core = Core::start(ui.waker(), project_dir);
        Agent { ui, core }
    }

    /// Browser: the page carries the messages (`page.rs`).
    #[cfg(target_os = "emscripten")]
    pub fn in_page(ui: Ui<S>) -> Self {
        let core = Core::new(Wire::default(), ui.waker());
        Agent { ui, core }
    }

    #[cfg(test)]
    fn with(ui: Ui<S>, wire: Wire, waker: Waker) -> Self {
        Agent { ui, core: Core::new(wire, waker) }
    }
}

/// The tests read the core's state through the agent.
#[cfg(test)]
impl<S: 'static> std::ops::Deref for Agent<S> {
    type Target = Core;
    fn deref(&self) -> &Core {
        &self.core
    }
}

impl Core {
    /// Desktop: the link thread to the hub.
    #[cfg(not(target_os = "emscripten"))]
    fn start(waker: Waker, project_dir: &str) -> Self {
        use designer_protocol::{AppInfo, PROTOCOL_VERSION};
        let url = std::env::var("DRAWNUI_DESIGNER_URL").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| format!("ws://127.0.0.1:{}", designer_protocol::DEFAULT_PORT));
        let name = std::env::current_exe().ok().and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or_else(|| "app".into());
        let hello = AppInfo {
            protocol: PROTOCOL_VERSION.into(),
            name,
            engine: "rust".into(),
            pid: std::process::id(),
            project: Some(std::path::Path::new(project_dir).join("Cargo.toml").to_string_lossy().into_owned()),
            agent: Some(concat!("drawnui-designer-agent ", env!("CARGO_PKG_VERSION")).into()),
            platform: Some(std::env::consts::OS.into()),
        };
        let wire = Wire::start(waker.clone(), url, hello);
        Self::new(wire, waker)
    }

    fn new(wire: Wire, waker: Waker) -> Self {
        Core { wire, waker, revision: 0, highlight: None, highlight_popup: None, box_color: BOX_COLOR, boxed: None, picking: false, tree_due: false, tree_waited: 0, hidden: false, placing: None, place_line: None, ids: Default::default(), actions_sent: None }
    }

    /// The designer's id of control `id`: given once, kept while the control lives.
    pub(crate) fn wire_id(&self, id: ControlId) -> u64 {
        let mut ids = self.ids.borrow_mut();
        if let Some(&n) = ids.of.get(&id) {
            return n;
        }
        let n = next_wire_id();
        ids.of.insert(id, n);
        ids.back.insert(n, id);
        n
    }

    /// The control behind a designer id (tests).
    #[cfg(test)]
    pub(crate) fn control(&self, id: u64) -> ControlId {
        self.ids.borrow().back[&id]
    }

    /// The control the designer's id `id` names, when this agent gave it and the control lives;
    /// else why not (a control of the app from before a hot swap or a rebuild).
    fn current(&self, ui: &dyn AppUi, id: u64) -> Result<ControlId, String> {
        match self.ids.borrow().back.get(&id) {
            Some(&c) if ui.tree().type_name(c).is_some() => Ok(c),
            _ => Err(format!("control {id} is not in the app's tree now (it was built again): ask for the tree again")),
        }
    }

    fn send(&self, msg: AppMsg) {
        self.wire.send(Out::Msg(msg));
    }

    /// The app's designer actions, as declared now.
    fn send_actions(&mut self, ui: &dyn AppUi) {
        self.actions_sent = Some(ui.designer_actions_revision());
        self.send(AppMsg::Actions { actions: ui.designer_actions().into_iter().map(|path| designer_protocol::Action { path }).collect() });
    }

    fn redraw(&self, ui: &dyn AppUi) {
        if let Some(root) = ui.tree().root() {
            self.waker.invalidate(root, Dirty::DRAW);
        }
    }

    /// Requests that change what the frame shows: before it is drawn.
    fn before_draw(&mut self, ui: &mut dyn AppUi, msg: HubToApp) -> Option<HubToApp> {
        match msg {
            HubToApp::SetProperty { request, node, name, value } => {
                if let Err(message) = self.current(ui, node).and_then(|id| set_property(ui.tree_mut(), id, &name, &value)) {
                    self.send(AppMsg::Error { request: Some(request), message });
                }
            }
            HubToApp::Highlight { node, color } => {
                self.box_color = box_color(color.as_deref());
                let node = node.and_then(|n| self.current(ui, n).ok());
                self.set_highlight(ui, node);
            }
            HubToApp::PickMode { on } => {
                self.placing = None; // place mode ends; pick starts fresh
                self.set_picking(ui, on);
            }
            HubToApp::PlaceMode { type_name: Some(type_name) } if !controls::can_create(&type_name) => {
                self.send(AppMsg::Error { request: None, message: format!("{type_name} cannot be created here") })
            }
            // Nothing to cancel: a pick mode on stays on.
            HubToApp::PlaceMode { type_name: None } if self.placing.is_none() => {}
            HubToApp::PlaceMode { type_name } => {
                self.set_picking(ui, false);
                self.set_picking(ui, type_name.is_some());
                self.placing = type_name;
            }
            HubToApp::PlaceAt { type_name, .. } if !controls::can_create(&type_name) => {
                self.send(AppMsg::Error { request: Some(0), message: format!("{type_name} cannot be created here") });
                self.send(AppMsg::Inserted { request: 0, node: None, parent: None, index: None });
            }
            HubToApp::PlaceAt { type_name, x, y } => {
                self.placing = Some(type_name);
                self.place_at(ui, x, y);
            }
            HubToApp::RunAction { path } => {
                if ui.run_designer_action(&path) {
                    // what the action changed: the tree after this frame
                    return Some(HubToApp::GetTree);
                }
                self.send(AppMsg::Error { request: None, message: format!("the app has no action \"{path}\"") });
            }
            HubToApp::OpenScreen { route } => {
                self.send(AppMsg::Error { request: None, message: format!("openScreen {route}: a DrawnUI for Rust app lists no screens") })
            }
            HubToApp::Insert { request, parent, index, type_name, props, children } => {
                let at = self.current(ui, parent);
                match at.and_then(|p| controls::insert(ui.tree_mut(), p, index, &type_name, &props, &children)) {
                    Ok((node, at, skipped)) => {
                        self.send(AppMsg::Inserted { request, node: Some(self.wire_id(node)), parent: Some(parent), index: Some(at) });
                        if !skipped.is_empty() {
                            self.send(AppMsg::Error { request: None, message: format!("inserted {type_name} without {}", skipped.join("; ")) });
                        }
                    }
                    Err(message) => {
                        self.send(AppMsg::Error { request: Some(request), message });
                        self.send(AppMsg::Inserted { request, node: None, parent: Some(parent), index });
                        return None;
                    }
                }
                // The new tree once it is laid out, after this frame.
                return Some(HubToApp::GetTree);
            }
            HubToApp::Remove { request, node } => {
                if let Err(message) = self.current(ui, node).and_then(|id| controls::remove(ui.tree_mut(), id)) {
                    self.send(AppMsg::Error { request: Some(request), message });
                    return None;
                }
                return Some(HubToApp::GetTree);
            }
            HubToApp::Move { request, node, parent, index } => {
                let ids = self.current(ui, node).and_then(|n| self.current(ui, parent).map(|p| (n, p)));
                match ids.and_then(|(n, p)| controls::move_to(ui.tree_mut(), n, p, index)) {
                    Ok(at) => self.send(AppMsg::Moved { request, node: Some(node), parent: Some(parent), index: Some(at) }),
                    Err(message) => {
                        self.send(AppMsg::Error { request: Some(request), message });
                        self.send(AppMsg::Moved { request, node: None, parent: Some(parent), index: Some(index) });
                        return None;
                    }
                }
                return Some(HubToApp::GetTree);
            }
            HubToApp::Copy { request, node } => {
                let spec = self.current(ui, node).ok().and_then(|id| controls::spec(ui.tree_mut(), id));
                self.send(AppMsg::Copied { request, spec });
            }
            HubToApp::GetControls => self.send(AppMsg::Controls { controls: controls::kinds() }),
            other => return Some(other),
        }
        None
    }

    /// Requests that read what was drawn: after it.
    fn after_draw(&mut self, ui: &mut dyn AppUi, msg: HubToApp, frame: &mut Frame) {
        match msg {
            // The host restores and raises the window after this frame (a hidden one too).
            HubToApp::Focus => frame.host.request_window_focus(),
            // Not before the app has built and laid out its controls (but not forever: it may have none).
            // Its next frame is asked for as a hidden one too: a covered or minimized window gets no
            // other (macOS draws it none).
            HubToApp::GetTree if !tree_ready(ui.tree()) && self.tree_waited < TREE_WAIT => {
                self.tree_due = true;
                self.tree_waited += 1;
                self.waker.request_hidden_frame();
            }
            HubToApp::GetTree => {
                self.tree_waited = 0;
                let tree = self.tree(ui, frame.width, frame.height);
                // The actions with every tree of an app that has some: a new session (the hub asks for a
                // tree first) gets them too.
                if ui.designer_actions().is_empty() {
                    self.actions_sent = Some(ui.designer_actions_revision()); // none yet: later ones go out when added
                } else {
                    self.send_actions(ui);
                }
                self.send(AppMsg::Tree(tree));
            }
            HubToApp::GetProps { node } => {
                let props = self.current(ui, node).map(|c| ui.tree().props(c).into_iter().filter(|p| p.settable).map(prop).collect()).unwrap_or_default();
                self.send(AppMsg::Props { node, props });
            }
            HubToApp::Pick { request, x, y } => {
                let node = pick_at(ui, x, y).map(|c| self.wire_id(c));
                self.send(AppMsg::Picked { request, node });
            }
            HubToApp::Snapshot { request } => {
                let (width, height) = (frame.surface.width(), frame.surface.height());
                let info = skia::ImageInfo::new((width, height), skia::ColorType::RGBA8888, skia::AlphaType::Unpremul, None);
                let mut rgba = vec![0u8; width as usize * height as usize * 4];
                if frame.surface.read_pixels(&info, &mut rgba, width as usize * 4, (0, 0)) {
                    self.wire.send(Out::Pixels { request, width: width as u32, height: height as u32, rgba });
                } else {
                    self.send(AppMsg::Error { request: Some(request), message: "the canvas could not be read".into() });
                }
            }
            _ => {}
        }
    }

    /// The app's tree, then each open popup window's content as a root of its own, its `host`
    /// "popup 1", "popup 2" (oldest first).
    fn tree(&mut self, ui: &dyn AppUi, width: f32, height: f32) -> Tree {
        let t = ui.tree();
        let canvas = Rect::from_wh(width, height);
        let main = t.root().map(|r| (r, ui.state_type().to_owned(), true, None));
        let canvas_scale = t.root().and_then(|r| t.base(r)).map_or(1.0, |b| b.scale);
        // A popup may show in a window of its own, past the canvas: not clipped to it. Its content
        // is laid out in its window's pixels, moved here to where the popup shows on the canvas.
        let popups = t.popup_roots().into_iter().enumerate().map(|(i, (popup, root))| {
            let at = t.popup_rect(popup).zip(t.drawn_rect(root)).zip(t.base(root)).map(|((shown, laid), b)| (shown, laid, canvas_scale / b.scale.max(0.01)));
            (root, format!("popup {}", i + 1), false, at)
        });
        let mut nodes = Vec::new();
        for (root, host, clipped, at) in main.into_iter().chain(popups) {
            let place = |r: Rect| match at {
                Some((shown, laid, k)) => Rect::from_xywh(shown.left + (r.left - laid.left) * k, shown.top + (r.top - laid.top) * k, r.width() * k, r.height() * k),
                None => r,
            };
            // Parents before children, children in their parent's order.
            let mut stack: Vec<(ControlId, Option<ControlId>, bool)> = vec![(root, None, true)];
            while let Some((id, parent, parent_visible)) = stack.pop() {
                let Some(base) = t.base(id) else { continue };
                let r = place(t.drawn_rect(id).unwrap_or_else(|| t.screen_rect(id)));
                let visible = parent_visible && base.p.is_visible && !r.is_empty() && (!clipped || Rect::intersects(&r, canvas));
                let text = t.find::<SkiaLabel>(id).map(|l| l.p.text.clone()).or_else(|| t.find::<SkiaButton>(id).map(|b| b.p.text.clone()));
                nodes.push(Node {
                    id: self.wire_id(id),
                    parent: parent.map(|p| self.wire_id(p)),
                    type_name: t.type_name(id).unwrap_or("Control").to_owned(),
                    tag: (!base.p.tag.is_empty()).then(|| base.p.tag.clone()),
                    text: text.filter(|s| !s.is_empty()).map(|s| s.chars().take(40).collect()),
                    host: parent.is_none().then(|| host.clone()),
                    internal: t.is_internal(id),
                    templated: t.is_templated(id),
                    // A row a layout made from its items: the item it shows (Layers reads "item N", and
                    // nothing is dropped into that layout or beside its rows).
                    template_index: parent.filter(|&p| t.is_templated(p)).and(base.context_index).map(|i| i as u64),
                    rect: [r.left, r.top, r.width(), r.height()],
                    visible,
                    ..Default::default()
                });
                let shown = parent_visible && base.p.is_visible;
                stack.extend(t.children(id).iter().rev().map(|&c| (c, Some(id), shown)));
            }
        }
        self.revision += 1;
        Tree { revision: self.revision, width, height, nodes }
    }

    fn set_picking(&mut self, ui: &dyn AppUi, on: bool) {
        if self.picking && !on {
            self.set_highlight(ui, None);
            self.placing = None;
            self.place_line = None;
        }
        self.picking = on;
    }

    /// Pointer input while picking: none of it reaches the app (true). A move highlights what is
    /// under the pointer, a release picks it and ends the mode.
    fn pick_pointer(&mut self, ui: &mut dyn AppUi, kind: PointerKind, x: f32, y: f32) -> bool {
        if !self.picking {
            return false;
        }
        match kind {
            PointerKind::Hover | PointerKind::Move => {
                let (under, line) = match self.placing {
                    // The layout that takes it, and the line where it goes; no box where nothing does.
                    Some(_) => controls::place(ui.tree_mut(), x, y).map_or((None, None), |p| (Some(p.container), p.line)),
                    None => (pick_at(ui, x, y), None),
                };
                if under != self.highlight || line != self.place_line {
                    self.place_line = line;
                    self.set_highlight(ui, under);
                    self.redraw(ui);
                }
            }
            PointerKind::Up if self.placing.is_some() => self.place_at(ui, x, y),
            PointerKind::Up => {
                let node = pick_at(ui, x, y);
                self.picking = false;
                self.set_highlight(ui, node); // the picked control keeps its box
                self.send(AppMsg::Picked { request: 0, node: node.map(|c| self.wire_id(c)) });
                self.redraw(ui);
            }
            _ => {}
        }
        true
    }

    /// The click in place mode: a new control in the container under the pointer, after the child
    /// under it; its box, `inserted` (request 0) and, once laid out, the tree.
    fn place_at(&mut self, ui: &mut dyn AppUi, x: f32, y: f32) {
        let type_name = self.placing.take().unwrap_or_default();
        let target = controls::place(ui.tree_mut(), x, y).map(|p| (p.container, p.index));
        self.set_picking(ui, false);
        let Some((container, index)) = target else {
            self.send(AppMsg::Error { request: Some(0), message: "no control can go here".into() });
            self.send(AppMsg::Inserted { request: 0, node: None, parent: None, index: None });
            return self.redraw(ui);
        };
        match controls::insert(ui.tree_mut(), container, index, &type_name, &[], &[]) {
            Ok((node, at, _)) => {
                self.set_highlight(ui, Some(node));
                self.send(AppMsg::Inserted { request: 0, node: Some(self.wire_id(node)), parent: Some(self.wire_id(container)), index: Some(at) });
                self.tree_due = true;
            }
            Err(message) => {
                self.send(AppMsg::Error { request: Some(0), message });
                self.send(AppMsg::Inserted { request: 0, node: None, parent: Some(self.wire_id(container)), index });
            }
        }
        self.redraw(ui);
    }

    /// The control to box, and the popup that holds it (none: the app's canvas). Looked up once
    /// per change, not per frame.
    fn set_highlight(&mut self, ui: &dyn AppUi, node: Option<ControlId>) {
        self.highlight = node;
        self.highlight_popup = node.and_then(|id| popup_of(ui.tree(), id)).map(|(popup, _)| popup.get());
    }

    /// The box around the highlighted control, over the app's frame: for a control of the canvas
    /// or of a popup drawn on it. A popup in a window of its own gets it in `popup_frame`.
    fn draw_highlight(&mut self, ui: &dyn AppUi, frame: &mut Frame) {
        let Some(id) = self.highlight else { return };
        let Some(r) = ui.tree().drawn_rect(id) else {
            self.set_highlight(ui, None); // the control is gone
            return;
        };
        let on_canvas = self.highlight_popup.is_none_or(|n| !frame.host.popup_windows.iter().any(|w| w.id == n));
        if on_canvas {
            draw_box(frame, r, self.box_color);
            if let Some(line) = self.place_line {
                let mut paint = skia::Paint::default();
                paint.set_anti_alias(false).set_color(self.box_color);
                let canvas = frame.surface.canvas();
                canvas.save();
                canvas.reset_matrix();
                canvas.draw_rect(line, &paint);
                canvas.restore();
            }
        }
    }

    /// The app's frame with the designer's requests around it.
    fn frame(&mut self, ui: &mut dyn AppUi, frame: &mut Frame) -> bool {
        let mut later = Vec::new();
        if std::mem::take(&mut self.tree_due) {
            later.push(HubToApp::GetTree);
        }
        let mut handled = false;
        if self.wire.ready() {
            while let Some(msg) = self.wire.next() {
                handled = true;
                match guarded(|| self.before_draw(ui, msg)) {
                    Ok(Some(msg)) => later.push(msg),
                    Ok(None) => {}
                    Err(message) => self.send(AppMsg::Error { request: None, message }),
                }
            }
        }
        // The designer's changes are in this frame: prepared again after them. macOS's host prepares
        // before the frame, so a removal made after that showed only in the next frame, and the
        // tree sent from this one still had the control (and none came after it). A hidden frame
        // (a covered or minimized window) comes without the host's `prepare`: prepared here too.
        if std::mem::take(&mut self.hidden) || handled {
            App::prepare(ui, frame.width, frame.height, frame.scale, frame.time_ms);
        }
        let more = App::frame(ui, frame);
        // Actions added or removed while the app runs (a screen offering its own): sent once a tree went out.
        if self.actions_sent.is_some_and(|sent| sent != ui.designer_actions_revision()) {
            self.send_actions(ui);
        }
        if self.highlight.is_some() {
            let _ = guarded(|| self.draw_highlight(ui, frame));
        }
        for msg in later {
            if let Err(message) = guarded(|| self.after_draw(ui, msg, frame)) {
                self.send(AppMsg::Error { request: None, message });
            }
        }
        // A tree waiting for the app's controls asks for the next frame.
        more || self.tree_due
    }

    /// A key while picking: Escape ends it, every key stays from the app (true).
    fn key(&mut self, ui: &dyn AppUi, kind: KeyKind, key: drawnui::InputKey) -> bool {
        if !(self.picking && key == "Escape") {
            return false;
        }
        if kind == KeyKind::Down {
            let placing = self.placing.is_some();
            self.set_picking(ui, false);
            self.send(if placing { AppMsg::Inserted { request: 0, node: None, parent: None, index: None } } else { AppMsg::Picked { request: 0, node: None } });
            self.redraw(ui);
        }
        true
    }

    /// The highlight box of a control in popup window `id`, in the window's own pixels.
    fn popup_frame(&mut self, ui: &dyn AppUi, id: u32, frame: &mut Frame) {
        self.boxed = self.boxed.filter(|&(n, ..)| n != id);
        if self.highlight_popup == Some(id)
            && let Some(node) = self.highlight
            && let Some(r) = ui.tree().drawn_rect(node)
        {
            draw_box(frame, r, self.box_color);
            self.boxed = Some((id, node, self.box_color));
        }
    }

    /// Popup window `id`'s highlight box came, moved, went or changed color.
    fn popup_box_changed(&self, id: u32) -> bool {
        let want = self.highlight.filter(|_| self.highlight_popup == Some(id)).map(|node| (node, self.box_color));
        want != self.boxed.filter(|&(n, ..)| n == id).map(|(_, node, color)| (node, color))
    }
}

fn set_property(t: &mut drawnui::Tree, id: ControlId, name: &str, value: &str) -> Result<(), String> {
    // Booleans parse lower case only; editors offer True / False.
    let boolean = t.props(id).iter().any(|p| p.name == name && p.type_name == "bool");
    let value = if boolean { value.to_ascii_lowercase() } else { value.to_owned() };
    t.set_prop(id, name, &value)
}

#[cfg(test)]
pub(crate) fn set_property_for_tests(t: &mut drawnui::Tree, id: ControlId, name: &str, value: &str) -> Result<(), String> {
    set_property(t, id, name, value)
}

/// The app's controls are there and laid out: the root has a child drawn somewhere.
fn tree_ready(t: &drawnui::Tree) -> bool {
    let laid_out = |id: ControlId| t.drawn_rect(id).is_some_and(|r| !r.is_empty());
    t.root().is_some_and(|root| laid_out(root) && t.children(root).iter().any(|&c| laid_out(c)))
}

/// The control a person sees at a canvas pixel; a part the library made (a button's label)
/// counts as the control that owns it.
fn pick_at(ui: &dyn AppUi, x: f32, y: f32) -> Option<ControlId> {
    let t = ui.tree();
    let mut hit = ui.hit_test_visual(Point::new(x, y))?;
    while t.is_internal(hit) {
        hit = t.parent(hit)?;
    }
    Some(hit)
}

/// The popup window `id` is in, with its content's root; `None` for a control of the app's canvas.
fn popup_of(t: &drawnui::Tree, id: ControlId) -> Option<(PopupId, ControlId)> {
    let top = std::iter::successors(Some(id), |&c| t.parent(c)).last()?;
    if Some(top) == t.root() {
        return None;
    }
    t.popup_roots().into_iter().find(|&(_, root)| root == top)
}

/// A 2 px box just outside `r` (pixels of the frame's surface), kept inside the surface: a control
/// that fills it (a snippet's root) gets its box along the edges.
fn draw_box(frame: &mut Frame, r: Rect, color: skia::Color) {
    let mut paint = skia::Paint::default();
    paint.set_style(skia::PaintStyle::Stroke).set_stroke_width(2.0).set_anti_alias(false).set_color(color);
    let (w, h) = (frame.surface.width() as f32, frame.surface.height() as f32);
    let shown = Rect::new((r.left - 1.0).max(1.0), (r.top - 1.0).max(1.0), (r.right + 1.0).min(w - 1.0), (r.bottom + 1.0).min(h - 1.0));
    let canvas = frame.surface.canvas();
    canvas.save();
    canvas.reset_matrix();
    canvas.draw_rect(shown, &paint);
    canvas.restore();
}

/// A property as the editor reads it; Rust types named by what the editor offers for them
/// (number steppers, True / False, the color picker), an enum's cases as a dropdown.
pub(crate) fn prop(p: drawnui::designer::PropInfo) -> Prop {
    let choices = p.choices.map(|cases| {
        // An Option also takes its cleared value, listed as the engine spells it: empty when the
        // cases have a `None` of their own (that `None` is the case), else `none`. First, as "no
        // value" leads a dropdown.
        let own_none = cases.iter().any(|c| c.eq_ignore_ascii_case("none"));
        let cleared = p.type_name.starts_with("Option<").then_some(if own_none { "" } else { "none" });
        cleared.into_iter().chain(cases.iter().copied()).map(String::from).collect()
    });
    let type_name = match p.type_name {
        "f32" => "System.Single",
        "f64" => "System.Double",
        "i32" => "System.Int32",
        "u32" | "usize" => "System.Int64",
        "bool" | "Option<bool>" => "System.Boolean",
        // The engine names a borrowed string with its path (`std::borrow::Cow<'static, str>`).
        t if t == "String" || t.ends_with("Cow<'static, str>") && !t.starts_with("Option<") => "System.String",
        "Color" | "Option<Color>" => "drawnui.skia.Color",
        // `crate::controls::button::SkiaTouchAnimation` reads as the type's own name.
        other if !other.contains('<') => other.rsplit("::").next().unwrap_or(other),
        other => other,
    };
    Prop { name: p.name.into(), type_name: type_name.into(), value: p.value, is_set: p.is_set, is_default: Some(!p.is_set), choices, ..Default::default() }
}

/// Runs a request handler; a panic in it becomes an error answer, never the app's.
fn guarded<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|e| {
        e.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| e.downcast_ref::<String>().cloned()).unwrap_or_else(|| "the agent failed".into())
    })
}

/// The shim: the designer's part goes to [`Core`], everything else to the app as it is.
impl<S: 'static> App for Agent<S> {
    fn frame(&mut self, frame: &mut Frame) -> bool {
        self.core.frame(&mut self.ui, frame)
    }
    fn pointer(&mut self, kind: PointerKind, x: f32, y: f32, time_ms: f64) {
        if !self.core.pick_pointer(&mut self.ui, kind, x, y) {
            App::pointer(&mut self.ui, kind, x, y, time_ms)
        }
    }
    fn pointer_button(&mut self, kind: PointerKind, button: MouseButton, x: f32, y: f32, time_ms: f64) {
        if !self.core.pick_pointer(&mut self.ui, kind, x, y) {
            App::pointer_button(&mut self.ui, kind, button, x, y, time_ms)
        }
    }
    fn pointer_touch(&mut self, kind: PointerKind, x: f32, y: f32, time_ms: f64) {
        if !self.core.pick_pointer(&mut self.ui, kind, x, y) {
            App::pointer_touch(&mut self.ui, kind, x, y, time_ms)
        }
    }
    fn pointer_finger(&mut self, finger: u64, kind: PointerKind, x: f32, y: f32, time_ms: f64) {
        if !self.core.picking {
            App::pointer_finger(&mut self.ui, finger, kind, x, y, time_ms)
        }
    }
    fn context_menu(&mut self, x: f32, y: f32, source: drawnui::ContextMenuSource, time_ms: f64) -> bool {
        self.core.picking || App::context_menu(&mut self.ui, x, y, source, time_ms)
    }
    fn key(&mut self, kind: KeyKind, key: drawnui::InputKey, text: &str, modifiers: drawnui::Modifiers, repeat: bool) -> bool {
        self.core.key(&self.ui, kind, key) || App::key(&mut self.ui, kind, key, text, modifiers, repeat)
    }
    fn popup_frame(&mut self, id: u32, frame: &mut Frame) {
        App::popup_frame(&mut self.ui, id, frame);
        self.core.popup_frame(&self.ui, id, frame);
    }
    fn popup_needs_frame(&self, id: u32) -> bool {
        App::popup_needs_frame(&self.ui, id) || self.core.popup_box_changed(id)
    }

    fn init(&mut self, host: &mut Host) {
        App::init(&mut self.ui, host)
    }
    fn page_font(&mut self, alias: &str, url: &str, weight: i32, host: &mut Host) {
        App::page_font(&mut self.ui, alias, url, weight, host)
    }
    fn prepare(&mut self, width: f32, height: f32, scale: f32, time_ms: f64) {
        App::prepare(&mut self.ui, width, height, scale, time_ms)
    }
    fn rendering_mode(&self) -> drawnui::RenderingModeType {
        App::rendering_mode(&self.ui)
    }
    fn gpu_backend(&self) -> drawnui::GpuBackend {
        App::gpu_backend(&self.ui)
    }
    fn background(&self) -> skia::Color {
        App::background(&self.ui)
    }
    fn gestures(&self) -> drawnui::GesturesMode {
        App::gestures(&self.ui)
    }
    fn blur(&mut self) {
        App::blur(&mut self.ui)
    }
    fn window_focus(&mut self, focused: bool) {
        App::window_focus(&mut self.ui, focused)
    }
    fn focus_out(&mut self) {
        App::focus_out(&mut self.ui)
    }
    fn mark_key_handled(&mut self, key: drawnui::InputKey) {
        App::mark_key_handled(&mut self.ui, key)
    }
    fn system_theme(&mut self, theme: drawnui::SystemTheme) {
        App::system_theme(&mut self.ui, theme)
    }
    fn screen_reader_running(&mut self, running: bool) {
        App::screen_reader_running(&mut self.ui, running)
    }
    fn app_closing(&mut self) -> bool {
        App::app_closing(&mut self.ui)
    }
    fn accessibility_context_menu(&mut self, id: u32) -> bool {
        App::accessibility_context_menu(&mut self.ui, id)
    }
    fn back(&mut self) -> bool {
        App::back(&mut self.ui)
    }
    fn accessibility_activate(&mut self, id: u32) {
        App::accessibility_activate(&mut self.ui, id)
    }
    fn accessibility_focused_id(&self) -> Option<u32> {
        App::accessibility_focused_id(&self.ui)
    }
    fn accessibility_focus(&mut self, id: u32, focused: bool) {
        App::accessibility_focus(&mut self.ui, id, focused)
    }
    fn accessibility_adjust(&mut self, id: u32, up: bool) {
        App::accessibility_adjust(&mut self.ui, id, up)
    }
    fn accessibility_set_value(&mut self, id: u32, value: f64) {
        App::accessibility_set_value(&mut self.ui, id, value)
    }
    fn accessibility_scroll(&mut self, id: u32, x: f32, y: f32) {
        App::accessibility_scroll(&mut self.ui, id, x, y)
    }
    fn accessibility_scroll_into_view(&mut self, id: u32) {
        App::accessibility_scroll_into_view(&mut self.ui, id)
    }
    fn visibility(&mut self, visible: bool) {
        App::visibility(&mut self.ui, visible)
    }
    fn safe_insets(&mut self, insets: drawnui::Thickness) {
        App::safe_insets(&mut self.ui, insets)
    }
    fn device_rotation(&mut self, degrees: i32) {
        App::device_rotation(&mut self.ui, degrees)
    }
    fn wants_device_rotation(&self) -> bool {
        App::wants_device_rotation(&self.ui)
    }
    fn location(&mut self, hash: &str, popped: Option<u32>) {
        App::location(&mut self.ui, hash, popped)
    }
    fn wheel(&mut self, x: f32, y: f32, delta: f32, horizontal: bool, time_ms: f64) -> bool {
        App::wheel(&mut self.ui, x, y, delta, horizontal, time_ms)
    }
    fn asset(&mut self, id: u32, bytes: Vec<u8>) {
        App::asset(&mut self.ui, id, bytes)
    }
    fn image(&mut self, id: u32, decoded: Option<drawnui::Decoded>) {
        App::image(&mut self.ui, id, decoded)
    }
    fn baked(&mut self, id: u32, image: Option<skia::Image>) {
        App::baked(&mut self.ui, id, image)
    }
    fn popup_window_placed(&mut self, id: u32, width: f32, height: f32, scale: f32, offset: Point) {
        App::popup_window_placed(&mut self.ui, id, width, height, scale, offset)
    }
    fn popup_pointer(&mut self, id: u32, kind: PointerKind, button: MouseButton, x: f32, y: f32, time_ms: f64) {
        App::popup_pointer(&mut self.ui, id, kind, button, x, y, time_ms)
    }
    fn popup_wheel(&mut self, id: u32, x: f32, y: f32, delta: f32, horizontal: bool, time_ms: f64) -> bool {
        App::popup_wheel(&mut self.ui, id, x, y, delta, horizontal, time_ms)
    }
    fn close_popup_windows(&mut self) {
        App::close_popup_windows(&mut self.ui)
    }
    fn take_hidden_frame_request(&mut self) -> bool {
        // Asked only while the window is covered; yes: the host draws a frame off screen now.
        self.core.hidden = App::take_hidden_frame_request(&mut self.ui);
        self.core.hidden
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        App::as_any(&self.ui)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drawnui::prelude::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, Receiver, Sender};

    #[test]
    fn base64_matches_rfc4648() {
        for (plain, coded) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(plain.as_bytes()), coded);
        }
    }

    const FONT: &[u8] = include_bytes!("../../../crates/designer-desktop/assets/OpenSans-Regular.ttf");

    #[derive(Default)]
    struct Page {
        taps: u32,
        button: Handle<SkiaButton>,
        label: Handle<SkiaLabel>,
        shape: Handle<SkiaShape>,
        list: Handle<SkiaLayout>,
        a: Handle<SkiaLabel>,
        stack: Handle<SkiaLayout>,
        s1: Handle<SkiaLabel>,
        row: Handle<SkiaLayout>,
        r1: Handle<SkiaLabel>,
        layer: Handle<SkiaLayout>,
    }

    struct Rig {
        agent: Agent<Page>,
        hub: Sender<HubToApp>,
        answers: Receiver<Out>,
        surface: skia::Surface,
        time: f64,
        /// The host's frame requests, counted (`ask_hidden`).
        requests: Arc<std::sync::atomic::AtomicU32>,
    }

    impl Rig {
        fn new() -> Rig {
            Rig::with_font(true)
        }

        /// `font_loaded` false: the font is fetched (id 0) and the first layout waits for it, as
        /// after a hot swap in the browser; `rig.agent.asset(0, ..)` delivers it.
        fn with_font(font_loaded: bool) -> Rig {
            Rig::build(font_loaded, false)
        }

        /// `button_first`: the app after a hot swap that put the button before the label, so the
        /// new arena gives the label's index (and generation) to the button.
        fn build(font_loaded: bool, button_first: bool) -> Rig {
            let ui = Ui::new(Page::default(), move |p: &mut Page| {
                let label = || SkiaLabel::new("Hello");
                let button = || SkiaButton::new("Tap").on_tapped(|_me, p: &mut Page, _cx| p.taps += 1);
                if button_first {
                    SkiaLayout::column().padding(10).spacing(10).children((button().assign(&mut p.button), label().assign(&mut p.label)))
                } else {
                    SkiaLayout::column().padding(10).spacing(10).children((label().assign(&mut p.label), button().assign(&mut p.button)))
                }
            });
            let ui = if font_loaded { ui.font_bytes("Default", FONT) } else { ui.font("Default", "OpenSans-Regular.ttf") };
            Rig::with_ui(ui)
        }

        fn with_ui(ui: Ui<Page>) -> Rig {
            let (hub, inbox) = mpsc::channel();
            let (out, answers) = mpsc::channel();
            let waker = ui.waker();
            let wire = Wire { inbox, queued: Arc::new(AtomicBool::new(false)), out };
            let mut agent = Agent::with(ui, wire, waker);
            // The host's frame requests, counted (see `ask_hidden`).
            let requests = Arc::new(std::sync::atomic::AtomicU32::new(0));
            let mut host = Host::default();
            let r = requests.clone();
            host.on_frame_requested(move || {
                r.fetch_add(1, Ordering::Relaxed);
            });
            App::init(&mut agent, &mut host);
            let surface = skia::surfaces::raster_n32_premul((400, 300)).unwrap();
            let mut rig = Rig { agent, hub, answers, surface, time: 1000.0, requests };
            rig.frames(3);
            // As the designer: the tree first, then requests about its controls.
            rig.ask(HubToApp::GetTree);
            rig
        }

        fn frames(&mut self, n: usize) {
            let (mut gpu, mut host) = (drawnui::Gpu::raster(), Host::default());
            for _ in 0..n {
                self.time += 16.0;
                let mut f = Frame { surface: &mut self.surface, gpu: &mut gpu, width: 400.0, height: 300.0, scale: 1.0, time_ms: self.time, host: &mut host };
                self.agent.frame(&mut f);
            }
        }

        /// A frame as macOS's host draws a shown window: `prepare`, then the frame.
        fn shown_frame(&mut self) {
            App::prepare(&mut self.agent, 400.0, 300.0, 1.0, self.time + 16.0);
            self.frames(1);
        }

        /// The designer's message, then two frames as macOS draws a shown window.
        fn ask_shown(&mut self, msg: HubToApp) -> Vec<AppMsg> {
            self.hub.send(msg).unwrap();
            self.agent.wire.queued.store(true, Ordering::Release);
            self.shown_frame();
            self.shown_frame();
            self.answers.try_iter().map(Out::into_msg).collect()
        }

        fn ask(&mut self, msg: HubToApp) -> Vec<Out> {
            self.hub.send(msg).unwrap();
            self.agent.wire.queued.store(true, Ordering::Release);
            self.frames(1);
            self.answers.try_iter().collect()
        }

        /// The designer's message while the window is covered or minimized, as macOS's host runs
        /// it: a redraw only when the app asks for a frame (`Host::on_frame_requested`), and then a
        /// frame only when it is a hidden one the agent asked for; else nothing is drawn.
        fn ask_hidden(&mut self, msg: HubToApp) -> Vec<AppMsg> {
            self.hub.send(msg).unwrap();
            self.agent.wire.queued.store(true, Ordering::Release);
            self.agent.ui.waker().request_hidden_frame();
            for _ in 0..2 * TREE_WAIT {
                if self.requests.swap(0, Ordering::Relaxed) == 0 {
                    break;
                }
                if App::take_hidden_frame_request(&mut self.agent) {
                    self.frames(1);
                }
            }
            self.answers.try_iter().map(Out::into_msg).collect()
        }

        fn click(&mut self, at: (f32, f32)) {
            for kind in [PointerKind::Hover, PointerKind::Down, PointerKind::Up] {
                self.time += 30.0;
                self.agent.pointer_button(kind, MouseButton::Left, at.0, at.1, self.time);
                self.frames(2);
            }
        }
    }

    #[test]
    fn the_box_takes_the_designers_color_and_magenta_without_a_good_one() {
        assert_eq!(box_color(Some("#8000C853")), skia::Color::from_argb(0x80, 0, 0xC8, 0x53));
        assert_eq!(box_color(Some("#00C853")), skia::Color::from_rgb(0, 0xC8, 0x53));
        assert_eq!((box_color(Some("green")), box_color(Some("")), box_color(None)), (BOX_COLOR, BOX_COLOR, BOX_COLOR));

        // Drawn in that color, a 2 px stroke centered 1 px outside the control.
        let mut rig = Rig::new();
        let label = ControlId::from(rig.agent.ui.state.label);
        let r = rig.agent.ui.tree.drawn_rect(label).unwrap();
        let edge = |rig: &mut Rig| {
            let info = skia::ImageInfo::new((1, 1), skia::ColorType::RGBA8888, skia::AlphaType::Unpremul, None);
            let mut px = [0u8; 4];
            assert!(rig.surface.read_pixels(&info, &mut px, 4, ((r.left - 1.0) as i32, r.center_y() as i32)));
            px
        };
        rig.ask(HubToApp::Highlight { node: Some(rig.agent.wire_id(label)), color: Some("#00C853".into()) });
        assert_eq!(edge(&mut rig), [0, 0xC8, 0x53, 255]);
        rig.ask(HubToApp::Highlight { node: Some(rig.agent.wire_id(label)), color: Some("not a color".into()) });
        assert_eq!(edge(&mut rig), [255, 0, 255, 255]);
    }

    #[test]
    fn an_optional_choice_lists_its_cleared_value_once() {
        use drawnui::controls::image::TransformAspect;
        use drawnui::designer::{PropInfo, PropText};
        fn info<T: PropText>(type_name: &'static str, value: Option<T>) -> PropInfo {
            PropInfo { name: "p", type_name, is_set: value.is_some(), value: value.to_text(), settable: true, choices: <Option<T>>::choices() }
        }
        // A `None` case of its own: cleared = empty, listed first; "None" is the case, once.
        let cleared = prop(info::<TransformAspect>("Option<TransformAspect>", None));
        let choices = cleared.choices.unwrap();
        assert_eq!((cleared.value.as_str(), &choices[..2]), ("", &["".to_owned(), "None".to_owned()][..]));
        assert_eq!(choices.iter().filter(|c| c.eq_ignore_ascii_case("none")).count(), 1);
        assert_eq!(prop(info("Option<TransformAspect>", Some(TransformAspect::None))).value, "None");
        // No `None` case: cleared = `none`, listed first.
        let cleared = prop(info::<LayoutOptions>("Option<LayoutOptions>", None));
        assert_eq!((cleared.value.as_str(), cleared.choices.unwrap()[0].as_str()), ("none", "none"));
    }

    fn trees(outs: Vec<Out>) -> Vec<Tree> {
        outs.into_iter().filter_map(|o| if let AppMsg::Tree(t) = msg(o) { Some(t) } else { None }).collect()
    }

    #[test]
    fn the_tree_waits_for_the_controls_to_be_built_and_laid_out() {
        let mut rig = Rig::with_font(false);
        // The font is still on its way: no layout yet, so no tree of the root alone.
        assert!(trees(rig.ask(HubToApp::GetTree)).is_empty(), "a tree went out before the controls were laid out");
        rig.frames(3);
        assert!(trees(rig.answers.try_iter().collect()).is_empty());
        // The font arrives: the next frame lays the controls out and the tree goes out, all of it.
        rig.agent.asset(0, FONT.to_vec());
        rig.frames(2);
        let trees = trees(rig.answers.try_iter().collect());
        assert_eq!(trees.len(), 1);
        assert!(trees[0].nodes.iter().any(|n| n.type_name == "SkiaButton"), "{:?}", trees[0].nodes);
    }

    #[test]
    fn a_tree_goes_out_anyway_when_the_app_never_lays_out() {
        let mut rig = Rig::with_font(false);
        rig.ask(HubToApp::GetTree);
        rig.frames(TREE_WAIT as usize + 1);
        assert_eq!(trees(rig.answers.try_iter().collect()).len(), 1);
    }

    /// After a hot swap the designer's ids of the old app name nothing in the new one, also where
    /// the new app holds a live control under the same ControlId bits (the engine's arena may give
    /// an index and generation again): the designer's id is never the bits, it is given once. A
    /// request with an old id is refused and nothing moves, in either app.
    #[test]
    fn an_id_from_before_a_swap_never_names_a_control_after_it() {
        // The app before the swap, still alive (a swap may run before the old app is dropped).
        let old = Rig::new();
        let (label, root) = (old.agent.ui.state.label.into(), old.agent.ui.tree.root().unwrap());
        let (stale_label, stale_root) = (old.agent.wire_id(label), old.agent.wire_id(root));
        let mut rig = Rig::build(true, true);
        let button: ControlId = rig.agent.ui.state.button.into();
        // The designer's ids are not the controls' bits: a ControlId the new arena gives again
        // never brings an old id back.
        assert_ne!(stale_label, label.to_bits());
        assert_ne!(rig.agent.wire_id(button), button.to_bits());
        let names = |t: &drawnui::Tree| t.children(t.root().unwrap()).iter().map(|&c| t.type_name(c).unwrap().to_owned()).collect::<Vec<_>>();
        let (before, old_before) = (names(&rig.agent.ui.tree), names(&old.agent.ui.tree));
        let refused: Vec<_> = rig.ask(HubToApp::Move { request: 1, node: stale_label, parent: stale_root, index: 1 }).into_iter().map(msg).collect();
        assert!(matches!(&refused[..], [AppMsg::Error { request: Some(1), message }, AppMsg::Moved { node: None, .. }] if message.contains("not in the app's tree now")), "{refused:?}");
        assert_eq!((names(&rig.agent.ui.tree), names(&old.agent.ui.tree)), (before, old_before), "nothing moved in either app");
        let refused: Vec<_> = rig.ask(HubToApp::SetProperty { request: 2, node: stale_label, name: "is_visible".into(), value: "False".into() }).into_iter().map(msg).collect();
        assert!(matches!(&refused[..], [AppMsg::Error { request: Some(2), .. }]), "{refused:?}");
        // The new app's own ids work.
        let moved: Vec<_> = rig.ask(HubToApp::Move { request: 3, node: rig.agent.wire_id(button), parent: rig.agent.wire_id(rig.agent.ui.tree.root().unwrap()), index: 1 }).into_iter().map(msg).collect();
        assert!(matches!(moved.first(), Some(AppMsg::Moved { node: Some(_), index: Some(1), .. })), "{moved:?}");
        assert_eq!(names(&rig.agent.ui.tree), ["SkiaLabel", "SkiaButton"]);
        // A control taken out: its id names nothing any more.
        let gone = rig.agent.wire_id(rig.agent.ui.state.label.into());
        rig.ask(HubToApp::Remove { request: 4, node: gone });
        let refused: Vec<_> = rig.ask(HubToApp::Move { request: 5, node: gone, parent: rig.agent.wire_id(rig.agent.ui.tree.root().unwrap()), index: 0 }).into_iter().map(msg).collect();
        assert!(matches!(&refused[..], [AppMsg::Error { request: Some(5), .. }, AppMsg::Moved { node: None, .. }]), "{refused:?}");
    }

    #[test]
    fn the_state_crosses_a_hot_swap_through_the_wrapper() {
        #[derive(Default, serde::Serialize, serde::Deserialize)]
        struct Counter {
            count: i32,
        }
        let ui = Ui::new(Counter { count: 7 }, |_: &mut Counter| SkiaLayout::column());
        let (_hub, inbox) = mpsc::channel();
        let (out, _answers) = mpsc::channel();
        let waker = ui.waker();
        let agent = Agent::with(ui, Wire { inbox, queued: Arc::new(AtomicBool::new(false)), out }, waker);
        let saved = drawnui::hot::save::<Counter>(&agent).expect("the wrapper hands its Ui to hot::save");
        let back: Counter = drawnui::hot::restore(Some(&saved));
        assert_eq!(back.count, 7);
    }

    /// macOS's host prepares a frame before the app draws it, so before the agent makes the
    /// designer's changes: an insert, a removal and a value each come back with the tree (and the
    /// properties) as they are after them, the last tree sent being the one after the change.
    #[test]
    fn on_macos_frames_the_tree_after_a_change_has_it() {
        let mut rig = Rig::new();
        rig.shown_frame();
        let root = rig.agent.wire_id(rig.agent.ui.tree.root().unwrap());
        let last_has = |msgs: &[AppMsg], id: u64| msgs.iter().rev().find_map(|m| if let AppMsg::Tree(t) = m { Some(t.nodes.iter().any(|n| n.id == id)) } else { None });
        let outs = rig.ask_shown(HubToApp::Insert { request: 1, parent: root, index: Some(1), type_name: "SkiaLabel".into(), props: vec![("text".into(), "Copy".into())], children: vec![] });
        let Some(&AppMsg::Inserted { node: Some(copy), .. }) = outs.iter().find(|m| matches!(m, AppMsg::Inserted { .. })) else { panic!("{outs:?}") };
        assert_eq!(last_has(&outs, copy), Some(true), "{outs:?}");
        // A value set live, then read: the new one.
        rig.ask_shown(HubToApp::SetProperty { request: 2, node: copy, name: "text".into(), value: "Set".into() });
        let outs = rig.ask_shown(HubToApp::GetProps { node: copy });
        assert!(outs.iter().any(|m| matches!(m, AppMsg::Props { props, .. } if props.iter().any(|p| p.name == "text" && p.value == "Set"))), "{outs:?}");
        let outs = rig.ask_shown(HubToApp::Remove { request: 3, node: copy });
        let first = outs.iter().find_map(|m| if let AppMsg::Tree(t) = m { Some(t.nodes.iter().any(|n| n.id == copy)) } else { None });
        assert_eq!(first, Some(false), "the first tree after the removal is without the copy: {outs:?}");
    }

    /// A covered window (macOS: no frames but the hidden ones the agent asks for, drawn without the
    /// host's `prepare`): an insert and then its removal each come back with the tree as it is
    /// after them.
    #[test]
    fn a_covered_window_answers_inserts_and_removals_with_its_tree() {
        let mut rig = Rig::new();
        // Shown once as macOS's host shows a frame (prepared first), then covered.
        rig.shown_frame();
        let root = rig.agent.ui.tree.root().unwrap();
        let root = rig.agent.wire_id(root);
        let has = |msgs: &[AppMsg], id: u64| msgs.iter().rev().find_map(|m| if let AppMsg::Tree(t) = m { Some(t.nodes.iter().any(|n| n.id == id)) } else { None });
        let outs = rig.ask_hidden(HubToApp::Insert { request: 1, parent: root, index: Some(1), type_name: "SkiaLabel".into(), props: vec![("text".into(), "Copy".into())], children: vec![] });
        let Some(&AppMsg::Inserted { node: Some(copy), .. }) = outs.iter().find(|m| matches!(m, AppMsg::Inserted { .. })) else { panic!("{outs:?}") };
        assert_eq!(has(&outs, copy), Some(true), "a tree with the copy: {outs:?}");
        let outs = rig.ask_hidden(HubToApp::Remove { request: 2, node: copy });
        assert_eq!(has(&outs, copy), Some(false), "a tree without the copy: {outs:?}");
    }

    /// What the host tells the app goes through the agent to it: the system theme, a close the app
    /// holds back (it asks "save first?"), and a screen reader's "show menu".
    #[test]
    fn the_hosts_calls_reach_the_app() {
        #[derive(Default)]
        struct Themed {
            theme: Option<drawnui::SystemTheme>,
            menu: bool,
        }
        let ui = Ui::new(Themed::default(), |_s: &mut Themed| {
            SkiaLayout::column().children(SkiaLabel::new("Hi").on_context_menu(|_me, s: &mut Themed, _cx, _menu| {
                s.menu = true;
                true
            }))
        })
            .font_bytes("Default", FONT)
            .on_system_theme_changed(|s: &mut Themed, theme, _cx| s.theme = Some(theme))
            .on_app_closing(|_s: &mut Themed, _cx| false);
        let (_hub, inbox) = mpsc::channel();
        let (out, _answers) = mpsc::channel();
        let waker = ui.waker();
        let mut agent = Agent::with(ui, Wire { inbox, queued: Arc::new(AtomicBool::new(false)), out }, waker);
        App::system_theme(&mut agent, drawnui::SystemTheme::Dark);
        assert_eq!(agent.ui.state.theme, Some(drawnui::SystemTheme::Dark));
        assert!(!App::app_closing(&mut agent), "the app keeps its window open");
        // A screen reader's "show menu" on the label opens the label's menu (once laid out and drawn).
        let mut surface = skia::surfaces::raster_n32_premul((400, 300)).unwrap();
        let (mut gpu, mut host) = (drawnui::Gpu::raster(), Host::default());
        agent.frame(&mut Frame { surface: &mut surface, gpu: &mut gpu, width: 400.0, height: 300.0, scale: 1.0, time_ms: 16.0, host: &mut host });
        // Accessibility ids are tree indexes: 0 the column (no menu), 1 the label.
        assert!(!App::accessibility_context_menu(&mut agent, 0), "no menu on the column");
        assert!(App::accessibility_context_menu(&mut agent, 1) && agent.ui.state.menu, "the label's menu opened");
        assert!(agent.ui.state.menu);
    }

    /// The app's designer actions go out with the tree and again when they change; RunAction runs one
    /// with the app state, then the tree follows; an unknown one is an error.
    #[test]
    fn designer_actions_are_listed_and_run() {
        #[derive(Default)]
        struct Game {
            level: u32,
        }
        let ui = Ui::new(Game::default(), |_s: &mut Game| SkiaLayout::column().children(SkiaLabel::new("Hi")))
            .font_bytes("Default", FONT)
            .designer_action("Levels/Level 3", |s: &mut Game, _cx| s.level = 3)
            // an action that offers another one, as a screen does while it is shown
            .designer_action("Game/Open options", |_s: &mut Game, cx| cx.add_designer_action::<Game>("Dialogs/Options", |s: &mut Game, _cx| s.level = 0));
        let (hub, inbox) = mpsc::channel();
        let (out, answers) = mpsc::channel();
        let queued = Arc::new(AtomicBool::new(false));
        let waker = ui.waker();
        let mut agent = Agent::with(ui, Wire { inbox, queued: queued.clone(), out }, waker);
        let mut surface = skia::surfaces::raster_n32_premul((400, 300)).unwrap();
        let (mut gpu, mut host) = (drawnui::Gpu::raster(), Host::default());
        let mut time = 0.0;
        let mut ask = |agent: &mut Agent<Game>, msg: Option<HubToApp>| -> Vec<AppMsg> {
            if let Some(msg) = msg {
                hub.send(msg).unwrap();
                queued.store(true, Ordering::Release);
            }
            for _ in 0..3 {
                time += 16.0;
                agent.frame(&mut Frame { surface: &mut surface, gpu: &mut gpu, width: 400.0, height: 300.0, scale: 1.0, time_ms: time, host: &mut host });
            }
            answers.try_iter().map(Out::into_msg).collect()
        };
        let paths = |msgs: &[AppMsg]| msgs.iter().rev().find_map(|m| if let AppMsg::Actions { actions } = m { Some(actions.iter().map(|a| a.path.clone()).collect::<Vec<_>>()) } else { None });
        let outs = ask(&mut agent, Some(HubToApp::GetTree));
        assert_eq!(paths(&outs), Some(vec!["Levels/Level 3".to_owned(), "Game/Open options".to_owned()]), "{outs:?}");
        let outs = ask(&mut agent, Some(HubToApp::RunAction { path: "Levels/Level 3".into() }));
        assert_eq!(agent.ui.state.level, 3);
        assert!(outs.iter().any(|m| matches!(m, AppMsg::Tree(_))), "the tree after it: {outs:?}");
        let outs = ask(&mut agent, Some(HubToApp::RunAction { path: "Nope".into() }));
        assert!(outs.iter().any(|m| matches!(m, AppMsg::Error { request: None, message } if message.contains("no action"))), "{outs:?}");
        // Added while the app runs: the new list goes out.
        let outs = ask(&mut agent, Some(HubToApp::RunAction { path: "Game/Open options".into() }));
        assert_eq!(paths(&outs), Some(vec!["Levels/Level 3".to_owned(), "Game/Open options".to_owned(), "Dialogs/Options".to_owned()]), "{outs:?}");
    }

    #[test]
    fn inserts_removes_moves_and_copies_controls() {
        let mut rig = Rig::new();
        let answers: Vec<_> = rig.ask(HubToApp::GetControls).into_iter().map(msg).collect();
        let [AppMsg::Controls { controls }] = &answers[..] else { panic!("{answers:?}") };
        let label_kind = controls.iter().find(|k| k.type_name == "SkiaLabel").unwrap();
        assert_eq!((label_kind.group.as_str(), label_kind.defaults.clone()), ("Text", vec![("text".to_owned(), "Label".to_owned())]));
        let root = rig.agent.ui.tree.root().unwrap();
        let (label, button) = (ControlId::from(rig.agent.ui.state.label), ControlId::from(rig.agent.ui.state.button));
        let names = |rig: &mut Rig| -> Vec<String> {
            let t = &mut rig.agent.ui.tree;
            t.cx().app_children(root).iter().map(|&c| t.type_name(c).unwrap().to_owned()).collect()
        };

        // The panel lists every type the engine makes, and each one is made by its name with its
        // defaults taking.
        let mut listed: Vec<_> = controls.iter().map(|k| k.type_name.as_str()).collect();
        let mut made = drawnui::designer::control_types().to_vec();
        listed.sort();
        made.sort();
        assert_eq!(listed, made);
        for k in controls {
            let outs: Vec<_> = rig.ask(HubToApp::Insert { request: 9, parent: rig.agent.wire_id(root), index: None, type_name: k.type_name.clone(), props: vec![], children: vec![] }).into_iter().map(msg).collect();
            assert!(!outs.iter().any(|m| matches!(m, AppMsg::Error { .. })), "{}: {outs:?}", k.type_name);
            let Some(AppMsg::Inserted { node: Some(node), .. }) = outs.first() else { panic!("{outs:?}") };
            assert_eq!(rig.agent.ui.tree.type_name(rig.agent.control(*node)), Some(k.type_name.as_str()));
            rig.ask(HubToApp::Remove { request: 10, node: *node });
        }
        assert_eq!(names(&mut rig), ["SkiaLabel", "SkiaButton"]);

        // A duplicate (its values given) taken out again (Undo of Duplicate): the tree after it has
        // the control no more.
        let outs: Vec<_> = rig.ask(HubToApp::Insert { request: 11, parent: rig.agent.wire_id(root), index: Some(1), type_name: "SkiaLabel".into(), props: vec![("text".into(), "Copy".into()), ("font_size".into(), "28".into())], children: vec![] }).into_iter().map(msg).collect();
        let Some(&AppMsg::Inserted { node: Some(copy), .. }) = outs.first() else { panic!("{outs:?}") };
        let outs: Vec<_> = rig.ask(HubToApp::Remove { request: 12, node: copy }).into_iter().map(msg).collect();
        let Some(AppMsg::Tree(after)) = outs.iter().rev().find(|m| matches!(m, AppMsg::Tree(_))) else { panic!("a tree after the removal: {outs:?}") };
        assert!(!after.nodes.iter().any(|n| n.id == copy), "the removed copy is gone from the tree: {:?}", after.nodes.iter().map(|n| (n.id, n.type_name.clone(), n.text.clone())).collect::<Vec<_>>());

        // Insert at 1: a label with its default text, and the laid-out tree after it.
        let outs: Vec<_> = rig.ask(HubToApp::Insert { request: 1, parent: rig.agent.wire_id(root), index: Some(1), type_name: "SkiaLabel".into(), props: vec![], children: vec![] }).into_iter().map(msg).collect();
        let Some(&AppMsg::Inserted { request: 1, node: Some(new), index: Some(1), .. }) = outs.first() else { panic!("{outs:?}") };
        let Some(AppMsg::Tree(tree)) = outs.last() else { panic!("a tree after the insert: {outs:?}") };
        let node = tree.nodes.iter().find(|n| n.id == new).unwrap();
        assert_eq!((node.text.as_deref(), node.rect[3] > 0.0), (Some("Label"), true));
        assert_eq!(names(&mut rig), ["SkiaLabel", "SkiaLabel", "SkiaButton"]);

        // Copy a shape with a child, paste it: the same properties and children.
        let shape = rig.ask(HubToApp::Insert { request: 2, parent: rig.agent.wire_id(root), index: None, type_name: "SkiaShape".into(), props: vec![], children: vec![] });
        let Some(AppMsg::Inserted { node: Some(shape), .. }) = shape.into_iter().map(msg).next() else { panic!() };
        rig.ask(HubToApp::Insert { request: 3, parent: shape, index: None, type_name: "SkiaLabel".into(), props: vec![("text".into(), "Inside".into())], children: vec![] });
        let copied: Vec<_> = rig.ask(HubToApp::Copy { request: 4, node: shape }).into_iter().map(msg).collect();
        let [AppMsg::Copied { request: 4, spec: Some(spec) }] = &copied[..] else { panic!("{copied:?}") };
        assert!(spec.props.iter().any(|p| p.name == "width_request" && p.value == "80" && p.type_name == "System.Single"), "{spec:?}");
        assert_eq!(spec.children.len(), 1);
        let pasted: Vec<_> = rig
            .ask(HubToApp::Insert { request: 5, parent: rig.agent.wire_id(root), index: None, type_name: spec.type_name.clone(), props: spec.props.iter().map(|p| (p.name.clone(), p.value.clone())).collect(), children: spec.children.clone() })
            .into_iter()
            .map(msg)
            .collect();
        let Some(AppMsg::Inserted { node: Some(paste), .. }) = pasted.first() else { panic!("{pasted:?}") };
        let pasted = rig.agent.control(*paste);
        assert_eq!(controls::spec(&mut rig.agent.ui.tree, pasted).as_ref(), Some(spec));

        // Move among siblings (the same parent), then into another control.
        let moved: Vec<_> = rig.ask(HubToApp::Move { request: 6, node: rig.agent.wire_id(button), parent: rig.agent.wire_id(root), index: 0 }).into_iter().map(msg).collect();
        assert_eq!(moved.first(), Some(&AppMsg::Moved { request: 6, node: Some(rig.agent.wire_id(button)), parent: Some(rig.agent.wire_id(root)), index: Some(0) }));
        assert_eq!(names(&mut rig)[..3], ["SkiaButton", "SkiaLabel", "SkiaLabel"]);
        let into: Vec<_> = rig.ask(HubToApp::Move { request: 7, node: rig.agent.wire_id(label), parent: shape, index: 0 }).into_iter().map(msg).collect();
        assert_eq!(into.first(), Some(&AppMsg::Moved { request: 7, node: Some(rig.agent.wire_id(label)), parent: Some(shape), index: Some(0) }));
        assert_eq!(rig.agent.ui.tree.parent(label), Some(rig.agent.control(shape)));

        // Remove; the root and a button's own label cannot be removed.
        rig.ask(HubToApp::Remove { request: 8, node: new });
        rig.frames(1);
        assert!(rig.agent.ui.tree.type_name(rig.agent.control(new)).is_none(), "removed");
        let root_refused: Vec<_> = rig.ask(HubToApp::Remove { request: 11, node: rig.agent.wire_id(root) }).into_iter().map(msg).collect();
        assert!(matches!(&root_refused[..], [AppMsg::Error { request: Some(11), .. }]), "{root_refused:?}");
        // Into a label: refused.
        let into_label: Vec<_> = rig.ask(HubToApp::Insert { request: 12, parent: rig.agent.wire_id(label), index: None, type_name: "SkiaLabel".into(), props: vec![], children: vec![] }).into_iter().map(msg).collect();
        assert!(matches!(&into_label[..], [AppMsg::Error { request: Some(12), .. }, AppMsg::Inserted { node: None, .. }]), "{into_label:?}");
    }

    #[test]
    fn moves_across_parents_and_refuses_a_templated_layout() {
        let ui = Ui::new(Page::default(), |p: &mut Page| {
            SkiaLayout::column().padding(10).spacing(10).children((
                SkiaLabel::new("Hello").assign(&mut p.label),
                SkiaShape::new().width_request(200).height_request(80).assign(&mut p.shape),
                SkiaButton::new("Tap").on_tapped(|_me, p: &mut Page, _cx| p.taps += 1).assign(&mut p.button),
                SkiaLayout::column().assign(&mut p.list).items(|_: &Page| 3, || (SkiaLabel::new("row"), ()), |_: &(), _: &Page, _, _| {}),
                SkiaLayout::column().items(|_: &Page| 0, || (SkiaLabel::new("row"), ()), |_: &(), _: &Page, _, _| {}),
            ))
        })
        .font_bytes("Default", FONT);
        let mut rig = Rig::with_ui(ui);
        let s = &rig.agent.ui.state;
        let (label, shape, button, list) = (ControlId::from(s.label), ControlId::from(s.shape), ControlId::from(s.button), ControlId::from(s.list));
        let wire = |rig: &mut Rig, id: ControlId| rig.agent.wire_id(id);
        let error = |outs: &[AppMsg], request: u32| matches!(outs.first(), Some(AppMsg::Error { request: Some(r), .. }) if *r == request);

        // The button goes into the shape and keeps its handler: a tap on it still counts.
        let (b, sh) = (wire(&mut rig, button), wire(&mut rig, shape));
        let moved: Vec<_> = rig.ask(HubToApp::Move { request: 1, node: b, parent: sh, index: 5 }).into_iter().map(msg).collect();
        assert_eq!(moved.first(), Some(&AppMsg::Moved { request: 1, node: Some(b), parent: Some(sh), index: Some(0) }));
        assert_eq!(rig.agent.ui.tree.parent(button), Some(shape));
        rig.frames(2);
        let r = rig.agent.ui.tree.drawn_rect(button).unwrap();
        assert!(r.left >= rig.agent.ui.tree.drawn_rect(shape).unwrap().left, "laid out inside the shape: {r:?}");
        rig.click(((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0));
        assert_eq!(rig.agent.ui.state.taps, 1);

        // Not into itself, not into a label.
        let l = wire(&mut rig, label);
        let outs: Vec<_> = rig.ask(HubToApp::Move { request: 2, node: sh, parent: sh, index: 0 }).into_iter().map(msg).collect();
        assert!(error(&outs, 2), "{outs:?}");
        let outs: Vec<_> = rig.ask(HubToApp::Move { request: 3, node: sh, parent: l, index: 0 }).into_iter().map(msg).collect();
        assert!(error(&outs, 3), "{outs:?}");

        // A layout made from items takes no control, by move or insert, and its rows stay.
        let li = wire(&mut rig, list);
        let outs: Vec<_> = rig.ask(HubToApp::Move { request: 4, node: l, parent: li, index: 0 }).into_iter().map(msg).collect();
        assert!(matches!(outs.first(), Some(AppMsg::Error { request: Some(4), message }) if message.contains("items")), "{outs:?}");
        assert_eq!(rig.agent.ui.tree.parent(label), rig.agent.ui.tree.root());
        let outs: Vec<_> = rig.ask(HubToApp::Insert { request: 5, parent: li, index: None, type_name: "SkiaLabel".into(), props: vec![], children: vec![] }).into_iter().map(msg).collect();
        assert!(matches!(outs.first(), Some(AppMsg::Error { request: Some(5), message }) if message.contains("items")), "{outs:?}");
        // Its rows reach the designer with the item each shows; the layout and the app's controls without.
        let tree = trees(rig.ask(HubToApp::GetTree)).pop().expect("a tree");
        let li_node = tree.nodes.iter().find(|n| n.id == li).unwrap();
        let rows: Vec<_> = tree.nodes.iter().filter(|n| n.parent == Some(li)).map(|n| n.template_index).collect();
        assert_eq!((li_node.template_index, rows), (None, vec![Some(0), Some(1), Some(2)]));
        assert!(tree.nodes.iter().filter(|n| n.parent != Some(li)).all(|n| n.template_index.is_none()));
        // Both items layouts say so, the one with no items too; no other control does.
        let templated: Vec<_> = tree.nodes.iter().filter(|n| n.templated).map(|n| n.id).collect();
        let root = rig.agent.wire_id(rig.agent.ui.tree.root().unwrap());
        assert!(templated.len() == 2 && templated.contains(&li) && !templated.contains(&sh) && !templated.contains(&root), "{templated:?}");
        let row = rig.agent.ui.tree.children(list).first().copied().expect("a row");
        let r = wire(&mut rig, row);
        let outs: Vec<_> = rig.ask(HubToApp::Move { request: 6, node: r, parent: sh, index: 0 }).into_iter().map(msg).collect();
        assert!(error(&outs, 6), "{outs:?}");
        let outs: Vec<_> = rig.ask(HubToApp::Remove { request: 7, node: r }).into_iter().map(msg).collect();
        assert!(error(&outs, 7), "{outs:?}");
    }

    /// Measured: a control of `ty`, 300 x 200 first in the rig's column (so at 10,10), keeps a shape
    /// inserted before its first layout (as a paste builds it) with no app children of its own beside
    /// it, then one moved into it after, in that order, and draws the first one inside its box (the
    /// second may be laid out past it: a carousel's next slide).
    fn holds_and_draws(ty: &str) -> bool {
        let mut rig = Rig::new();
        let root = rig.agent.ui.tree.root().unwrap();
        let shape = |t: &mut drawnui::Tree, parent: ControlId, color: &str, margin: &str| {
            let n = t.cx().app_children(parent).len();
            let id = t.cx().insert_child(parent, n, drawnui::designer::create("SkiaShape").unwrap());
            for (k, v) in [("width_request", "40"), ("height_request", "40"), ("background_color", color), ("margin", margin)] {
                t.set_prop(id, k, v).unwrap();
            }
            id
        };
        let t = &mut rig.agent.ui.tree;
        let x = t.cx().insert_child(root, 0, drawnui::designer::create(ty).unwrap());
        let _ = t.set_prop(x, "width_request", "300");
        let _ = t.set_prop(x, "height_request", "200");
        let a = shape(t, x, "#FFFF0000", "0,0,0,0");
        rig.frames(2);
        if rig.agent.ui.tree.cx().app_children(x) != [a] {
            return false;
        }
        let t = &mut rig.agent.ui.tree;
        let b = shape(t, root, "#FF0000FF", "60,0,0,0");
        if !t.cx().move_to(b, x, 99) {
            return false;
        }
        rig.frames(4);
        let t = &mut rig.agent.ui.tree;
        let laid = |r: Option<Rect>| r.is_some_and(|r| r.width() > 0.0 && r.height() > 0.0);
        if t.cx().app_children(x) != [a, b] || !laid(t.drawn_rect(a)) || !laid(t.drawn_rect(b)) {
            return false;
        }
        let r = t.drawn_rect(a).unwrap();
        if r.left < 10.0 || r.top < 10.0 || r.right > 310.0 || r.bottom > 210.0 {
            return false;
        }
        let info = skia::ImageInfo::new((1, 1), skia::ColorType::RGBA8888, skia::AlphaType::Unpremul, None);
        let mut px = [0u8; 4];
        rig.surface.read_pixels(&info, &mut px, 4, (r.center_x() as i32, r.center_y() as i32)) && px == [255, 0, 0, 255]
    }

    #[test]
    fn a_container_is_a_control_that_holds_and_draws_what_goes_into_it() {
        let kinds = controls::kinds();
        for k in &kinds {
            assert_eq!(k.container, holds_and_draws(&k.type_name), "{}", k.type_name);
        }
        assert!(kinds.iter().filter(|k| k.container).any(|k| k.type_name == "SkiaLayout"));
    }

    #[test]
    fn a_controls_own_caption_is_a_part_the_designer_cannot_edit() {
        let mut rig = Rig::new();
        let root = rig.agent.ui.tree.root().unwrap();
        let built = ControlId::from(rig.agent.ui.state.button);
        // A button and an editor as the designer inserts them, and the app's own button.
        let mut owners = vec![built];
        for ty in ["SkiaButton", "SkiaEditor"] {
            let outs: Vec<_> = rig.ask(HubToApp::Insert { request: 1, parent: rig.agent.wire_id(root), index: None, type_name: ty.into(), props: vec![], children: vec![] }).into_iter().map(msg).collect();
            let Some(AppMsg::Inserted { node: Some(node), .. }) = outs.first() else { panic!("{outs:?}") };
            owners.push(rig.agent.control(*node));
        }
        rig.frames(2);
        for owner in owners {
            let t = &mut rig.agent.ui.tree;
            let name = t.type_name(owner).unwrap().to_owned();
            let caption = t.children(owner).iter().copied().find(|&c| t.type_name(c) == Some("SkiaLabel")).unwrap_or_else(|| panic!("{name}: no caption"));
            assert!(t.is_internal(caption), "{name}: the engine tells its caption is its own");
            let tree: Vec<_> = trees(rig.ask(HubToApp::GetTree));
            let node = tree.last().unwrap().nodes.iter().find(|n| n.id == rig.agent.wire_id(caption)).cloned().unwrap();
            assert!(node.internal, "{name}: the designer sees a part");
            let (c, o, r) = (rig.agent.wire_id(caption), rig.agent.wire_id(owner), rig.agent.wire_id(root));
            let error = |outs: Vec<Out>| matches!(outs.into_iter().map(msg).next(), Some(AppMsg::Error { .. }));
            assert!(error(rig.ask(HubToApp::Remove { request: 2, node: c })), "{name}: removed");
            assert!(error(rig.ask(HubToApp::Move { request: 3, node: c, parent: r, index: 0 })), "{name}: moved out");
            assert!(error(rig.ask(HubToApp::Insert { request: 4, parent: c, index: None, type_name: "SkiaLabel".into(), props: vec![], children: vec![] })), "{name}: inserted into");
            let label = rig.agent.wire_id(ControlId::from(rig.agent.ui.state.label));
            assert!(error(rig.ask(HubToApp::Move { request: 5, node: label, parent: c, index: 0 })), "{name}: moved into");
            assert_eq!(rig.agent.ui.tree.parent(caption), Some(owner), "{name}: still its own");
            // A copy is the control alone: a paste must not add a second caption.
            let copied: Vec<_> = rig.ask(HubToApp::Copy { request: 6, node: o }).into_iter().map(msg).collect();
            let [AppMsg::Copied { spec: Some(spec), .. }] = &copied[..] else { panic!("{copied:?}") };
            assert!(spec.children.is_empty(), "{name}: {spec:?}");
        }
    }

    /// Every row of an items layout is the same template: nothing goes into, out of or is taken
    /// from any control inside a row, at any depth.
    #[test]
    fn nothing_changes_inside_a_row_made_from_items() {
        let ui = Ui::new(Page::default(), |p: &mut Page| {
            SkiaLayout::column().padding(10).spacing(10).children((
                SkiaLabel::new("Hello").assign(&mut p.label),
                SkiaLayout::column().assign(&mut p.list).items(|_: &Page| 2, || (SkiaLayout::column().children((SkiaLayout::column().children((SkiaLabel::new("in"),)),)), ()), |_: &(), _: &Page, _, _| {}),
            ))
        })
        .font_bytes("Default", FONT);
        let mut rig = Rig::with_ui(ui);
        let list = ControlId::from(rig.agent.ui.state.list);
        let t = &rig.agent.ui.tree;
        let row = t.children(list).first().copied().expect("a row");
        let inner = t.children(row).first().copied().filter(|&c| t.type_name(c) == Some("SkiaLayout")).expect("the row's own layout");
        let deep = t.children(inner).first().copied().expect("a label inside it");
        let (r, i, d, l) = (rig.agent.wire_id(row), rig.agent.wire_id(inner), rig.agent.wire_id(deep), rig.agent.wire_id(ControlId::from(rig.agent.ui.state.label)));
        let refused = |outs: Vec<Out>| matches!(outs.into_iter().map(msg).next(), Some(AppMsg::Error { message, .. }) if message.contains("items"));
        assert!(refused(rig.ask(HubToApp::Insert { request: 1, parent: i, index: None, type_name: "SkiaLabel".into(), props: vec![], children: vec![] })), "insert into a row's layout");
        assert!(refused(rig.ask(HubToApp::Insert { request: 2, parent: r, index: None, type_name: "SkiaLabel".into(), props: vec![], children: vec![] })), "insert into a row");
        assert!(refused(rig.ask(HubToApp::Move { request: 3, node: l, parent: i, index: 0 })), "move into a row's layout");
        assert!(refused(rig.ask(HubToApp::Move { request: 4, node: d, parent: rig.agent.wire_id(rig.agent.ui.tree.root().unwrap()), index: 0 })), "move out of a row");
        assert!(refused(rig.ask(HubToApp::Remove { request: 5, node: d })), "remove inside a row");
        assert_eq!(rig.agent.ui.tree.parent(deep), Some(inner), "nothing moved");
    }

    /// A new snippet: one empty stack and nothing in it. The place hover boxes it, the click puts the
    /// control into it.
    #[test]
    fn place_mode_takes_a_lone_empty_root() {
        let ui = Ui::new(Page::default(), |_: &mut Page| SkiaLayout::column().fill().padding(16).spacing(8)).font_bytes("Default", FONT);
        let mut rig = Rig::with_ui(ui);
        let root = rig.agent.ui.tree.root().unwrap();
        rig.ask(HubToApp::PlaceMode { type_name: Some("SkiaLabel".into()) });
        rig.agent.pointer_button(PointerKind::Hover, MouseButton::Left, 200.0, 150.0, rig.time);
        assert_eq!(rig.agent.highlight, Some(root), "the hover boxes the empty root");
        // It fills the canvas: the box is drawn just inside the canvas's edges, where it shows.
        rig.frames(1);
        let px = |rig: &mut Rig, x: i32, y: i32| {
            let info = skia::ImageInfo::new((1, 1), skia::ColorType::RGBA8888, skia::AlphaType::Unpremul, None);
            let mut p = [0u8; 4];
            assert!(rig.surface.read_pixels(&info, &mut p, 4, (x, y)));
            p
        };
        assert_eq!((px(&mut rig, 0, 150), px(&mut rig, 200, 0), px(&mut rig, 399, 150), px(&mut rig, 200, 299)), ([255, 0, 255, 255], [255, 0, 255, 255], [255, 0, 255, 255], [255, 0, 255, 255]));
        rig.click((200.0, 150.0));
        let outs: Vec<_> = rig.answers.try_iter().map(msg).collect();
        assert!(outs.iter().any(|m| matches!(m, AppMsg::Inserted { request: 0, node: Some(_), parent: Some(p), .. } if *p == rig.agent.wire_id(root))), "{outs:?}");
        // A drop from the editor's Controls onto the canvas: the same place, with no place mode.
        let outs: Vec<_> = rig.ask(HubToApp::PlaceAt { type_name: "SkiaButton".into(), x: 390.0, y: 290.0 }).into_iter().map(msg).collect();
        assert!(matches!(outs.first(), Some(AppMsg::Inserted { request: 0, node: Some(_), parent: Some(p), index: Some(1) }) if *p == rig.agent.wire_id(root)), "{outs:?}");
        assert!(!rig.agent.picking);
    }

    /// Where a placed control lands: into the control under the pointer when it holds children, at
    /// the index the pointer is at; else next to it, before or after by the half the pointer is in.
    #[test]
    fn place_mode_puts_the_control_where_the_pointer_is() {
        let label = |text: &'static str| SkiaLabel::new(text).width_request(60).height_request(20);
        let ui = Ui::new(Page::default(), move |p: &mut Page| {
            SkiaLayout::column().padding(10).spacing(10).children((
                label("A").assign(&mut p.a),
                SkiaLayout::column().spacing(20).width_request(200).children((label("s1").assign(&mut p.s1), label("s2"))).assign(&mut p.stack),
                SkiaLayout::row().spacing(10).children((label("r1").assign(&mut p.r1), label("r2"))).assign(&mut p.row),
                SkiaLayout::new().width_request(200).height_request(40).children((label("in"),)).assign(&mut p.layer),
                SkiaLayout::column().items(|_: &Page| 1, || (SkiaLayout::column().width_request(80).height_request(20), ()), |_: &(), _: &Page, _, _| {}).assign(&mut p.list),
            ))
        })
        .font_bytes("Default", FONT);
        let mut rig = Rig::with_ui(ui);
        let rect = |rig: &Rig, id: ControlId| rig.agent.ui.tree.drawn_rect(id).unwrap();
        let s = &rig.agent.ui.state;
        let (root, a, stack, s1, row, r1, layer, cell) = (rig.agent.ui.tree.root().unwrap(), s.a.id(), s.stack.id(), s.s1.id(), s.row.id(), s.r1.id(), s.layer.id(), s.list.id());
        let place = |rig: &mut Rig, at: (f32, f32)| -> Option<(ControlId, Option<u32>)> {
            rig.ask(HubToApp::PlaceMode { type_name: Some("SkiaLabel".into()) });
            rig.click(at);
            let outs: Vec<_> = rig.answers.try_iter().map(msg).collect();
            outs.into_iter().find_map(|m| match m {
                AppMsg::Inserted { request: 0, node: Some(n), parent: Some(p), index } => {
                    let (n, p) = (rig.agent.control(n), rig.agent.control(p));
                    rig.agent.ui.tree.cx().remove(n);
                    rig.frames(2);
                    Some((p, index))
                }
                _ => None,
            })
        };
        // Into the stack, in the gap between its two labels: at 1.
        let (r, s1r) = (rect(&rig, stack), rect(&rig, s1));
        assert_eq!(place(&mut rig, (r.left + 150.0, s1r.bottom + 10.0)), Some((stack, Some(1))));
        // Beside label A in the column: its top half before it, its bottom half after it.
        let ar = rect(&rig, a);
        assert_eq!(place(&mut rig, (ar.center_x(), ar.top + 3.0)), Some((root, Some(0))));
        assert_eq!(place(&mut rig, (ar.center_x(), ar.bottom - 3.0)), Some((root, Some(1))));
        // Beside r1 in the row: its left half before it, its right half after it.
        let rr = rect(&rig, r1);
        assert_eq!(place(&mut rig, (rr.left + 3.0, rr.center_y())), Some((row, Some(0))));
        assert_eq!(place(&mut rig, (rr.right - 3.0, rr.center_y())), Some((row, Some(1))));
        // The layer's empty area: into it, last (after its one label).
        let lr = rect(&rig, layer);
        assert_eq!(place(&mut rig, (lr.right - 5.0, lr.bottom - 5.0)), Some((layer, Some(1))));
        // A template's cell: nothing.
        let cr = rect(&rig, rig.agent.ui.tree.children(cell)[0]);
        assert_eq!(place(&mut rig, (cr.center_x(), cr.center_y())), None);
        // The hover over the stack's gap boxes the stack, with a line across it at the index.
        rig.ask(HubToApp::PlaceMode { type_name: Some("SkiaLabel".into()) });
        rig.agent.pointer_button(PointerKind::Hover, MouseButton::Left, r.left + 150.0, s1r.bottom + 10.0, rig.time);
        assert_eq!(rig.agent.highlight, Some(stack));
        let line = rig.agent.place_line.expect("a line at the index");
        let s2 = rig.agent.ui.tree.cx().app_children(stack)[1];
        let s2_top = rect(&rig, s2).top;
        assert!(line.height() <= 2.0 && line.width() >= r.width() - 1.0 && (line.center_y() - s2_top).abs() <= 1.0, "{line:?}");
    }

    #[test]
    fn place_mode_takes_a_layouts_empty_area_drawn_or_not() {
        let mut rig = Rig::new();
        let root = rig.agent.ui.tree.root().unwrap();
        // A nested column with no background (the engine's hit test looks through it).
        let outs: Vec<_> = rig
            .ask(HubToApp::Insert { request: 1, parent: rig.agent.wire_id(root), index: None, type_name: "SkiaLayout".into(), props: vec![("height_request".into(), "80".into()), ("width_request".into(), "200".into())], children: vec![] })
            .into_iter()
            .map(msg)
            .collect();
        let Some(&AppMsg::Inserted { node: Some(nested), .. }) = outs.first() else { panic!("{outs:?}") };
        rig.frames(1);
        let n = rig.agent.ui.tree.drawn_rect(rig.agent.control(nested)).unwrap();
        let place_at = |rig: &mut Rig, at: (f32, f32)| -> Vec<AppMsg> {
            rig.ask(HubToApp::PlaceMode { type_name: Some("SkiaLabel".into()) });
            rig.click(at);
            rig.answers.try_iter().map(msg).filter(|m| !matches!(m, AppMsg::Tree(_))).collect()
        };

        // The nested column's empty area: into it, last.
        let outs = place_at(&mut rig, (n.center_x(), n.center_y()));
        assert!(matches!(&outs[..], [AppMsg::Inserted { node: Some(_), parent: Some(p), index: Some(0), .. }] if *p == nested), "{outs:?}");
        // The root column's empty area below its children: into it, last (label, button, nested, new).
        let r = rig.agent.ui.tree.drawn_rect(root).unwrap();
        let outs = place_at(&mut rig, (r.left + 5.0, r.bottom - 3.0));
        assert!(matches!(&outs[..], [AppMsg::Inserted { node: Some(_), parent: Some(p), index: Some(3), .. }] if *p == rig.agent.wire_id(root)), "{outs:?}");
        // Outside every layout: refused.
        let outs = place_at(&mut rig, (r.right + 5.0, r.bottom + 5.0));
        assert!(matches!(&outs[..], [AppMsg::Error { .. }, AppMsg::Inserted { node: None, .. }]), "{outs:?}");
    }

    #[test]
    fn place_mode_inserts_into_the_container_under_the_pointer() {
        let mut rig = Rig::new();
        let root = rig.agent.ui.tree.root().unwrap();
        let button = ControlId::from(rig.agent.ui.state.button);
        let r = rig.agent.ui.tree.drawn_rect(button).unwrap();
        let at = (r.center_x(), r.center_y());
        let place = |rig: &mut Rig| rig.ask(HubToApp::PlaceMode { type_name: Some("SkiaLabel".into()) });

        // A type the app cannot make is refused at once.
        let refused: Vec<_> = rig.ask(HubToApp::PlaceMode { type_name: Some("Nope".into()) }).into_iter().map(msg).collect();
        assert!(matches!(&refused[..], [AppMsg::Error { .. }]) && !rig.agent.picking, "{refused:?}");

        // The hover boxes the container (the column), not the button under the pointer.
        assert!(place(&mut rig).is_empty());
        rig.agent.pointer_button(PointerKind::Hover, MouseButton::Left, at.0, at.1, rig.time);
        assert_eq!(rig.agent.highlight, Some(root));
        // Esc cancels: inserted with no node.
        assert!(rig.agent.key(KeyKind::Down, "Escape", "", drawnui::Modifiers::default(), false));
        assert_eq!(rig.answers.try_iter().map(msg).collect::<Vec<_>>(), vec![AppMsg::Inserted { request: 0, node: None, parent: None, index: None }]);
        assert!(!rig.agent.picking);

        // The click inserts after the child under it (the button is child 1), never reaches the app,
        // boxes the new control, then the tree goes out once laid out.
        place(&mut rig);
        rig.click(at);
        let outs: Vec<_> = rig.answers.try_iter().map(msg).collect();
        let Some(&AppMsg::Inserted { request: 0, node: Some(node), parent: Some(parent), index: Some(2) }) = outs.first() else { panic!("{outs:?}") };
        assert_eq!(parent, rig.agent.wire_id(root));
        let Some(AppMsg::Tree(tree)) = outs.iter().find(|m| matches!(m, AppMsg::Tree(_))) else { panic!("no tree: {outs:?}") };
        assert!(tree.nodes.iter().any(|n| n.id == node && n.text.as_deref() == Some("Label") && n.rect[3] > 0.0));
        assert_eq!((rig.agent.ui.state.taps, rig.agent.highlight.map(|c| rig.agent.wire_id(c)), rig.agent.picking), (0, Some(node), false));

        // PlaceMode None with no place mode leaves pick mode alone.
        rig.ask(HubToApp::PickMode { on: true });
        rig.ask(HubToApp::PlaceMode { type_name: None });
        assert!(rig.agent.picking);
    }

    fn msg(out: Out) -> AppMsg {
        match out {
            Out::Msg(m) => m,
            Out::Pixels { request, width, height, .. } => AppMsg::Snapshot { request, png_base64: String::new(), width, height },
        }
    }

    #[test]
    fn answers_the_designer_and_picks_without_the_app_seeing_the_click() {
        let mut rig = Rig::new();
        let (button, label) = (ControlId::from(rig.agent.ui.state.button), ControlId::from(rig.agent.ui.state.label));

        // The tree: types, texts, the button's own label marked as the library's.
        let answers: Vec<_> = rig.ask(HubToApp::GetTree).into_iter().map(msg).collect();
        let [AppMsg::Tree(tree)] = &answers[..] else { panic!("{answers:?}") };
        let node = |id: ControlId| tree.nodes.iter().find(|n| n.id == rig.agent.wire_id(id)).unwrap().clone();
        assert_eq!((node(label).type_name.as_str(), node(label).text.as_deref()), ("SkiaLabel", Some("Hello")));
        let b = node(button);
        assert_eq!(b.type_name, "SkiaButton");
        assert!(tree.nodes.iter().any(|n| n.parent == Some(b.id) && n.internal), "the button's label is internal");
        let at = (b.rect[0] + b.rect[2] / 2.0, b.rect[1] + b.rect[3] / 2.0);

        // A point pick lands on the button, not on its label.
        let picked: Vec<_> = rig.ask(HubToApp::Pick { request: 7, x: at.0, y: at.1 }).into_iter().map(msg).collect();
        assert_eq!(picked, vec![AppMsg::Picked { request: 7, node: Some(b.id) }]);

        // Pick mode: the click picks, keeps the box and never reaches the app.
        assert!(rig.ask(HubToApp::PickMode { on: true }).is_empty());
        rig.click(at);
        assert_eq!(rig.answers.try_iter().map(msg).collect::<Vec<_>>(), vec![AppMsg::Picked { request: 0, node: Some(b.id) }]);
        assert_eq!((rig.agent.ui.state.taps, rig.agent.highlight, rig.agent.picking), (0, Some(button), false));
        // Out of pick mode the app gets its clicks again.
        rig.click(at);
        assert_eq!(rig.agent.ui.state.taps, 1);

        // Esc cancels the mode.
        rig.ask(HubToApp::PickMode { on: true });
        assert!(rig.agent.key(KeyKind::Down, "Escape", "", drawnui::Modifiers::default(), false));
        assert_eq!(rig.answers.try_iter().map(msg).collect::<Vec<_>>(), vec![AppMsg::Picked { request: 0, node: None }]);

        // Live edits: a bool spelled as the editor spells it, a bad value refused with the request id.
        rig.ask(HubToApp::SetProperty { request: 1, node: rig.agent.wire_id(label), name: "is_visible".into(), value: "False".into() });
        let props: Vec<_> = rig.ask(HubToApp::GetProps { node: rig.agent.wire_id(label) }).into_iter().map(msg).collect();
        let [AppMsg::Props { props, .. }] = &props[..] else { panic!("{props:?}") };
        let visible = props.iter().find(|p| p.name == "is_visible").unwrap();
        assert_eq!((visible.type_name.as_str(), visible.value.as_str(), visible.is_default), ("System.Boolean", "false", Some(false)));
        // Only what can be set: a gradient (listed by the engine, not settable from text) is left out.
        assert!(props.iter().any(|p| p.name == "font_size") && !props.iter().any(|p| p.name == "fill_gradient" || p.name == "text_gradient"), "{props:?}");
        // An enum lists its cases, and takes one by name.
        let options = props.iter().find(|p| p.name == "horizontal_options").unwrap();
        assert_eq!(options.choices.as_deref(), Some(&["Start", "Center", "End", "Fill"].map(String::from)[..]));
        rig.ask(HubToApp::SetProperty { request: 4, node: rig.agent.wire_id(label), name: "horizontal_options".into(), value: "Center".into() });
        let props: Vec<_> = rig.ask(HubToApp::GetProps { node: rig.agent.wire_id(label) }).into_iter().map(msg).collect();
        let [AppMsg::Props { props, .. }] = &props[..] else { panic!("{props:?}") };
        assert_eq!(props.iter().find(|p| p.name == "horizontal_options").map(|p| p.value.as_str()), Some("Center"));
        let refused: Vec<_> = rig.ask(HubToApp::SetProperty { request: 2, node: rig.agent.wire_id(label), name: "font_size".into(), value: "big".into() }).into_iter().map(msg).collect();
        assert!(matches!(&refused[..], [AppMsg::Error { request: Some(2), .. }]), "{refused:?}");

        // The snapshot is the canvas, the highlight box drawn in it.
        let shot: Vec<_> = rig.ask(HubToApp::Snapshot { request: 3 }).into_iter().map(msg).collect();
        assert_eq!(shot, vec![AppMsg::Snapshot { request: 3, png_base64: String::new(), width: 400, height: 300 }]);
    }
}
