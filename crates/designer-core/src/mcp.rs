//! The hub's MCP server: lets an AI agent see and change running DrawnUI apps, and write property
//! changes into their source code. Streamable HTTP at `http://127.0.0.1:<port>/mcp`, loopback only.
//! For the apps it is one more editor: its requests go through the hub and wait for the answer.

use std::collections::HashMap;
use std::time::Duration;

use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerConfig};
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager};
use rmcp::{ErrorData, ServerHandler, schemars, tool, tool_handler, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::net::TcpListener;

use super::*;

const WAIT: Duration = Duration::from_secs(5);

/// Serves MCP on a bound listener until the process ends.
pub(crate) async fn serve(hub: Shared, listener: TcpListener) {
    let service: StreamableHttpService<Designer, LocalSessionManager> =
        StreamableHttpService::new(move || Ok(Designer::new(hub.clone())), Default::default(), StreamableHttpServerConfig::default());
    let router = axum::Router::new().nest_service("/mcp", service);
    if let Err(e) = axum::serve(listener, router).await {
        eprintln!("designer hub: MCP server stopped: {e}");
    }
}

#[derive(Clone)]
pub(crate) struct Designer {
    hub: Shared,
    tool_router: ToolRouter<Self>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct AppArg {
    /// The app's id, from list_apps.
    app: AppId,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct ScreenArg {
    /// The app's id, from list_apps.
    app: AppId,
    /// A route from list_screens.
    route: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct TreeArg {
    /// The app's id, from list_apps.
    app: AppId,
    /// Also list the controls the library makes for its own parts (a button's label, shell
    /// chrome). They are not in any code. Default false.
    #[serde(default)]
    include_internal: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct NodeArg {
    /// The app's id, from list_apps.
    app: AppId,
    /// A control's id, from get_tree.
    node: NodeId,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct SelectArg {
    /// The app's id, from list_apps.
    app: AppId,
    /// A control's id, from get_tree; omit to clear the highlight.
    node: Option<NodeId>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct PickArg {
    /// The app's id, from list_apps.
    app: AppId,
    /// Canvas x in pixels (the coordinate space of get_tree's rect).
    x: f32,
    /// Canvas y in pixels.
    y: f32,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct PropertyArg {
    /// The app's id, from list_apps.
    app: AppId,
    /// A control's id, from get_tree.
    node: NodeId,
    /// A property name as get_properties lists it, such as FontSize or TextColor.
    name: String,
    /// The new value as text, invariant culture: 18, true, Hello, Center, #FF0000 (#RGB, #RRGGBB,
    /// #AARRGGBB) for colors, "8" / "8,4" / "8,4,8,4" (left,top,right,bottom) for thickness.
    value: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct PropValue {
    /// A property name as get_properties lists it.
    name: String,
    /// Its value as text, as for set_property.
    value: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct InsertArg {
    /// The app's id, from list_apps.
    app: AppId,
    /// The control that gets the new one among its children, from get_tree.
    parent: NodeId,
    /// Its place among the parent's children (the app's own, without library parts); omit for last.
    index: Option<u32>,
    /// The full type name, as get_controls lists it, such as DrawnUi.Draw.SkiaLabel. Not needed with copy.
    r#type: Option<String>,
    /// Properties set right after its starting values, in this order.
    #[serde(default)]
    props: Vec<PropValue>,
    /// A copy as the copy tool returns it ({typeName, props, children}): pasted whole, with what
    /// is inside it; type and props are then not used.
    copy: Option<Value>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct MoveArg {
    /// The app's id, from list_apps.
    app: AppId,
    /// The control to move, from get_tree.
    node: NodeId,
    /// The control that gets it among its children (its own parent to reorder), from get_tree.
    parent: NodeId,
    /// Its place among the parent's children (the app's own, without library parts), counted without
    /// the control itself: the index it has after the move.
    index: u32,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct UndoArg {
    /// The `undo` id write_property, write_insert, write_remove or write_move returned with code.edited.
    undo: u32,
}

/// The editor id under which the MCP server's code edits are kept (editors count up from 1).
const MCP_EDITOR: u32 = u32::MAX;

fn json_result(value: Value) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::success(vec![ContentBlock::json(value)?]))
}

/// A failure the agent should read and act on (not a protocol error).
fn failed(reason: impl Into<String>) -> Result<CallToolResult, ErrorData> {
    Ok(CallToolResult::error(vec![ContentBlock::text(reason.into())]))
}

/// `AppMsg::Error` for this request, as the failure text.
fn app_error(msg: &AppMsg) -> Option<String> {
    match msg {
        AppMsg::Error { message, .. } => Some(message.clone()),
        _ => None,
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
struct OnArg {
    /// true: apps on other devices may connect; false: no new ones.
    on: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct RunArg {
    /// The project of a recent app, as list_apps shows it. Omit for the most recent app.
    project: Option<String>,
}

impl Designer {
    /// Waits for the run of `project` to connect (the new app's id and pid) or fail (why).
    async fn wait_run(&self, mut events: tokio::sync::broadcast::Receiver<run::RunEvent>, project: &str) -> Result<CallToolResult, ErrorData> {
        let key = run::project_key(project);
        let wait = async {
            loop {
                match events.recv().await {
                    Ok(e) if e.project == key && e.state == RunState::Running => break Ok(e.app),
                    Ok(e) if e.project == key && e.state == RunState::Failed => break Err(e.message),
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break Err("the hub stopped".to_string()),
                    _ => {}
                }
            }
        };
        let timeout = self.hub.lock().unwrap().runs.run_timeout();
        match tokio::time::timeout(timeout + Duration::from_secs(5), wait).await {
            Ok(Ok(Some(app))) => {
                let pid = self.hub.lock().unwrap().core.apps.get(&app).map(|a| a.info.pid);
                json_result(json!({"app": app, "pid": pid, "project": project}))
            }
            Ok(Ok(None)) => failed("the app connected but is gone again"),
            Ok(Err(reason)) => failed(reason),
            Err(_) => failed(format!("{project} did not connect within {} s", timeout.as_secs())),
        }
    }

    fn new(hub: Shared) -> Self {
        // Its writes to code are kept like an editor's, so undo_write can take them back.
        hub.lock().unwrap().core.add_editor(MCP_EDITOR);
        Self { hub, tool_router: Self::tool_router() }
    }

    fn send(&self, app: AppId, msg: HubToApp) -> Result<(), String> {
        let mut h = self.hub.lock().unwrap();
        h.core.to_app(app, &msg);
        h.send_to_app(app, &msg)
    }

    /// A write to code (or its undo) through the hub, as an editor's: matched with the tree the
    /// app's code makes (the hub's record of live changes not written). The request and the answer.
    async fn code(&self, make: impl FnOnce(u32) -> EditorMsg) -> (u32, HubToEditor) {
        let (request, mut outs) = {
            let mut h = self.hub.lock().unwrap();
            let request = h.request_id();
            (request, h.core.editor_msg(MCP_EDITOR, make(request)))
        };
        loop {
            let mut next = vec![];
            for out in outs {
                match out {
                    Out::ToEditor(Some(MCP_EDITOR), m) => return (request, m),
                    Out::ToApp(app, m) => {
                        let _ = self.hub.lock().unwrap().send_to_app(app, &m);
                    }
                    Out::Work(work) => {
                        let done = tokio::task::spawn_blocking(move || work.run()).await.expect("code work");
                        next.extend(self.hub.lock().unwrap().core.done(done));
                    }
                    _ => {}
                }
            }
            if next.is_empty() {
                let refusal = Refusal::new("hub-failed", &[("error", &"no answer")]);
                return (request, HubToEditor::CodeRefused { request, node: 0, reason: refusal.to_string(), refusal: Some(refusal) });
            }
            outs = next;
        }
    }

    /// A code answer as a tool's result: `edited` (file, range, undo id) or `refused` (why, key).
    fn code_json(request: u32, answer: HubToEditor) -> Value {
        match answer {
            HubToEditor::CodeEdited { uri, range, .. } => json!({"edited": {"uri": uri, "range": range, "undo": request}}),
            HubToEditor::CodeRefused { reason, refusal, .. } => json!({"refused": reason, "key": refusal.map(|r| r.key)}),
            other => json!({"refused": format!("{other:?}")}),
        }
    }

    /// A control's type and parent in the app's last tree.
    fn node_info(&self, app: AppId, node: NodeId) -> Option<(String, Option<NodeId>)> {
        let h = self.hub.lock().unwrap();
        h.core.apps.get(&app)?.tree.as_ref()?.nodes.iter().find(|n| n.id == node).map(|n| (n.type_name.clone(), n.parent))
    }

    /// Asks for the tree until it holds `node` (`present`) or no longer holds it; false after `WAIT`.
    async fn tree_with(&self, app: AppId, node: NodeId, present: bool) -> bool {
        ask(&self.hub, app, |_| vec![HubToApp::GetTree], move |_, m| matches!(m, AppMsg::Tree(t) if t.nodes.iter().any(|n| n.id == node) == present), WAIT).await.is_ok()
    }


    async fn fresh_props(&self, app: AppId, node: NodeId) -> Result<Vec<Prop>, String> {
        let msg = ask(&self.hub, app, |_| vec![HubToApp::GetProps { node }], move |_, m| matches!(m, AppMsg::Props { node: n, .. } if *n == node), WAIT).await?;
        match msg {
            AppMsg::Props { props, .. } => Ok(props),
            _ => unreachable!(),
        }
    }

    /// Sets a property on the live control, then asks for its properties. The app handles messages
    /// in order: a failure arrives as an error before the properties, a success shows in them.
    async fn set_live(&self, app: AppId, node: NodeId, name: &str, value: &str) -> Result<Option<Prop>, String> {
        let (n, v) = (name.to_string(), value.to_string());
        let msg = ask(
            &self.hub,
            app,
            move |request| vec![HubToApp::SetProperty { request, node, name: n, value: v }, HubToApp::GetProps { node }],
            move |request, m| matches!(m, AppMsg::Error { request: Some(r), .. } if *r == request) || matches!(m, AppMsg::Props { node: x, .. } if *x == node),
            WAIT,
        );
        match msg.await? {
            AppMsg::Props { props, .. } => Ok(props.into_iter().find(|p| p.name == name)),
            m => Err(app_error(&m).unwrap_or_default()),
        }
    }
}

#[tool_router]
impl Designer {
    #[tool(description = "List the DrawnUI apps connected to the designer hub: id (use it in every other tool), name, engine (such as dotnet-wpf), platform (windows, android, ios, maccatalyst...), pid, the project they were built from, and hotReload (the last hot reload step, applied or reloaded, with its time: check it after editing code), and notes (the last messages the app sent on its own, such as errors its designer agent caught). Then the recent apps this computer can start (its own platform: Windows apps on Windows, Mac Catalyst on a Mac) that are not running now, with connected: false and no id: start one with run_app.")]
    async fn list_apps(&self) -> Result<CallToolResult, ErrorData> {
        let h = self.hub.lock().unwrap();
        let mut apps: Vec<_> = h
            .core
            .apps
            .iter()
            .map(|(&id, a)| json!({"id": id, "name": a.info.name, "engine": a.info.engine, "platform": a.info.platform, "pid": a.info.pid, "project": a.info.project, "connected": true, "hotReload": a.hot_reload.as_ref().map(|(stage, at)| json!({"stage": stage, "at": at})), "notes": a.notes}))
            .collect();
        apps.sort_by_key(|a| a["id"].as_u64());
        if let HubToEditor::Apps { recent, .. } = h.core.apps_msg() {
            apps.extend(recent.into_iter().map(|r| json!({"name": r.name, "engine": r.engine, "platform": r.platform, "project": r.project, "lastSeen": r.last_seen, "connected": false})));
        }
        json_result(json!(apps))
    }

    #[tool(description = "Start a recent app (in watch mode, with the designer agent) and wait until it connects, up to 3 minutes. project: as list_apps shows it; omitted, the most recent app. Returns the app's id, or the build errors.")]
    async fn run_app(&self, Parameters(RunArg { project }): Parameters<RunArg>) -> Result<CallToolResult, ErrorData> {
        let (project, events) = {
            let h = self.hub.lock().unwrap();
            let Some(project) = project.or_else(|| h.core.recent.first().map(|r| r.project.clone())) else {
                return failed("no recent app: start an app with the designer agent once");
            };
            (project, h.runs.events.subscribe())
        };
        if let Err(e) = run::run_app(&self.hub, &project) {
            return failed(e);
        }
        self.wait_run(events, &project).await
    }

    #[tool(description = "Stop a connected app (the whole process tree when the designer started it) and run it again; waits until it connects again and returns its new id, or the build errors.")]
    async fn restart_app(&self, Parameters(AppArg { app }): Parameters<AppArg>) -> Result<CallToolResult, ErrorData> {
        let events = self.hub.lock().unwrap().runs.events.subscribe();
        match run::restart_app(&self.hub, app) {
            Ok(project) => self.wait_run(events, &project).await,
            Err(e) => failed(e),
        }
    }

    #[tool(description = "Let apps on other devices (a phone, another computer on the same network) connect to the designer, or stop it. Returns the address with its secret token: an app on a phone is built with it (MSBuild property DrawnUiDesignerUrl, see the designer docs). Apps already connected stay connected when it is turned off.")]
    async fn allow_devices(&self, Parameters(OnArg { on }): Parameters<OnArg>) -> Result<CallToolResult, ErrorData> {
        match crate::allow_devices(&self.hub, on).await {
            (_, Some(refusal)) => failed(refusal),
            (url, None) => json_result(json!({"allowed": url.is_some(), "url": url})),
        }
    }

    #[tool(description = "List the screens (shell routes) an app can open, with their titles. Open one with open_screen.")]
    async fn list_screens(&self, Parameters(AppArg { app }): Parameters<AppArg>) -> Result<CallToolResult, ErrorData> {
        let h = self.hub.lock().unwrap();
        match h.core.apps.get(&app) {
            Some(a) => json_result(json!(a.screens.clone().unwrap_or_default())),
            None => failed(format!("no app with id {app} is connected (see list_apps)")),
        }
    }

    #[tool(description = "Navigate an app to a screen (a route from list_screens). The page transition takes about a second; call get_tree after it.")]
    async fn open_screen(&self, Parameters(ScreenArg { app, route }): Parameters<ScreenArg>) -> Result<CallToolResult, ErrorData> {
        match self.send(app, HubToApp::OpenScreen { route: route.clone() }) {
            Ok(()) => json_result(json!({"opening": route})),
            Err(e) => failed(e),
        }
    }

    #[tool(description = "The app's control tree as it is drawn now, parents before children: id, parent, type (short class name), field (the app field holding the control, from .Assign(out _field)), tag, rect [x, y, width, height] in canvas pixels, visible, templated (true on a layout that makes its children from items: nothing is inserted or moved into it), item (for a recycled cell of a templated list: the item index it shows; the tree is taken again when a scroll comes to rest), and source (file:line of the code that creates the control, when found; every cell of one template has the same). Library-made parts are left out unless include_internal is true.")]
    async fn get_tree(&self, Parameters(TreeArg { app, include_internal }): Parameters<TreeArg>) -> Result<CallToolResult, ErrorData> {
        let tree = match ask(&self.hub, app, |_| vec![HubToApp::GetTree], |_, m| matches!(m, AppMsg::Tree(_)), WAIT).await {
            Ok(AppMsg::Tree(t)) => t,
            Ok(_) => unreachable!(),
            Err(e) => match self.hub.lock().unwrap().core.apps.get(&app).and_then(|a| a.tree.clone()) {
                Some(t) => t,
                None => return failed(e),
            },
        };
        let project = self.hub.lock().unwrap().core.apps.get(&app).and_then(|a| a.info.project.clone());
        let t = tree.clone();
        let sources = tokio::task::spawn_blocking(move || project.map(|p| locate(&p, &t)).unwrap_or_default()).await.unwrap_or_default();
        let short = |t: &str| t.rsplit(['.', '+']).next().unwrap_or(t).to_string();
        let nodes: Vec<Value> = tree
            .nodes
            .iter()
            .filter(|n| include_internal || !n.internal)
            .map(|n| {
                let mut o = json!({"id": n.id, "parent": n.parent, "type": short(&n.type_name), "rect": n.rect, "visible": n.visible});
                let m = o.as_object_mut().unwrap();
                if let Some(f) = &n.field {
                    m.insert("field".into(), json!(f));
                }
                if let Some(t) = &n.tag {
                    m.insert("tag".into(), json!(t));
                }
                if let Some(h) = &n.host {
                    m.insert("host".into(), json!(h));
                }
                if n.internal {
                    m.insert("internal".into(), json!(true));
                }
                if n.templated {
                    m.insert("templated".into(), json!(true));
                }
                // A recycled cell: the item it showed when the tree was taken.
                if let Some(i) = n.template_index {
                    m.insert("item".into(), json!(i));
                }
                // The app's own location hint (React dev builds) wins over the matcher's.
                if let Some(s) = n.source.as_ref().or_else(|| sources.get(&n.id)) {
                    m.insert("source".into(), json!(s));
                }
                o
            })
            .collect();
        json_result(json!({"width": tree.width, "height": tree.height, "nodes": nodes}))
    }

    #[tool(description = "The bindable properties of one control: name, .NET type, current value as text, and whether it was set (not the default).")]
    async fn get_properties(&self, Parameters(NodeArg { app, node }): Parameters<NodeArg>) -> Result<CallToolResult, ErrorData> {
        match self.fresh_props(app, node).await {
            Ok(props) => json_result(json!(props)),
            Err(e) => failed(e),
        }
    }

    #[tool(description = "Highlight a control in the running app (a box drawn around it), or clear the highlight when node is omitted.")]
    async fn select(&self, Parameters(SelectArg { app, node }): Parameters<SelectArg>) -> Result<CallToolResult, ErrorData> {
        match self.send(app, HubToApp::Highlight { node, color: None }) {
            Ok(()) => json_result(json!({"highlighted": node})),
            Err(e) => failed(e),
        }
    }

    #[tool(description = "The topmost control at a canvas point (pixels, same space as get_tree's rect). Returns its id, or null.")]
    async fn pick(&self, Parameters(PickArg { app, x, y }): Parameters<PickArg>) -> Result<CallToolResult, ErrorData> {
        let msg = ask(
            &self.hub,
            app,
            |request| vec![HubToApp::Pick { request, x, y }],
            |request, m| matches!(m, AppMsg::Picked { request: r, .. } | AppMsg::Error { request: Some(r), .. } if *r == request),
            WAIT,
        );
        match msg.await {
            Ok(AppMsg::Picked { node, .. }) => json_result(json!({"node": node})),
            Ok(m) => failed(app_error(&m).unwrap_or_default()),
            Err(e) => failed(e),
        }
    }

    #[tool(description = "Change a property on the live control only, to try a value. Lost when the app restarts; use write_property to also change the code. Returns the property as the app reports it after the change.")]
    async fn set_property(&self, Parameters(PropertyArg { app, node, name, value }): Parameters<PropertyArg>) -> Result<CallToolResult, ErrorData> {
        match self.set_live(app, node, &name, &value).await {
            Ok(prop) => json_result(json!({"live": "set", "property": prop})),
            Err(e) => failed(format!("the app refused {name} = {value}: {e}")),
        }
    }

    #[tool(description = "Change a property on the live control AND write it into the C# code that creates the control (the literal it is set to, the helper call argument that feeds it, or a new assignment), preserving the file's formatting. The code is matched with what it makes, whatever else is changed live and not written yet (controls inserted or removed live). Returns code.edited with the file, range and an undo id (for undo_write), or code.refused with the reason and its key (an expression the designer will not rewrite, an unsupported type, no matching code, a control not in the code yet). Nothing is written when the target is ambiguous.")]
    async fn write_property(&self, Parameters(PropertyArg { app, node, name, value }): Parameters<PropertyArg>) -> Result<CallToolResult, ErrorData> {
        if let Err(e) = self.set_live(app, node, &name, &value).await {
            return failed(format!("the app refused {name} = {value}: {e}"));
        }
        let (request, answer) = self.code(|request| EditorMsg::SetCode { request, app, node, prop: name, value }).await;
        json_result(json!({"live": "set", "code": Self::code_json(request, answer)}))
    }

    #[tool(description = "Take back a code edit of write_property, write_insert, write_remove or write_move: the edited text goes back to what it was, if the file still holds it as written there. undo: the id in the tool's code.edited. The running app follows through hot reload; set_property puts a live value back at once, insert puts a removed control back, remove takes an inserted one out.")]
    async fn undo_write(&self, Parameters(UndoArg { undo }): Parameters<UndoArg>) -> Result<CallToolResult, ErrorData> {
        match self.code(|_| EditorMsg::UndoCode { request: undo }).await.1 {
            HubToEditor::CodeUndone { uri, .. } => json_result(json!({"undone": {"uri": uri}})),
            HubToEditor::CodeRefused { reason, .. } => failed(reason),
            other => failed(format!("{other:?}")),
        }
    }

    #[tool(description = "Insert a new control into the running app only (not the code): a control of type (a full type name, as get_controls lists it) added to parent's children at index (omit: last), with its type's starting values and then props; or a copy from the copy tool, with everything inside it. Returns its id once the app's tree has it. It stays live until write_insert puts it into the code; remove takes it out. A hot reload rebuilds the screen from the code: a control not written by then is gone.")]
    async fn insert(&self, Parameters(InsertArg { app, parent, index, r#type, props, copy }): Parameters<InsertArg>) -> Result<CallToolResult, ErrorData> {
        let (r#type, props, children) = match copy {
            Some(v) => match serde_json::from_value::<ControlSpec>(v) {
                Ok(c) => (c.type_name, c.props.into_iter().map(|p| (p.name, p.value)).collect(), c.children),
                Err(e) => return failed(format!("copy is not a copy from the copy tool: {e}")),
            },
            None => match r#type {
                Some(t) => (t, props.into_iter().map(|p| (p.name, p.value)).collect::<Vec<(String, String)>>(), vec![]),
                None => return failed("give type, or copy"),
            },
        };
        let msg = ask(
            &self.hub,
            app,
            move |request| vec![HubToApp::Insert { request, parent, index, type_name: r#type, props, children }],
            |request, m| matches!(m, AppMsg::Inserted { request: r, .. } | AppMsg::Error { request: Some(r), .. } if *r == request),
            WAIT,
        );
        match msg.await {
            Ok(AppMsg::Inserted { node: Some(node), parent, index, .. }) => {
                self.tree_with(app, node, true).await;
                json_result(json!({"node": node, "parent": parent, "index": index}))
            }
            Ok(AppMsg::Inserted { node: None, .. }) => failed("the app did not insert it"),
            Ok(m) => failed(app_error(&m).unwrap_or_default()),
            Err(e) => failed(e),
        }
    }

    #[tool(description = "Read a control with everything inside it, as the app has it: its type, its set properties with their types, and its own children (not the library's parts, not cells made from data), recursively. Pass it as insert's copy to paste it.")]
    async fn copy(&self, Parameters(NodeArg { app, node }): Parameters<NodeArg>) -> Result<CallToolResult, ErrorData> {
        let msg = ask(
            &self.hub,
            app,
            |request| vec![HubToApp::Copy { request, node }],
            |request, m| matches!(m, AppMsg::Copied { request: r, .. } | AppMsg::Error { request: Some(r), .. } if *r == request),
            WAIT,
        );
        match msg.await {
            Ok(AppMsg::Copied { spec: Some(spec), .. }) => json_result(json!(spec)),
            Ok(AppMsg::Copied { spec: None, .. }) => failed(format!("no control {node} in app {app}")),
            Ok(m) => failed(app_error(&m).unwrap_or_default()),
            Err(e) => failed(format!("{e}: this app's agent may not support copy yet")),
        }
    }

    #[tool(description = "The control types the app can create (for insert): full type name, group, whether the app declares it (own), whether it holds children (container), and the values a new one starts with.")]
    async fn get_controls(&self, Parameters(AppArg { app }): Parameters<AppArg>) -> Result<CallToolResult, ErrorData> {
        match ask(&self.hub, app, |_| vec![HubToApp::GetControls], |_, m| matches!(m, AppMsg::Controls { .. }), WAIT).await {
            Ok(AppMsg::Controls { controls }) => json_result(json!(controls)),
            Ok(_) => unreachable!(),
            Err(e) => failed(e),
        }
    }

    #[tool(description = "Take a control out of the running app only (not the code). For a control the code makes, write_remove then takes it out of the code too; for one inserted and not written, this is all (it was never in the code). Returns once the app's tree no longer has it.")]
    async fn remove(&self, Parameters(NodeArg { app, node }): Parameters<NodeArg>) -> Result<CallToolResult, ErrorData> {
        if self.node_info(app, node).is_none() {
            return failed(format!("no control {node} in app {app}'s tree (see get_tree)"));
        }
        let request = self.hub.lock().unwrap().request_id();
        if let Err(e) = self.send(app, HubToApp::Remove { request, node }) {
            return failed(e);
        }
        if self.tree_with(app, node, false).await { json_result(json!({"removed": node})) } else { failed(format!("the app still shows control {node}")) }
    }

    #[tool(description = "Write a control inserted live (insert, or placed in the designer) into the C# code that makes its parent's children, as new T { its starting values and props } at its place among the children the code makes (other live changes not written are taken into account). Returns code.edited with an undo id, or code.refused with the reason and its key: a parent whose children come from a loop or a template, a parent not in the code yet, a control already in the code.")]
    async fn write_insert(&self, Parameters(NodeArg { app, node }): Parameters<NodeArg>) -> Result<CallToolResult, ErrorData> {
        let Some((type_name, Some(parent))) = self.node_info(app, node) else { return failed(format!("no control {node} with a parent in app {app}'s tree (see get_tree)")) };
        // Its property types spell the values.
        if let Err(e) = self.fresh_props(app, node).await {
            return failed(e);
        }
        let (request, answer) = self.code(|request| EditorMsg::InsertCode { request, app, node, parent, index: 0, type_name, defaults: vec![], spec: None }).await;
        json_result(json!({"code": Self::code_json(request, answer)}))
    }

    #[tool(description = "Take a control out of the C# code that makes it: one removed live with remove (its place in the code is remembered), or one still shown. Returns code.edited with an undo id, or code.refused with the reason and its key.")]
    async fn write_remove(&self, Parameters(NodeArg { app, node }): Parameters<NodeArg>) -> Result<CallToolResult, ErrorData> {
        let (request, answer) = self.code(|request| EditorMsg::RemoveCode { request, app, node }).await;
        json_result(json!({"code": Self::code_json(request, answer)}))
    }

    #[tool(name = "move", description = "Move a control, with everything inside it, in the running app only (not the code): into parent's children at index (counted without the control: the index it has after the move); its own parent reorders it. Not into itself or what it holds. Returns where it went once the app answers. write_move then moves it in the code; a hot reload before that puts it back where the code has it, then moves it again.")]
    async fn r#move(&self, Parameters(MoveArg { app, node, parent, index }): Parameters<MoveArg>) -> Result<CallToolResult, ErrorData> {
        let msg = ask(
            &self.hub,
            app,
            move |request| vec![HubToApp::Move { request, node, parent, index }],
            |request, m| matches!(m, AppMsg::Moved { request: r, .. } | AppMsg::Error { request: Some(r), .. } if *r == request),
            WAIT,
        );
        match msg.await {
            Ok(AppMsg::Moved { node: Some(node), parent, index, .. }) => {
                self.tree_with(app, node, true).await;
                json_result(json!({"node": node, "parent": parent, "index": index}))
            }
            Ok(AppMsg::Moved { node: None, .. }) => failed("the app did not move it"),
            Ok(m) => failed(app_error(&m).unwrap_or_default()),
            Err(e) => failed(format!("{e}: this app's agent may not support move yet")),
        }
    }

    #[tool(description = "Move a control in the C# code that makes it to where move put it live (its place in the code is remembered). Returns code.edited with an undo id, or code.refused with the reason and its key: a parent whose children come from a loop or a template, a control in another file, a parent not in the code yet.")]
    async fn write_move(&self, Parameters(NodeArg { app, node }): Parameters<NodeArg>) -> Result<CallToolResult, ErrorData> {
        let Some((_, Some(parent))) = self.node_info(app, node) else { return failed(format!("no control {node} with a parent in app {app}'s tree (see get_tree)")) };
        let (request, answer) = self.code(|request| EditorMsg::MoveCode { request, app, node, parent, index: 0 }).await;
        json_result(json!({"code": Self::code_json(request, answer)}))
    }

    #[tool(description = "Bring the app's window to the front, restored if it is minimized. Windows may keep the keyboard with the app the user works in; the window still comes back from the taskbar.")]
    async fn focus(&self, Parameters(AppArg { app }): Parameters<AppArg>) -> Result<CallToolResult, ErrorData> {
        match self.send(app, HubToApp::Focus) {
            Ok(()) => json_result(json!({"focus": "asked"})),
            Err(e) => failed(e),
        }
    }

    #[tool(description = "A PNG screenshot of the app's canvas as drawn now.")]
    async fn snapshot(&self, Parameters(AppArg { app }): Parameters<AppArg>) -> Result<CallToolResult, ErrorData> {
        let msg = ask(
            &self.hub,
            app,
            |request| vec![HubToApp::Snapshot { request }],
            |request, m| matches!(m, AppMsg::Snapshot { request: r, .. } | AppMsg::Error { request: Some(r), .. } if *r == request),
            WAIT,
        );
        match msg.await {
            Ok(AppMsg::Snapshot { png_base64, .. }) => Ok(CallToolResult::success(vec![ContentBlock::image(png_base64, "image/png")])),
            Ok(m) => failed(app_error(&m).unwrap_or_default()),
            Err(e) => failed(format!("{e}: this app's agent may not support snapshots yet")),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Designer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "DrawnUI Designer: inspect and change running DrawnUI apps. Start with list_apps, then get_tree for an app. \
             Use set_property to try a value live and write_property to change the app's C# code as well. \
             insert, remove and move change the controls live; write_insert, write_remove and write_move put those changes into the code; \
             undo_write takes any write back.",
        )
    }
}

/// `file:line` of the code that creates each matched control of `tree`, from the project sources.
fn locate(project: &str, tree: &Tree) -> HashMap<NodeId, String> {
    let files = designer_hubcore::read_sources(&FsStore, project_dir(project));
    let texts: Vec<(String, String)> = files.into_iter().map(|f| (f.uri, f.text)).collect();
    let matches = designer_sources::match_runtime(&texts, tree);
    let mut lines: HashMap<&str, HashMap<String, u32>> = HashMap::new();
    let mut out = HashMap::new();
    for m in &matches {
        let keys = lines.entry(m.uri.as_str()).or_insert_with(|| {
            let text = &texts.iter().find(|(u, _)| *u == m.uri).expect("matched file").1;
            designer_sources::read_csharp(&m.uri, text).nodes.into_iter().map(|n| (n.key, n.range.start.line + 1)).collect()
        });
        if let Some(line) = keys.get(&m.key) {
            out.insert(m.node, format!("{}:{line}", m.uri));
        }
    }
    out
}
