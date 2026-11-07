//! Messages between the three parts of DrawnUI Designer, as JSON text frames over one WebSocket:
//! - an **app agent** inside the running app (DrawnUi.Net, DrawnUi.Rust, DrawnUi.React),
//! - the **hub** (`designer-core`), which every app and every editor window connects to,
//! - an **editor** (the desktop window or the VS Code webview).
//!
//! The first frame a connection sends is [`Hello`]: it says which of the two it is. After that an
//! app sends [`AppMsg`] and receives [`HubToApp`]; an editor sends [`EditorMsg`] and receives
//! [`HubToEditor`]. The hub forwards what an app reports to every editor, tagged with the app's id.
//!
//! [`source`] holds the code side: the source tree a language service reads from a file and the
//! edits it answers, in the shape of the NX language protocol (`@nx-lang/language-protocol`).

use serde::{Deserialize, Serialize};

pub mod source;

/// The default port of the hub on localhost.
pub const DEFAULT_PORT: u16 = 54270;

/// The default port of the hub's MCP server (streamable HTTP at `/mcp`): the hub port + 1.
pub const DEFAULT_MCP_PORT: u16 = DEFAULT_PORT + 1;

/// The protocol version. A connection with another major version is refused.
pub const PROTOCOL_VERSION: &str = "0.1.0";

/// The first frame of every connection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "role", rename_all = "camelCase")]
pub enum Hello {
    /// A running app.
    App(AppInfo),
    /// An editor window.
    Editor { protocol: String },
}

/// Who a running app is.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub protocol: String,
    /// The app's display name.
    pub name: String,
    /// The engine and head, such as `dotnet-wpf`, `rust-web`.
    pub engine: String,
    pub pid: u32,
    /// The project the app was built from, when the agent knows it. Several instances of one
    /// project (Windows and Android) are shown together.
    pub project: Option<String>,
    /// The path of the in-app agent the app runs with (the .NET agent DLL): the hub passes it on
    /// when it runs the app again.
    #[serde(default)]
    pub agent: Option<String>,
    /// The operating system the app runs on: `windows`, `android`, `ios`, `ipados`, `maccatalyst`,
    /// `macos`, `linux` or `web`. Absent from older agents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

/// The id the hub gives each connected app.
pub type AppId = u32;

/// A control's id inside one app, stable while the control lives.
pub type NodeId = u64;

/// What an app reports.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AppMsg {
    /// The whole control tree, sent on connect and after every [`HubToApp::GetTree`].
    Tree(Tree),
    /// The screens the app can open (a shell's routes).
    Screens { screens: Vec<Screen> },
    /// The properties of one control, the answer to [`HubToApp::GetProps`].
    Props { node: NodeId, props: Vec<Prop> },
    /// The answer to [`HubToApp::Pick`]: the topmost control at that point, if any.
    Picked { request: u32, node: Option<NodeId> },
    /// A request failed.
    Error { request: Option<u32>, message: String },
    /// The answer to [`HubToApp::Snapshot`]: the canvas as a PNG, base64 (`pngBase64` in JSON).
    Snapshot { request: u32, png_base64: String, width: u32, height: u32 },
    /// A hot reload step in the app: `applied` when the runtime applied a code update (the
    /// framework's metadata update handler ran), `reloaded` when the app's own reload signal went
    /// out (DrawnUi Super.HotReload: code-built pages rebuild).
    HotReload { stage: String },
    /// The controls the app can create, the answer to [`HubToApp::GetControls`]: the library's and
    /// the app's own (custom controls), for the designer's Controls panel.
    Controls { controls: Vec<ControlKind> },
    /// A control was inserted live (an [`HubToApp::Insert`], or a click in place mode, request 0):
    /// its new id and where it went. A new [`AppMsg::Tree`] follows. `node` is `None` when it failed
    /// (an [`AppMsg::Error`] with the request says why).
    Inserted { request: u32, node: Option<NodeId>, parent: Option<NodeId>, index: Option<u32> },
    /// The answer to [`HubToApp::Move`]: the control (the same id), where it went now. `node` is
    /// `None` when it was refused (an [`AppMsg::Error`] with the request says why). A new
    /// [`AppMsg::Tree`] follows.
    Moved { request: u32, node: Option<NodeId>, parent: Option<NodeId>, index: Option<u32> },
    /// The answer to [`HubToApp::Copy`]: the control as a [`ControlSpec`], `None` when it is gone.
    Copied { request: u32, spec: Option<ControlSpec> },
}

/// A control with everything inside it, as a copy: what Paste builds again, live
/// ([`HubToApp::Insert`]) and in code ([`EditorMsg::InsertCode`]).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ControlSpec {
    /// Full type name, as [`Node::type_name`].
    pub type_name: String,
    /// Its set properties (the Properties panel's Set group), in the order to write them.
    #[serde(default)]
    pub props: Vec<SpecProp>,
    /// Its own children (not the library's parts, not children made from data), in order.
    #[serde(default)]
    pub children: Vec<ControlSpec>,
}

/// One property of a [`ControlSpec`]: its value as text (as for [`HubToApp::SetProperty`]) and its
/// type (as [`Prop::type_name`]), so code can spell it without asking the app again.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpecProp {
    pub name: String,
    pub value: String,
    pub type_name: String,
}

/// A control type the app can create, for the designer's Controls panel.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ControlKind {
    /// Full type name, as [`Node::type_name`].
    pub type_name: String,
    /// The panel's group: `Layout`, `Text`, `Media`, `Input`, `Shapes` or `Other`.
    pub group: String,
    /// Declared by the app (a custom control), not by the library.
    #[serde(default)]
    pub own: bool,
    /// The properties a new one gets, in the order written to code (`Text` = `Label`, a size for
    /// an empty layout): the same values live and in code. Values as for [`HubToApp::SetProperty`].
    #[serde(default)]
    pub defaults: Vec<(String, String)>,
    /// Can hold children (`Children`), so a placed control can go into it.
    #[serde(default)]
    pub container: bool,
}

/// One frame's control tree.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tree {
    /// Rises with every snapshot.
    pub revision: u64,
    /// Canvas size in pixels.
    pub width: f32,
    pub height: f32,
    /// Parents before their children, children in their parent's order.
    pub nodes: Vec<Node>,
}

/// One control.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    /// The control's type, such as `DrawnUi.Draw.SkiaLabel` or a user class.
    pub type_name: String,
    /// The assembly or crate that declares the type: tells the app's own classes from the library's.
    #[serde(default)]
    pub assembly: Option<String>,
    /// The control's Tag, when set.
    pub tag: Option<String>,
    /// A short preview of what the control shows (a label's or button's text, cut to 40 chars),
    /// so rows of the same type can be told apart.
    #[serde(default)]
    pub text: Option<String>,
    /// The library made this control for its own parts (a button's label, shell chrome, a slider's
    /// thumb): no app code declares it.
    #[serde(default)]
    pub internal: bool,
    /// The hosting page's own wrapper around the app (DrawFiddle's root layout for a C# snippet):
    /// not in the app's code, so nothing is placed, pasted or moved into it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub page_root: bool,
    /// A layout that makes its children from items and a template (ItemsSource / ItemTemplate,
    /// `items`), even with no items yet: nothing is placed, pasted or moved into it. Its cells carry
    /// [`Node::template_index`]. Left out (false) by agents that do not tell.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub templated: bool,
    /// For a recycled cell of a templated layout, its item index. Every cell of one template maps
    /// to the same code.
    #[serde(default)]
    pub template_index: Option<u64>,
    /// The field of an app class that holds this control (`.Assign(out _title)` -> `_title`), with
    /// the class that declares it: `Namespace.Page._title`.
    #[serde(default)]
    pub field: Option<String>,
    /// The name the app's code gives the control, when a helper made it: the method or local
    /// function that returns it (`Circle(..) => new SkiaShape { .. }`), the React component whose
    /// own element it is. Left out when the code writes the control in place, or nobody tells.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub made_by: Option<String>,
    /// On a root node: the app type (page or view) that holds the canvas, such as
    /// `DrawnUI.Tutorials.FirstApp.FirstAppPageCode`.
    #[serde(default)]
    pub host: Option<String>,
    /// Where the code that created this control is, when the app knows it (DrawnUi.React dev
    /// builds): `"<file>:<line>:<column>"`, 1-based, at the `<` of the JSX element (the column may be
    /// left out). The file is a path or a uri (`fiddle:///snippet.tsx`), compared by its ending.
    #[serde(default)]
    pub source: Option<String>,
    /// Where it was drawn last, in canvas pixels: x, y, width, height.
    pub rect: [f32; 4],
    /// Shown on the canvas now: visible, and its rect meets the canvas.
    pub visible: bool,
}

/// One property of a control.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Prop {
    pub name: String,
    /// The property's type, such as `System.Double`.
    pub type_name: String,
    /// The value as text (invariant culture).
    pub value: String,
    /// Whether the value was set, rather than left at its default.
    pub is_set: bool,
    /// Where an editor lists it: `Layout`, `Text`, `Look`, `Transform` or `Behavior`. Absent from older agents.
    #[serde(default)]
    pub group: Option<String>,
    /// The values an editor offers: an enum's cases in declaration order; None for free values.
    #[serde(default)]
    pub choices: Option<Vec<String>>,
    /// The choices combine (a [Flags] enum): an editor offers toggles.
    #[serde(default)]
    pub flags: bool,
    /// The value equals the property's default (the library may still have set it). None when the
    /// agent cannot tell, or from older agents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_default: Option<bool>,
}

/// A screen the app can open.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Screen {
    pub route: String,
    pub title: Option<String>,
}

/// What the hub asks of an app.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum HubToApp {
    /// The whole tree, without properties (they come per control with `GetProps`).
    GetTree,
    GetProps { node: NodeId },
    /// The topmost control at a canvas point in pixels.
    Pick { request: u32, x: f32, y: f32 },
    /// Draw a highlight box around a control on the device; `None` clears it. `color`: the box's
    /// color as `#RRGGBB` / `#AARRGGBB` (the designer's Selection color); `None` keeps the agent's own.
    Highlight {
        node: Option<NodeId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
    },
    /// Set a property on the live control (lost on restart).
    SetProperty { request: u32, node: NodeId, name: String, value: String },
    OpenScreen { route: String },
    /// Bring the app's window to the front (restored if minimized). The editor that asks has the
    /// user's click, so on Windows it grants the app the right to take the foreground first.
    Focus,
    /// The canvas as it is drawn now, answered by [`AppMsg::Snapshot`].
    Snapshot { request: u32 },
    /// Element picker in the app: while on, the pointer highlights the control under it and the next click
    /// does not reach the app but answers [`AppMsg::Picked`] (request 0) and turns the mode off. Esc or
    /// `on: false` cancels.
    PickMode { on: bool },
    /// The controls the app can create: answered by [`AppMsg::Controls`].
    GetControls,
    /// Create a control of `type_name` (one of [`AppMsg::Controls`], with its defaults) and add it
    /// live to `parent`'s children at `index` (`None`: last). Answered by [`AppMsg::Inserted`].
    Insert {
        request: u32,
        parent: NodeId,
        index: Option<u32>,
        type_name: String,
        /// A pasted copy's properties, as for [`HubToApp::SetProperty`]: on a paste (props or
        /// children given), instead of the type's defaults.
        #[serde(default)]
        props: Vec<(String, String)>,
        /// A pasted copy's children, built inside it in order, each with its own properties and children.
        #[serde(default)]
        children: Vec<ControlSpec>,
    },
    /// Place mode: like [`HubToApp::PickMode`], but the pointer boxes the layout a control of
    /// `type_name` would go into, with a line where it goes in a stack, row or wrap, and the click
    /// puts it there: [`AppMsg::Inserted`] with request 0. The control under the pointer (the
    /// topmost one, a library part counting as its owner) takes it when it holds children, at the
    /// index the pointer is at (a stack by the children's vertical middles, a row by their
    /// horizontal ones, a wrap in reading order; a layer or a grid last). Otherwise it goes next to
    /// it in its parent: before it in the first half along the parent's direction, else after (in
    /// a layer or a grid after). Where neither takes it (a template's cell, a page's own wrapper)
    /// nothing is placed and there is no box. `None`, Esc or a second click outside cancels.
    PlaceMode { type_name: Option<String> },
    /// A control of `type_name` put at canvas point `x`, `y` (pixels), where a click in place mode
    /// there puts it (a drop from the editor's Controls onto the app's canvas in a page). Answered
    /// as that click: [`AppMsg::Inserted`] with request 0.
    PlaceAt { type_name: String, x: f32, y: f32 },
    /// Take a control out of its parent, live. Answered by a new [`AppMsg::Tree`] (or an
    /// [`AppMsg::Error`]).
    Remove { request: u32, node: NodeId },
    /// Move a control, live, with everything inside it: into `parent` at `index`, its place among
    /// `parent`'s children that are not library parts, counted without the control itself (the
    /// index it has after the move; same counting as [`HubToApp::Insert`]). Answered by
    /// [`AppMsg::Moved`], then a new [`AppMsg::Tree`].
    Move { request: u32, node: NodeId, parent: NodeId, index: u32 },
    /// Read a control with everything inside it, for Copy: answered by [`AppMsg::Copied`].
    Copy { request: u32, node: NodeId },
}

/// What an editor sends. Every message names the app it is for.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EditorMsg {
    /// A request the hub forwards to the app.
    ToApp { app: AppId, msg: HubToApp },
    /// Write a property of a control into the code that creates it. `value` is text, as for
    /// [`HubToApp::SetProperty`]; the hub spells it by the property's type from the app's last
    /// [`AppMsg::Props`] for that control. Answered by [`HubToEditor::CodeEdited`] or
    /// [`HubToEditor::CodeRefused`].
    SetCode { request: u32, app: AppId, node: NodeId, prop: String, value: String },
    /// Take back the code edit of an earlier [`EditorMsg::SetCode`] of this editor (`request`):
    /// the edited text goes back to what it was, if it is still there as written. Answered by
    /// [`HubToEditor::CodeUndone`] or [`HubToEditor::CodeRefused`].
    UndoCode { request: u32 },
    /// Start a recent app (its project, as [`RecentApp::project`]). Progress comes as
    /// [`HubToEditor::AppRunState`].
    RunApp { project: String },
    /// Stop a connected app (the whole process tree when the hub started it) and run it again.
    RestartApp { app: AppId },
    /// Let apps on other devices (phones, other computers on the network) connect, or stop it.
    /// They connect to a separate port with a secret token in the address; only apps, never editors.
    /// Answered (to every editor) by [`HubToEditor::Devices`].
    AllowDevices { on: bool },
    /// Write an inserted control into the code: `new T { defaults }` added to the code that makes
    /// `parent`'s children. `node` is the control inserted live: the hub places it among the
    /// children the code makes (its record of live changes not written: `index` is not used) and
    /// spells each default by that control's property types from its last [`AppMsg::Props`].
    /// Empty `defaults`: the values it was inserted with. Answered by
    /// [`HubToEditor::CodeEdited`] (its `node` is the parent) or [`HubToEditor::CodeRefused`];
    /// [`EditorMsg::UndoCode`] takes it back.
    InsertCode {
        request: u32,
        app: AppId,
        node: NodeId,
        parent: NodeId,
        index: u32,
        type_name: String,
        defaults: Vec<(String, String)>,
        /// A pasted copy: the whole control as written (its props with their types and its children,
        /// nested); used instead of `type_name` and `defaults` when present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        spec: Option<ControlSpec>,
    },
    /// Take a control out of the code that makes it (one taken out live with [`HubToApp::Remove`]
    /// first: the hub matches the code with the tree from before that). Answered by
    /// [`HubToEditor::CodeEdited`] or [`HubToEditor::CodeRefused`]; [`EditorMsg::UndoCode`] puts it back.
    RemoveCode { request: u32, app: AppId, node: NodeId },
    /// Move a control in the code that makes it, to where it was moved live with [`HubToApp::Move`]:
    /// into `parent` at `index` (as in [`HubToApp::Move`]; the hub matches the code with its view of
    /// it, the control still at its old place). Answered by [`HubToEditor::CodeEdited`] or
    /// [`HubToEditor::CodeRefused`]; [`EditorMsg::UndoCode`] moves it back in the code.
    MoveCode { request: u32, app: AppId, node: NodeId, parent: NodeId, index: u32 },
    /// Put control `node`, taken out live, back in the app where it was, with `props` (as for
    /// [`HubToApp::SetProperty`]): the hub sends the [`HubToApp::Insert`] (its parent and index from
    /// its record of the removal) and knows the code still has it. Answered by [`AppMsg::Inserted`].
    Restore {
        request: u32,
        app: AppId,
        node: NodeId,
        props: Vec<(String, String)>,
        /// What it held (its [`AppMsg::Copied`] before it was taken out), built inside it again.
        #[serde(default)]
        children: Vec<ControlSpec>,
    },
}

/// Why something was not done, as a text key of the designer's texts (designer-i18n) and its values:
/// an editor shows it in its language, `to_string()` gives the English that AI agents read.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Refusal {
    pub key: String,
    #[serde(default)]
    pub args: Vec<(String, String)>,
}

impl Refusal {
    pub fn new(key: &str, args: &[(&str, &dyn std::fmt::Display)]) -> Refusal {
        Refusal { key: key.to_owned(), args: args.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&designer_i18n::english(&self.key, &self.args))
    }
}

impl From<Refusal> for String {
    fn from(r: Refusal) -> String {
        r.to_string()
    }
}

/// An app that connected before, kept across hub restarts so it can be run again.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecentApp {
    pub name: String,
    pub engine: String,
    /// The project it was built from; the key of the list.
    pub project: String,
    /// The in-app agent the app ran with.
    #[serde(default)]
    pub agent: Option<String>,
    /// When it last connected, in seconds since the Unix epoch.
    pub last_seen: u64,
    /// The platform it last ran on (`AppInfo::platform`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

/// Where a run started by [`EditorMsg::RunApp`] / [`EditorMsg::RestartApp`] is.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RunState {
    Building,
    Starting,
    /// An app from the project connected.
    Running,
    Failed,
}

/// What the hub tells an editor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HubToEditor {
    /// The designer version of the hub, its first message on connect: an editor of another version
    /// (or one that hears the apps first, from a hub older than this message) says so.
    Hub { version: String },
    /// The phones on a USB cable the hub watches for apps: Android phones through adb, iPhones and
    /// iPads through usbmux. Sent on connect and whenever a phone is plugged in or out.
    Cables { android: u32, ios: u32 },
    /// The properties the app's code sets on a control (the code that creates it), sent by the hub
    /// after the app's [`AppMsg::Props`] for that control. `names: None` when no code was found for it
    /// (XAML, parts the library builds): an editor then falls back to set-and-not-default.
    CodeSet { app: AppId, node: NodeId, names: Option<Vec<String>> },
    /// The names the app's code gives controls of its tree `revision` ([`Node::made_by`]), found by the
    /// hub's code reader after the tree came: the hub's own copy of the tree has them too.
    MadeBy { app: AppId, revision: u64, names: Vec<(NodeId, String)> },
    /// The address apps on other devices connect to (with its token), `None` when they cannot;
    /// `refusal` says why turning it on failed. Sent on [`EditorMsg::AllowDevices`] and on connect.
    Devices {
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refusal: Option<Refusal>,
    },
    /// The connected apps, sent on connect and whenever one comes or goes, and the recent apps
    /// that are not connected now, newest first.
    Apps {
        apps: Vec<ConnectedApp>,
        #[serde(default)]
        recent: Vec<RecentApp>,
    },
    /// A run of a project moved on. `message`: what happens now, or for `failed` the reason with the
    /// last build error lines.
    AppRunState {
        project: String,
        state: RunState,
        message: String,
        /// The message as a text key and its values, when it is the hub's own (not a build's output).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refusal: Option<Refusal>,
    },
    /// Something an app reported.
    FromApp { app: AppId, msg: AppMsg },
    /// A [`EditorMsg::SetCode`] was written: the file and the written text's range in it.
    CodeEdited { request: u32, node: NodeId, uri: String, range: source::EditorRange },
    /// A [`EditorMsg::SetCode`] was not written, and why: `reason` in English, `refusal` the same as a
    /// text key and its values for an editor to show in its language.
    CodeRefused {
        request: u32,
        node: NodeId,
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refusal: Option<Refusal>,
    },
    /// The code edit of [`EditorMsg::SetCode`] `request` was taken back in file `uri`.
    CodeUndone { request: u32, uri: String },
    /// The app built its controls again from its code (a hot reload) and the hub put the live
    /// changes not written back onto them: `moved` the controls they are on now (old id, new id),
    /// `replaying` the inserts on their way back (their new id comes in a later `Replayed`), `lost`
    /// the changes whose place is gone from the code.
    Replayed {
        app: AppId,
        moved: Vec<(NodeId, NodeId)>,
        #[serde(default)]
        replaying: Vec<NodeId>,
        #[serde(default)]
        lost: Vec<Refusal>,
    },
}

/// An app the hub holds a connection to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedApp {
    pub id: AppId,
    pub info: AppInfo,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_round_trips_with_a_role_tag() {
        let hello = Hello::App(AppInfo {
            protocol: PROTOCOL_VERSION.into(),
            name: "HelloWpf".into(),
            engine: "dotnet-wpf".into(),
            pid: 42,
            project: None,
            agent: Some(r"C:\a\DrawnUi.Designer.Agent.dll".into()),
            platform: None,
        });
        let text = serde_json::to_string(&hello).unwrap();
        assert!(text.starts_with(r#"{"role":"app""#), "{text}");
        assert_eq!(serde_json::from_str::<Hello>(&text).unwrap(), hello);
        // An agent that does not send `agent` still connects.
        let old = r#"{"role":"app","protocol":"0.1.0","name":"A","engine":"dotnet-wpf","pid":1,"project":null}"#;
        assert_eq!(serde_json::from_str::<Hello>(old).unwrap(), Hello::App(AppInfo { name: "A".into(), pid: 1, agent: None, ..info() }));
    }

    fn info() -> AppInfo {
        AppInfo { protocol: "0.1.0".into(), name: String::new(), engine: "dotnet-wpf".into(), pid: 0, project: None, agent: None, platform: None }
    }

    #[test]
    fn run_messages_carry_a_type_tag() {
        let text = serde_json::to_string(&EditorMsg::RunApp { project: "P".into() }).unwrap();
        assert_eq!(text, r#"{"type":"runApp","project":"P"}"#);
        let msg = HubToEditor::AppRunState { project: "P".into(), state: RunState::Building, message: "m".into(), refusal: None };
        assert_eq!(serde_json::to_string(&msg).unwrap(), r#"{"type":"appRunState","project":"P","state":"building","message":"m"}"#);
        let recent = RecentApp { name: "A".into(), engine: "dotnet-wpf".into(), project: "P".into(), agent: None, last_seen: 5, platform: None };
        let text = serde_json::to_string(&HubToEditor::Apps { apps: vec![], recent: vec![recent] }).unwrap();
        assert_eq!(text, r#"{"type":"apps","apps":[],"recent":[{"name":"A","engine":"dotnet-wpf","project":"P","agent":null,"lastSeen":5}]}"#);
        assert!(matches!(serde_json::from_str(r#"{"type":"apps","apps":[]}"#).unwrap(), HubToEditor::Apps { recent, .. } if recent.is_empty()));
    }

    #[test]
    fn app_messages_carry_a_type_tag() {
        let text = serde_json::to_string(&AppMsg::Screens { screens: vec![] }).unwrap();
        assert_eq!(text, r#"{"type":"screens","screens":[]}"#);
        let text = serde_json::to_string(&HubToApp::PickMode { on: true }).unwrap();
        assert_eq!(text, r#"{"type":"pickMode","on":true}"#);
        let prop: Prop = serde_json::from_str(r#"{"name":"Text","typeName":"System.String","value":"a","isSet":true}"#).unwrap();
        assert_eq!(prop.group, None);
        let text = serde_json::to_string(&HubToApp::Pick { request: 1, x: 2.0, y: 3.0 }).unwrap();
        assert_eq!(text, r#"{"type":"pick","request":1,"x":2.0,"y":3.0}"#);
    }

    #[test]
    fn editor_messages_carry_a_type_tag() {
        let msg = EditorMsg::ToApp { app: 1, msg: HubToApp::GetTree };
        let text = serde_json::to_string(&msg).unwrap();
        assert_eq!(text, r#"{"type":"toApp","app":1,"msg":{"type":"getTree"}}"#);
        assert_eq!(serde_json::from_str::<EditorMsg>(&text).unwrap(), msg);
        let msg = EditorMsg::SetCode { request: 2, app: 1, node: 7, prop: "FontSize".into(), value: "20".into() };
        let text = serde_json::to_string(&msg).unwrap();
        assert_eq!(text, r#"{"type":"setCode","request":2,"app":1,"node":7,"prop":"FontSize","value":"20"}"#);
        let text = serde_json::to_string(&AppMsg::Snapshot { request: 3, png_base64: "iVBO".into(), width: 2, height: 1 }).unwrap();
        assert_eq!(text, r#"{"type":"snapshot","request":3,"pngBase64":"iVBO","width":2,"height":1}"#);
        let text = serde_json::to_string(&HubToEditor::CodeRefused { request: 2, node: 7, reason: "x".into(), refusal: None }).unwrap();
        assert_eq!(text, r#"{"type":"codeRefused","request":2,"node":7,"reason":"x"}"#);
    }
}
