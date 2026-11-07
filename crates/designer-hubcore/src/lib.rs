//! The designer hub's logic without IO: no sockets, no threads, no async runtime, no file system.
//! [`HubCore`] takes what apps and editors say and returns what to send where ([`Out`]).
//!
//! Two shells drive it: the native hub (designer-core: WebSocket server, MCP, running apps, phones on
//! a cable) and a hub inside a browser page (the editor's web build, apps in the same page). Project
//! files are reached through a [`CodeStore`]: the file system natively, the page's text in a browser.
//!
//! Parsing a project and writing a file are [`Out::Work`]: the shell runs it where blocking is fine
//! (a native thread pool, or inline in a page) and hands the result back to [`HubCore::done`].

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};

use designer_protocol::*;

mod codeset;
pub mod live;
pub use codeset::CodeCache;
pub mod page;

/// How many recent apps are kept.
pub const MAX_RECENT: usize = 10;

/// The platform name apps report (`AppInfo::platform`) for an app this computer can run itself.
pub const HOST_PLATFORM: &str = if cfg!(target_os = "macos") { "maccatalyst" } else if cfg!(windows) { "windows" } else { "linux" };

/// What changes whenever a file does (size, modified time, or a version counter).
pub type Stamp = (u64, u64);

/// Where the hub reads and writes an app's C# sources.
pub trait CodeStore: Send + Sync {
    /// Every `*.cs`, `*.tsx`, `*.jsx` and `*.rs` file under `dir` (without build output and hidden
    /// folders) with its stamp.
    fn list(&self, dir: &Path) -> Vec<(String, Stamp)>;
    /// A file's bytes, or why it could not be read.
    fn read(&self, uri: &str) -> Result<Vec<u8>, String>;
    /// Replaces a file's bytes (all or nothing), or says why it could not.
    fn write(&self, uri: &str, bytes: &[u8]) -> Result<(), String>;
    /// Whether a project folder is there (a recent app whose copy was deleted is not offered).
    fn dir_exists(&self, dir: &Path) -> bool;
    /// A write is on its way and the next one must wait for it (a page that has not taken the
    /// last edit yet): the hub holds the editors' writes until [`HubCore::code_arrived`].
    fn busy(&self) -> bool {
        false
    }
}

/// One connected app: who it is and what it last reported.
pub struct App {
    pub info: AppInfo,
    pub tree: Option<Tree>,
    pub screens: Option<Vec<Screen>>,
    /// The last properties the app sent for each control: their types spell a [`EditorMsg::SetCode`].
    pub props: HashMap<NodeId, Vec<Prop>>,
    /// The app's last hot reload step and when (seconds since the Unix epoch).
    pub hot_reload: Option<(String, u64)>,
    /// The last messages the app sent on its own (errors not tied to a request), newest last.
    pub notes: VecDeque<String>,
    /// Its live changes not in the code yet: every write to code is matched with the tree its code
    /// makes, [`live::Live::code_view`] of `tree`.
    pub live: live::Live,
}

/// What the hub sends, or work it needs done.
#[derive(Debug)]
pub enum Out {
    ToApp(AppId, HubToApp),
    /// To one editor, or to every editor (`None`).
    ToEditor(Option<u32>, HubToEditor),
    /// Blocking work (parsing a project, file IO): run it, then pass its result to [`HubCore::done`].
    Work(Work),
}

/// See [`Out::Work`].
pub struct Work(Box<dyn FnOnce() -> Done + Send>);

impl Work {
    pub fn run(self) -> Done {
        (self.0)()
    }
}

impl std::fmt::Debug for Work {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Work")
    }
}

/// The result of a [`Work`], for [`HubCore::done`].
pub struct Done(Finished);

enum Finished {
    Nothing,
    Broadcast(HubToEditor),
    Written { editor: u32, request: u32, node: NodeId, result: Result<(String, source::EditorRange, Written), Refusal>, change: Option<(AppId, live::Change)> },
    Undone { editor: u32, request: u32, written: Written, result: Result<(), Refusal> },
    /// The names the code gives controls of an app's tree.
    MadeBy { app: AppId, revision: u64, names: Vec<(NodeId, String)> },
}

/// The hub's state.
pub struct HubCore {
    pub apps: HashMap<AppId, App>,
    /// Apps that connected before, newest first (the native hub keeps them on disk and runs them).
    pub recent: Vec<RecentApp>,
    /// While apps on other devices may connect: their address.
    pub devices: Option<String>,
    /// Phones on a cable: (Android, iPhones and iPads).
    cables: (u32, u32),
    next_id: u32,
    /// The id each app had, by who it is (name, engine, pid, platform): an app that comes back
    /// (a phone app after the lock screen took its connection) gets its id again.
    known: HashMap<(String, String, u32, Option<String>), AppId>,
    editors: HashSet<u32>,
    /// Every code edit written for an editor, by (editor, request), so it can be taken back.
    written: HashMap<(u32, u32), Written>,
    /// The project sources parsed for [`HubToEditor::CodeSet`], shared with the work that parses them.
    code: Arc<Mutex<CodeCache>>,
    store: Arc<dyn CodeStore>,
    /// The project of apps that name none: a page's one snippet ([`page::PROJECT`]) in a page hub.
    pub page_project: Option<String>,
    /// Request ids of the hub's own messages to apps (live changes put back): from 2^31 up, apart
    /// from the editors' and the shell's.
    next_request: u32,
    /// Code writes and undos held while the store is busy, in the order the editors sent them.
    held: std::collections::VecDeque<(u32, EditorMsg)>,
}

impl HubCore {
    pub fn new(store: Arc<dyn CodeStore>) -> Self {
        Self { apps: HashMap::new(), recent: vec![], devices: None, cables: (0, 0), next_id: 0, known: HashMap::new(), editors: HashSet::new(), written: HashMap::new(), code: Arc::default(), store, page_project: None, next_request: 1 << 31, held: Default::default() }
    }

    pub fn store(&self) -> Arc<dyn CodeStore> {
        self.store.clone()
    }

    /// An app said hello: its id (its old one when it comes back) and what to send. `None`: another
    /// major protocol version, the connection is dropped.
    pub fn app_hello(&mut self, info: AppInfo) -> Option<(AppId, Vec<Out>)> {
        if !same_major(&info.protocol) {
            return None;
        }
        let key = (info.name.clone(), info.engine.clone(), info.pid, info.platform.clone());
        // Its old id when that is free (two apps alike at once keep apart).
        let id = match self.known.get(&key) {
            Some(&id) if !self.apps.contains_key(&id) => id,
            _ => {
                self.next_id += 1;
                self.next_id
            }
        };
        // ponytail: forgets every id past 256 apps seen; a per-entry age if that ever matters.
        if self.known.len() >= 256 {
            self.known.clear();
        }
        self.known.insert(key, id);
        // It becomes the newest recent app.
        if let Some(project) = info.project.clone() {
            let key = project_key(&project);
            let old_agent = self.recent.iter().find(|r| project_key(&r.project) == key).and_then(|r| r.agent.clone());
            self.recent.retain(|r| project_key(&r.project) != key);
            self.recent.insert(0, RecentApp { name: info.name.clone(), engine: info.engine.clone(), project, agent: info.agent.clone().or(old_agent), last_seen: now(), platform: info.platform.clone() });
            self.recent.truncate(MAX_RECENT);
        }
        self.apps.insert(id, App { info, tree: None, screens: None, props: HashMap::new(), hot_reload: None, notes: VecDeque::new(), live: live::Live::default() });
        Some((id, vec![Out::ToEditor(None, self.apps_msg())]))
    }

    /// App `id` says hello again as the same app (its agent attached again after a rebuild or after
    /// the page let it go: same name, engine and platform; a page app's pid and module say nothing):
    /// it stays, its tree and the changes made live and not written kept, so its next tree is a
    /// rebuild like a hot reload's. Its info is updated (editors hear it when it changed). `None` when
    /// it is another app.
    pub fn app_hello_again(&mut self, id: AppId, info: &AppInfo) -> Option<Vec<Out>> {
        let app = self.apps.get_mut(&id)?;
        let a = &app.info;
        if (&a.name, &a.engine, &a.platform) != (&info.name, &info.engine, &info.platform) || !same_major(&info.protocol) {
            return None;
        }
        if app.info == *info {
            return Some(vec![]);
        }
        app.info = info.clone();
        Some(vec![Out::ToEditor(None, self.apps_msg())])
    }

    /// A page's app says hello (a page hub has one app at a time): the same app again keeps its id,
    /// tree and live changes ([`Self::app_hello_again`]); another replaces `current`. The app now, or
    /// `None` when it speaks another protocol version.
    pub fn page_hello(&mut self, current: Option<AppId>, info: AppInfo) -> (Option<AppId>, Vec<Out>) {
        if let Some(old) = current
            && let Some(outs) = self.app_hello_again(old, &info)
        {
            return (Some(old), outs);
        }
        let mut outs = current.map(|old| self.app_closed(old)).unwrap_or_default();
        match self.app_hello(info) {
            Some((id, more)) => {
                outs.extend(more);
                (Some(id), outs)
            }
            None => (None, outs),
        }
    }

    /// A message of a connected app: kept, passed to every editor, and after a control's
    /// properties, which of them its code sets.
    pub fn app_msg(&mut self, id: AppId, msg: AppMsg) -> Vec<Out> {
        let Some(app) = self.apps.get_mut(&id) else { return vec![] };
        let tree = app.tree.clone();
        // An insert put back after a rebuild: editors move its marks to its new id.
        let mut replay = app.live.heard(&msg, tree.as_ref());
        // A new tree: live changes whose controls were built again go back onto them.
        if let AppMsg::Tree(new) = &msg {
            let mut next = self.next_request;
            let r = app.live.rebuilt(tree.as_ref(), new, &mut || {
                next += 1;
                next
            });
            self.next_request = next;
            replay = Some(r).filter(|r| !r.is_empty());
        }
        match &msg {
            AppMsg::Tree(tree) => app.tree = Some(tree.clone()),
            AppMsg::Screens { screens } => app.screens = Some(screens.clone()),
            AppMsg::Props { node, props } => {
                app.props.insert(*node, props.clone());
            }
            AppMsg::Error { request: None, message } => {
                if app.notes.len() == 20 {
                    app.notes.pop_front();
                }
                app.notes.push_back(message.clone());
            }
            AppMsg::HotReload { stage } => app.hot_reload = Some((stage.clone(), now())),
            _ => {}
        }
        let props_of = match &msg {
            AppMsg::Props { node, .. } => Some(*node),
            _ => None,
        };
        let new_tree = matches!(msg, AppMsg::Tree(_));
        // Editors hear where the changes went before the tree that shows them.
        let mut out = vec![];
        let mut sends = vec![];
        if let Some(r) = replay {
            sends = r.sends;
            out.push(Out::ToEditor(None, HubToEditor::Replayed { app: id, moved: r.moved, replaying: r.replaying, lost: r.lost }));
        }
        out.push(Out::ToEditor(None, HubToEditor::FromApp { app: id, msg }));
        out.extend(sends.into_iter().map(|m| Out::ToApp(id, m)));
        // A new tree: the names its code gives the controls helpers made, read off the app's code.
        if new_tree
            && let (Some(project), Some(tree)) = (app.info.project.clone().or_else(|| self.page_project.clone()), app.tree.as_ref().map(|t| app.live.code_view(t, None)))
        {
            let (cache, store) = (self.code.clone(), self.store.clone());
            out.push(Out::Work(Work(Box::new(move || {
                let names = cache.lock().unwrap().made_by(&*store, id, &project, &tree);
                Done(Finished::MadeBy { app: id, revision: tree.revision, names })
            }))));
        }
        // Right after a control's properties: which of them its code sets.
        if let Some(node) = props_of {
            // Matched with the tree the code makes (live changes not written left out).
            let (project, tree) = (app.info.project.clone().or_else(|| self.page_project.clone()), app.tree.as_ref().map(|t| app.live.code_view(t, None)));
            let (cache, store) = (self.code.clone(), self.store.clone());
            out.push(Out::Work(Work(Box::new(move || {
                let names = match (project, tree) {
                    (Some(project), Some(tree)) => cache.lock().unwrap().names(&*store, id, &project, &tree, node),
                    _ => None,
                };
                Done(Finished::Broadcast(HubToEditor::CodeSet { app: id, node, names }))
            }))));
        }
        out
    }

    /// An app's connection closed.
    pub fn app_closed(&mut self, id: AppId) -> Vec<Out> {
        self.apps.remove(&id);
        let cache = self.code.clone();
        vec![
            // Off the caller's lock: a parse may hold the cache.
            Out::Work(Work(Box::new(move || {
                cache.lock().unwrap().forget(id);
                Done(Finished::Nothing)
            }))),
            Out::ToEditor(None, self.apps_msg()),
        ]
    }

    /// An editor said hello: its id and what it gets first (the apps and everything they last
    /// reported). `None`: another major protocol version.
    pub fn editor_hello(&mut self, protocol: &str) -> Option<(u32, Vec<Out>)> {
        if !same_major(protocol) {
            return None;
        }
        self.next_id += 1;
        let id = self.next_id;
        self.editors.insert(id);
        // First the hub's version, then the apps.
        let mut first = vec![HubToEditor::Hub { version: env!("CARGO_PKG_VERSION").into() }, self.apps_msg()];
        if let Some(url) = &self.devices {
            first.push(HubToEditor::Devices { url: Some(url.clone()), refusal: None });
        }
        for (&app, a) in &self.apps {
            if let Some(screens) = &a.screens {
                first.push(HubToEditor::FromApp { app, msg: AppMsg::Screens { screens: screens.clone() } });
            }
            if let Some(tree) = &a.tree {
                first.push(HubToEditor::FromApp { app, msg: AppMsg::Tree(tree.clone()) });
            }
        }
        // Last, after the apps and what they reported: the phones on a cable.
        first.push(HubToEditor::Cables { android: self.cables.0, ios: self.cables.1 });
        Some((id, first.into_iter().map(|m| Out::ToEditor(Some(id), m)).collect()))
    }

    /// A message of an editor. Running apps and letting devices in are the shell's (nothing here).
    pub fn editor_msg(&mut self, editor: u32, msg: EditorMsg) -> Vec<Out> {
        // A page has not taken the last edit: this write waits its turn, made against the text the
        // page sends next (never against offsets that are stale by then).
        let write = matches!(msg, EditorMsg::SetCode { .. } | EditorMsg::InsertCode { .. } | EditorMsg::RemoveCode { .. } | EditorMsg::MoveCode { .. } | EditorMsg::UndoCode { .. });
        if write && (self.store.busy() || !self.held.is_empty()) {
            self.held.push_back((editor, msg));
            return vec![];
        }
        self.editor_msg_now(editor, msg)
    }

    fn editor_msg_now(&mut self, editor: u32, msg: EditorMsg) -> Vec<Out> {
        match msg {
            EditorMsg::ToApp { app, msg } if self.apps.contains_key(&app) => {
                self.to_app(app, &msg);
                vec![Out::ToApp(app, msg)]
            }
            EditorMsg::Restore { request, app, node, props, children } => {
                let Some(a) = self.apps.get_mut(&app) else { return vec![] };
                let Some((parent, type_name, index)) = a.tree.as_ref().and_then(|t| a.live.restore_place(t, node)) else { return vec![] };
                a.live.restoring(request, node);
                vec![Out::ToApp(app, HubToApp::Insert { request, parent, index: Some(index), type_name, props, children })]
            }
            EditorMsg::RemoveCode { request, app, node } => {
                let refuse = |key: &str, args: &[(&str, &dyn std::fmt::Display)]| vec![refused(editor, request, node, Refusal::new(key, args))];
                let Some(a) = self.apps.get(&app) else { return refuse("hub-not-connected", &[]) };
                let Some(tree) = a.tree.as_ref() else { return refuse("hub-no-tree", &[]) };
                if !a.live.in_code(tree, node) {
                    return refuse("hub-not-in-code-yet", &[("type", &type_of(tree, node))]);
                }
                let view = a.live.code_view(tree, None);
                let change = a.live.removes.iter().find(|r| r.node() == node).cloned().map(|r| (app, live::Change::Remove(r)));
                let project = a.info.project.clone().or_else(|| self.page_project.clone());
                let store = self.store.clone();
                vec![Out::Work(Work(Box::new(move || {
                    let result = match project {
                        None => Err(Refusal::new("hub-no-project", &[])),
                        Some(project) => write_edit(&*store, &project, |texts| designer_sources::remove_element(texts, &view, node)),
                    };
                    Done(Finished::Written { editor, request, node, result, change })
                })))]
            }
            EditorMsg::MoveCode { request, app, node, .. } => {
                let refuse = |key: &str, args: &[(&str, &dyn std::fmt::Display)]| vec![refused(editor, request, node, Refusal::new(key, args))];
                let Some(a) = self.apps.get(&app) else { return refuse("hub-not-connected", &[]) };
                let Some(tree) = a.tree.as_ref() else { return refuse("hub-no-tree", &[]) };
                if !a.live.in_code(tree, node) {
                    return refuse("hub-not-in-code-yet", &[("type", &type_of(tree, node))]);
                }
                let Some(parent) = tree.nodes.iter().find(|n| n.id == node).and_then(|n| n.parent) else { return refuse("src-not-in-tree", &[]) };
                if !a.live.in_code(tree, parent) {
                    return refuse("hub-parent-not-in-code", &[("type", &type_of(tree, parent))]);
                }
                // Where it goes: its place now among the children the code makes (the other live
                // changes as the code has them); matched with the code view, where it is still at
                // its place in the code.
                let Some(index) = live::Live::index_in(&a.live.code_view(tree, Some(node)), parent, node) else { return refuse("src-not-in-tree", &[]) };
                let view = a.live.code_view(tree, None);
                let change = a.live.moves.iter().find(|m| m.node == node).cloned().map(|m| (app, live::Change::Move(m)));
                let project = a.info.project.clone().or_else(|| self.page_project.clone());
                let store = self.store.clone();
                vec![Out::Work(Work(Box::new(move || {
                    let result = match project {
                        None => Err(Refusal::new("hub-no-project", &[])),
                        Some(project) => write_edit(&*store, &project, |texts| designer_sources::move_element(texts, &view, node, parent, index)),
                    };
                    Done(Finished::Written { editor, request, node, result, change })
                })))]
            }
            EditorMsg::SetCode { request, app, node, prop, value } => {
                let refuse = |key: &str, args: &[(&str, &dyn std::fmt::Display)]| vec![refused(editor, request, node, Refusal::new(key, args))];
                let Some(a) = self.apps.get(&app) else { return refuse("hub-not-connected", &[]) };
                // Matched with the tree the code makes, whatever else is changed live and not written.
                let tree = match a.tree.as_ref() {
                    Some(t) if !a.live.in_code(t, node) => return refuse("hub-not-in-code-yet", &[("type", &type_of(t, node))]),
                    Some(t) => Some(a.live.code_view(t, None)),
                    None => None,
                };
                let (project, props) = (a.info.project.clone().or_else(|| self.page_project.clone()), a.props.get(&node).cloned());
                let store = self.store.clone();
                let change = Some((app, live::Change::Prop(node, prop.clone())));
                vec![Out::Work(Work(Box::new(move || {
                    let result = set_code(&*store, project.as_deref(), tree, props, node, &prop, &value);
                    Done(Finished::Written { editor, request, node, result, change })
                })))]
            }
            EditorMsg::InsertCode { request, app, node, parent, type_name, defaults, spec, .. } => {
                let refuse = |key: &str, args: &[(&str, &dyn std::fmt::Display)]| vec![refused(editor, request, parent, Refusal::new(key, args))];
                let Some(a) = self.apps.get(&app) else { return refuse("hub-not-connected", &[]) };
                let Some(tree) = a.tree.as_ref() else { return refuse("hub-no-tree", &[]) };
                if !tree.nodes.iter().any(|n| n.id == node && n.parent == Some(parent)) {
                    return refuse("src-not-in-tree", &[]);
                }
                let Some(recorded) = a.live.inserts.get(&node) else { return refuse("hub-in-code-already", &[("type", &type_of(tree, node))]) };
                // Its parent was inserted live too: the code has no place for it yet.
                if !a.live.in_code(tree, parent) {
                    return refuse("hub-parent-not-in-code", &[("type", &type_of(tree, parent))]);
                }
                // Its place among the children the code makes, then the code view without it.
                let view = a.live.code_view(tree, Some(node));
                let Some(index) = live::Live::index_in(&view, parent, node) else { return refuse("src-not-in-tree", &[]) };
                let view = live::without(&view, node);
                // What to write: the editor's spec, else its values, else what it was inserted with.
                let spec = spec.unwrap_or_else(|| {
                    if defaults.is_empty() {
                        recorded.clone()
                    } else {
                        ControlSpec { type_name: type_name.clone(), props: defaults.iter().map(|(n, v)| SpecProp { name: n.clone(), value: v.clone(), type_name: String::new() }).collect(), children: recorded.children.clone() }
                    }
                });
                // A pasted copy: written as its source's code has it (see live::code_spec).
                let copy = a.live.copies.iter().rev().find(|c| live::same_copy(&c.spec, &spec)).cloned();
                let change = Some((app, live::Change::Insert(node, spec.clone())));
                // The root's values without a type are spelled by the live control's property types.
                let types: Vec<(String, String)> = a.props.get(&node).cloned().unwrap_or_default().into_iter().map(|p| (p.name, p.type_name)).collect();
                let project = a.info.project.clone().or_else(|| self.page_project.clone());
                let (store, cache) = (self.store.clone(), self.code.clone());
                vec![Out::Work(Work(Box::new(move || {
                    let result = match project {
                        None => Err(Refusal::new("hub-no-project", &[])),
                        Some(project) => {
                            let (spec, sources) = match &copy {
                                Some(c) => live::code_spec(c, &mut |tree, id| cache.lock().unwrap().names(&*store, app, &project, tree, id)),
                                None => (spec, HashMap::new()),
                            };
                            write_edit(&*store, &project, |texts| {
                                // A copy of a control a call makes (`swatch(CORAL, "coral")`): that call again.
                                if let (Some(c), Some(&source)) = (&copy, sources.get(&Vec::new()))
                                    && let Some(call) = designer_sources::copy_call(texts, &c.tree, source)
                                {
                                    return designer_sources::insert_text(texts, &view, parent, index as usize, &call);
                                }
                                // Each copied list in the form its original code writes it.
                                let ids: Vec<NodeId> = sources.values().copied().collect();
                                let by_id = copy.as_ref().map(|c| designer_sources::children_forms(texts, &c.tree, &ids)).unwrap_or_default();
                                let forms = sources.iter().filter_map(|(path, id)| by_id.get(id).map(|f| (path.clone(), f.clone()))).collect();
                                designer_sources::insert_spec(texts, &view, parent, index as usize, &spec, &types, &forms)
                            })
                        }
                    };
                    Done(Finished::Written { editor, request, node: parent, result, change })
                })))]
            }
            EditorMsg::UndoCode { request } => match self.written.remove(&(editor, request)) {
                Some(written) => {
                    let store = self.store.clone();
                    vec![Out::Work(Work(Box::new(move || {
                        let result = written.undo(&*store);
                        Done(Finished::Undone { editor, request, written, result })
                    })))]
                }
                None => vec![refused(editor, request, 0, Refusal::new("hub-nothing-written", &[]))],
            },
            _ => vec![],
        }
    }

    /// A message goes to app `app` (from an editor, or the shell's own MCP server): the app's record
    /// of live changes follows it. Every [`HubToApp`] a shell sends on its own goes through here.
    pub fn to_app(&mut self, app: AppId, msg: &HubToApp) {
        if let Some(a) = self.apps.get_mut(&app) {
            a.live.sent(msg, a.tree.as_ref());
        }
    }

    /// A client of the shell's own that writes to code like an editor (the MCP server): its writes
    /// are kept so it can take them back.
    pub fn add_editor(&mut self, editor: u32) {
        self.editors.insert(editor);
    }

    /// An editor's connection closed: its code edits can no longer be taken back.
    pub fn editor_closed(&mut self, editor: u32) {
        self.editors.remove(&editor);
        self.held.retain(|(e, _)| *e != editor);
        self.written.retain(|&(e, _), _| e != editor);
    }

    /// The result of a [`Work`].
    pub fn done(&mut self, done: Done) -> Vec<Out> {
        match done.0 {
            Finished::Nothing => vec![],
            Finished::Broadcast(msg) => vec![Out::ToEditor(None, msg)],
            Finished::MadeBy { app, revision, names } => {
                // Kept on the hub's copy of the tree (the MCP server reads it), only for that tree.
                let Some(tree) = self.apps.get_mut(&app).and_then(|a| a.tree.as_mut()).filter(|t| t.revision == revision) else { return vec![] };
                let mut told = vec![];
                for n in tree.nodes.iter_mut().filter(|n| n.made_by.is_none()) {
                    if let Some((_, name)) = names.iter().find(|(id, _)| *id == n.id) {
                        n.made_by = Some(name.clone());
                        told.push((n.id, name.clone()));
                    }
                }
                if told.is_empty() {
                    return vec![];
                }
                vec![Out::ToEditor(None, HubToEditor::MadeBy { app, revision, names: told })]
            }
            Finished::Written { editor, request, node, result, change } => match result {
                Ok((uri, range, mut written)) => {
                    if let Some((app, c)) = &change
                        && let Some(a) = self.apps.get_mut(app)
                    {
                        a.live.wrote(c);
                    }
                    written.change = change;
                    self.shift_written(&written.path, written.at, written.before.len(), written.after.len());
                    self.keep(editor, request, written);
                    vec![Out::ToEditor(Some(editor), HubToEditor::CodeEdited { request, node, uri, range })]
                }
                Err(refusal) => vec![refused(editor, request, node, refusal)],
            },
            Finished::Undone { editor, request, written, result } => match result {
                Ok(()) => {
                    self.shift_written(&written.path, written.at, written.after.len(), written.before.len());
                    if let Some((app, c)) = &written.change
                        && let Some(a) = self.apps.get_mut(app)
                    {
                        let tree = a.tree.clone();
                        a.live.undone(c, tree.as_ref());
                    }
                    vec![Out::ToEditor(Some(editor), HubToEditor::CodeUndone { request, uri: written.path })]
                }
                Err(refusal) => {
                    // Kept: the file may be put back by hand, then the undo works again.
                    self.keep(editor, request, written);
                    vec![refused(editor, request, 0, refusal)]
                }
            },
        }
    }

    /// Runs every [`Out::Work`] here and now and returns the messages only: for a shell where
    /// blocking is fine (a browser page; tests).
    /// The store took its last write (a page sent its code): the held writes go, one by one while
    /// the store is free, each against the code as it is now. Their work runs inline (a page hub).
    pub fn code_arrived(&mut self) -> Vec<Out> {
        let mut sent = vec![];
        while !self.store.busy()
            && let Some((editor, msg)) = self.held.pop_front()
        {
            let outs = self.editor_msg_now(editor, msg);
            sent.extend(self.run_inline(outs));
        }
        sent
    }

    pub fn run_inline(&mut self, outs: Vec<Out>) -> Vec<Out> {
        let mut sent = vec![];
        for out in outs {
            match out {
                Out::Work(work) => {
                    let more = self.done(work.run());
                    sent.extend(self.run_inline(more));
                }
                out => sent.push(out),
            }
        }
        sent
    }

    /// Keeps a code edit so `editor` (an editor's id, or any id of the shell's own, such as the
    /// MCP server's) can take it back by `request`.
    pub fn remember(&mut self, editor: u32, request: u32, written: Written) {
        self.written.insert((editor, request), written);
    }

    /// The file at `path` had `removed` bytes at `at` replaced by `added` ones (a write, or an undo):
    /// the kept writes after that span move with their text, so each can still be taken back in any
    /// order (Undo takes the last action first, whatever order the writes went in).
    fn shift_written(&mut self, path: &str, at: usize, removed: usize, added: usize) {
        for w in self.written.values_mut().filter(|w| w.path == path && w.at >= at + removed) {
            w.at = w.at + added - removed;
        }
    }

    /// [`HubCore::remember`] for an editor that may have closed while the work ran.
    fn keep(&mut self, editor: u32, request: u32, written: Written) {
        if self.editors.contains(&editor) {
            self.remember(editor, request, written);
        }
    }

    /// Takes a code edit out to undo it; [`HubCore::remember`] it again when the undo fails.
    pub fn take_written(&mut self, editor: u32, request: u32) -> Option<Written> {
        self.written.remove(&(editor, request))
    }

    /// A phone was plugged in or out: the count for that kind changes and every editor hears it.
    pub fn set_cables(&mut self, android: Option<u32>, ios: Option<u32>) -> Vec<Out> {
        let next = (android.unwrap_or(self.cables.0), ios.unwrap_or(self.cables.1));
        if next == self.cables {
            return vec![];
        }
        self.cables = next;
        vec![Out::ToEditor(None, HubToEditor::Cables { android: next.0, ios: next.1 })]
    }

    /// The connected apps, and the recent ones this computer can start that are not running.
    pub fn apps_msg(&self) -> HubToEditor {
        let mut apps: Vec<_> = self.apps.iter().map(|(&id, a)| ConnectedApp { id, info: a.info.clone() }).collect();
        apps.sort_by_key(|a| a.id);
        let live: Vec<String> = apps.iter().filter_map(|a| a.info.project.as_deref()).map(project_key).collect();
        // Not offered: the running ones, and those whose project folder is gone (a deleted copy).
        let recent = self
            .recent
            .iter()
            .filter(|r| !live.contains(&project_key(&r.project)) && self.store.dir_exists(project_dir(&r.project)) && runs_here(r))
            .cloned()
            .collect();
        HubToEditor::Apps { apps, recent }
    }

    /// The recent app of a project.
    pub fn recent_of(&self, project: &str) -> Option<&RecentApp> {
        let key = project_key(project);
        self.recent.iter().find(|r| project_key(&r.project) == key)
    }
}

/// A control's short type name, for a refusal.
fn type_of(tree: &Tree, id: NodeId) -> String {
    tree.nodes.iter().find(|n| n.id == id).map_or_else(String::new, |n| n.type_name.rsplit(['.', '+']).next().unwrap_or(&n.type_name).to_owned())
}

fn refused(editor: u32, request: u32, node: NodeId, refusal: Refusal) -> Out {
    Out::ToEditor(Some(editor), HubToEditor::CodeRefused { request, node, reason: refusal.to_string(), refusal: Some(refusal) })
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Whether a protocol version speaks this hub's major version.
pub fn same_major(protocol: &str) -> bool {
    protocol.split('.').next() == PROTOCOL_VERSION.split('.').next()
}

/// A recent app this computer can start: it last ran on this computer's platform (WPF apps are
/// Windows ones; an app from before platforms were reported waits until it connects again).
fn runs_here(r: &RecentApp) -> bool {
    match r.platform.as_deref() {
        Some(p) => p == HOST_PLATFORM || (HOST_PLATFORM == "maccatalyst" && p == "macos"),
        None => r.engine == "dotnet-wpf" && cfg!(windows),
    }
}

/// The folder of an app's project: the folder of its .csproj or Cargo.toml, or the path itself.
pub fn project_dir(project: &str) -> &Path {
    let p = Path::new(project);
    if p.extension().is_some_and(|e| e == "csproj") || p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("Cargo.toml")) { p.parent().unwrap_or(p) } else { p }
}

/// A project path compared as written by any agent: separators, `.` and `..` segments, case
/// (Windows), a trailing separator and a `.csproj` or `Cargo.toml` file name do not matter.
pub fn project_key(project: &str) -> String {
    let mut parts: Vec<&str> = vec![];
    for part in project.split(['/', '\\']) {
        match part {
            "" | "." if !parts.is_empty() => {}
            ".." if parts.len() > 1 => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    if parts.last().is_some_and(|l| l.to_ascii_lowercase().ends_with(".csproj") || l.eq_ignore_ascii_case("Cargo.toml")) {
        parts.pop();
    }
    let p = parts.join("\\");
    if cfg!(windows) { p.to_lowercase() } else { p }
}

/// A file's text without its UTF-8 BOM; `None` when it is not UTF-8 (such files are never written).
fn utf8(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)).ok()
}

/// One C# source file as read.
pub struct SourceFile {
    pub uri: String,
    pub text: String,
    pub bytes: Vec<u8>,
    pub bom: bool,
}

/// Every UTF-8 C# and TSX file of a project folder, read now.
// ponytail: read fresh on every edit; cache by stamp if large projects make it slow.
pub fn read_sources(store: &dyn CodeStore, dir: &Path) -> Vec<SourceFile> {
    let mut out = vec![];
    for (uri, _) in store.list(dir) {
        let Ok(bytes) = store.read(&uri) else { continue };
        let Some(text) = utf8(&bytes).map(str::to_owned) else { continue };
        out.push(SourceFile { uri, text, bom: bytes.starts_with(&[0xEF, 0xBB, 0xBF]), bytes });
    }
    out
}

/// One code edit as written: the file and the one span that changed, before and after.
pub struct Written {
    pub path: String,
    /// Where the changed span starts (bytes; the same in the file before and after).
    at: usize,
    before: Vec<u8>,
    after: Vec<u8>,
    /// The live change this write put into the code (an insert, a removal), for the app's record.
    change: Option<(AppId, live::Change)>,
}

impl Written {
    /// The span that differs between two versions of a file.
    fn diff(path: String, old: &[u8], new: &[u8]) -> Written {
        let mut at = old.iter().zip(new).take_while(|(a, b)| a == b).count();
        let mut tail = old[at..].iter().rev().zip(new[at..].iter().rev()).take_while(|(a, b)| a == b).count();
        // Text only taken out ("FontTextTitle" -> "FontText") leaves nothing to check where it goes
        // back: the byte before it joins the span. (Text only put in is checked by itself, and a
        // write right after it, such as another call appended, does not touch it.)
        if new.len() == at + tail {
            if at > 0 {
                at -= 1;
            } else if tail > 0 {
                tail -= 1;
            }
        }
        Written { path, at, before: old[at..old.len() - tail].to_vec(), after: new[at..new.len() - tail].to_vec(), change: None }
    }

    /// Puts the span back as it was, when the file still holds it as written at its place (moved by
    /// the hub's later writes and undos above it; an edit made by hand above it refuses the undo).
    pub fn undo(&self, store: &dyn CodeStore) -> Result<(), Refusal> {
        let mut bytes = store.read(&self.path).map_err(|e| Refusal::new("hub-read-failed", &[("file", &self.path), ("error", &e)]))?;
        if bytes.get(self.at..self.at + self.after.len()) != Some(self.after.as_slice()) {
            return Err(Refusal::new("hub-changed-since", &[("file", &self.path)]));
        }
        bytes.splice(self.at..self.at + self.after.len(), self.before.iter().copied());
        write(store, &self.path, &bytes)
    }
}

fn write(store: &dyn CodeStore, path: &str, bytes: &[u8]) -> Result<(), Refusal> {
    store.write(path, bytes).map_err(|e| Refusal::new("hub-write-failed", &[("file", &path), ("error", &e)]))
}

/// Writes `prop = value` of control `node` into the app's project sources. Reads every file fresh
/// and writes nothing unless one literal (or one new assignment) is the clear target.
pub fn set_code(store: &dyn CodeStore, project: Option<&str>, tree: Option<Tree>, props: Option<Vec<Prop>>, node: NodeId, prop: &str, value: &str) -> Result<(String, source::EditorRange, Written), Refusal> {
    let project = project.ok_or_else(|| Refusal::new("hub-no-project", &[]))?;
    let tree = tree.ok_or_else(|| Refusal::new("hub-no-tree", &[]))?;
    let p = props
        .and_then(|ps| ps.into_iter().find(|p| p.name == prop))
        .ok_or_else(|| Refusal::new("hub-type-unknown", &[("prop", &prop)]))?;
    let value = designer_sources::value_from_runtime(&p.type_name, &p.value, value)?;
    write_edit(store, project, |texts| designer_sources::set_property(texts, &tree, node, prop, &value))
}

/// Reads the project's sources fresh, makes one edit with `edit` and writes that one file, unless
/// it changed meanwhile.
fn write_edit(store: &dyn CodeStore, project: &str, edit: impl FnOnce(&[(String, String)]) -> Result<designer_sources::CodeEdit, Refusal>) -> Result<(String, source::EditorRange, Written), Refusal> {
    let dir = project_dir(project);
    let files = read_sources(store, dir);
    if files.is_empty() {
        return Err(Refusal::new("hub-no-sources", &[("folder", &dir.display())]));
    }
    let texts: Vec<(String, String)> = files.iter().map(|f| (f.uri.clone(), f.text.clone())).collect();
    let edit = edit(&texts)?;
    let file = files.iter().find(|f| f.uri == edit.uri).expect("edited file was read");
    // The file changed since it was read: the edit may no longer fit.
    if store.read(&file.uri).ok().as_deref() != Some(file.bytes.as_slice()) {
        return Err(Refusal::new("hub-changed-during", &[("file", &file.uri)]));
    }
    let mut bytes = if file.bom { vec![0xEF, 0xBB, 0xBF] } else { vec![] };
    bytes.extend_from_slice(edit.text.as_bytes());
    write(store, &file.uri, &bytes)?;
    Ok((edit.uri, edit.range, Written::diff(file.uri.clone(), &file.bytes, &bytes)))
}

#[cfg(test)]
mod tests;
