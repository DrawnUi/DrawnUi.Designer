//! The window: apps, screens, the control tree and the selected control's properties.

use crate::i18n;
use std::sync::OnceLock;
use std::sync::mpsc::{Receiver, channel};

use designer_protocol::{AppId, AppMsg, ConnectedApp, ControlKind, EditorMsg, HubToApp, HubToEditor, Node, Screen, Tree};
use drawnui::prelude::*;

/// What the UI hears from its link to the hub (the desktop's socket thread, or the web page's socket).
pub enum Incoming {
    Connected(bool),
    Hub(HubToEditor),
    /// The hosting page's look (DrawFiddle): a theme id, "dark" and "light" included.
    #[cfg_attr(not(target_os = "emscripten"), allow(dead_code))]
    Theme(String),
    /// Page mode: where the editor's frame sits in the page (CSS pixels), for a drag carried out of it.
    #[cfg_attr(not(target_os = "emscripten"), allow(dead_code))]
    FrameOffset(f32, f32),
    /// Page mode: a control dragged from Controls and dropped on the app's canvas, at that canvas
    /// point (pixels): it goes where a place-mode click there puts it.
    #[cfg_attr(not(target_os = "emscripten"), allow(dead_code))]
    Place { type_name: String, x: f32, y: f32 },
    /// Page mode: the page needs the code (`code.flush`, its request): the changes not in code are
    /// written (Auto, Always) and the page is answered with `code.flushed`.
    #[cfg_attr(not(target_os = "emscripten"), allow(dead_code))]
    Flush(serde_json::Value),
}

/// Sends one message to the hosting page (page mode).
type ToPage = Box<dyn Fn(serde_json::Value)>;

/// Sends one message to the hub.
type ToHub = Box<dyn Fn(EditorMsg)>;

static PORT: OnceLock<u16> = OnceLock::new();

mod color;
mod gradient;
mod history;
mod moves;
mod view;
mod writes;
pub use writes::WriteMode;

use history::{AddVerb, Step};

/// One row of the properties list.
/// The orders of the properties list, in the order its switch shows them (Set, Type).
/// Every start opens on Set: what the code sets matters first.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum PropsOrder {
    /// Two groups, each A to Z: Set (in code or live) and Unset.
    #[default]
    Set,
    /// "Type": under headers Layout, Text, Look, Transform, Behavior.
    Groups,
}

/// What the designer remembers between runs: on the desktop in
/// `%APPDATA%\DrawnUi.Designer\settings.json`; the web build keeps the defaults.
#[derive(Clone, Default, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: view::Theme,
    /// Layers also lists the parts the library builds for itself (a button's label).
    pub show_internal: bool,
    /// The side panels' widths as last dragged, points; 0 = the default width.
    pub screens_width: f32,
    pub properties_width: f32,
    /// The share of the left column Screens takes above Controls, as last dragged; 0 = half.
    pub screens_share: f32,
    /// Controls groups the user folded.
    pub folded_control_groups: Vec<String>,
    /// Property groups folded or unfolded against their default.
    pub toggled_groups: Vec<String>,
    /// The window's language (a code of `i18n::LANGUAGES`); none = the system's.
    pub language: Option<String>,
    /// Apps on other devices (phones, other computers) may connect to the hub.
    pub allow_devices: bool,
    /// The Settings section last shown; one that no longer exists opens Appearance.
    #[serde(deserialize_with = "section_or_default")]
    pub section: Section,
    /// The selection box's color in the app (`#AARRGGBB`); `None`: the agents' own magenta.
    pub selection_color: Option<String>,
    /// Theme and language changes apply at once, without the opening circle.
    pub instant_changes: bool,
    /// When the live changes are written to code (Settings, Code).
    pub write_mode: WriteMode,
}

fn section_or_default<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Section, D::Error> {
    let value = <serde_json::Value as serde::Deserialize>::deserialize(d)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

impl Settings {
    fn path() -> Option<std::path::PathBuf> {
        if cfg!(target_os = "emscripten") {
            return None;
        }
        std::env::var_os("APPDATA").or_else(|| std::env::var_os("HOME")).map(|d| std::path::PathBuf::from(d).join("DrawnUi.Designer").join("settings.json"))
    }

    fn load() -> Settings {
        Self::path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    fn save(&self) {
        if let Some(path) = Self::path() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, serde_json::to_string_pretty(self).expect("serializable"));
        }
    }
}

/// The sections of the Settings screen, in the order its list shows them: the look first, then the
/// panels as the window has them, then the connection, About last. (The Properties order has no
/// section: the switch over the list is the setting.)
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, serde::Serialize, serde::Deserialize)]
pub enum Section {
    #[default]
    Appearance,
    Layers,
    Code,
    Connection,
    About,
}

pub const SECTIONS: [Section; 5] = [Section::Appearance, Section::Layers, Section::Code, Section::Connection, Section::About];

#[derive(Clone)]
enum PropRow {
    /// A group's header: its name and how many properties it holds.
    Header { group: String, count: usize },
    /// A property: its index in `App::props`.
    Prop(usize),
}

/// The groups in the order a person looks for them; the agent puts every property in one.
const GROUPS: [&str; 5] = ["Layout", "Text", "Look", "Transform", "Behavior"];

/// The Controls panel's groups, in its order: the library's by kind, the app's own classes last.
pub const CONTROL_GROUPS: [&str; 7] = ["Layout", "Text", "Media", "Input", "Shapes", "Other", "Own"];

/// One row of the Controls panel.
#[derive(Clone)]
enum CtlRow {
    Header { group: String, count: usize },
    /// A control type: its index in the shown app's controls.
    Item(usize),
}

/// What is dragged over Layers: a control type from the Controls panel (its index there), or the
/// Layers row of a control (its id), to move it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum DragWhat {
    Control(usize),
    Row(u64),
}

/// A drag over Layers: what, where the press started (canvas pixels), where the pointer is
/// (points), and whether it moved far enough to be a drag.
#[derive(Clone, Copy)]
struct Drag {
    what: DragWhat,
    start: drawnui::skia::Point,
    at: drawnui::skia::Point,
    active: bool,
}

/// Where a dragged control would go in Layers: `parent`'s children at `index`; drawn as a line
/// before row `row` (`into`: row `row` itself highlighted, the control goes in last), `top` points
/// from the top of the Layers viewport, `depth` the indent of the line.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct DropAt {
    pub parent: u64,
    pub index: u32,
    pub row: usize,
    pub into: bool,
    pub depth: usize,
    pub top: f32,
}

/// A control added from the designer, by its tree path: the code write Undo takes back, once written.
#[derive(Clone, Default)]
struct Inserted {
    written: Option<u32>,
    /// What Write puts in code (a pasted copy, with what is inside it); None: its type's defaults.
    defaults: Option<Copied>,
}

/// A control the app just inserted, until a tree has it: its id, the values Write gives it, and
/// whether it is an added one (false: a taken-out control put back).
#[derive(Clone)]
struct Arriving {
    id: u64,
    defaults: Option<Copied>,
    added: bool,
    /// A new action of the history (not a control put back or added again by Redo).
    verb: Option<AddVerb>,
}

/// A control taken out of the app here. Layers keeps it as a dimmed row where it was (`node`,
/// under the parent at `parent_path`, child `index` of it) with Undo and Write, until Undo puts it
/// back or it is written out of the code (then it stays until the next start, Undo still there).
#[derive(Clone)]
struct Removed {
    node: Node,
    parent_path: String,
    /// The parent's id it hangs under in the last tree. Not written yet and that id gone: the app
    /// built that part again from its code, so the control is back and this row goes.
    parent_id: u64,
    index: usize,
    /// Its Set properties and what it holds: Undo puts it back with them.
    props: Vec<(String, String)>,
    children: Vec<Copied>,
    written: Option<u32>,
}

/// What Copy took: the control with everything inside it, as the app reads it ([`HubToApp::Copy`]).
pub(crate) type Copied = designer_protocol::ControlSpec;

/// What a copy read from the app is for.
#[derive(Clone, Copy, Debug)]
enum CopyFor {
    /// Copy: kept for Paste.
    Clip,
    /// Duplicate: pasted right after the selected control at once.
    Duplicate,
    /// Delete: the taken-out control `id`, so Undo puts it back whole.
    Removed(u64),
}

/// One row of the layers outline: a node, how deep it sits, and whether it has rows under it.
#[derive(Clone, Copy)]
struct Row {
    node: usize,
    depth: usize,
    has_children: bool,
    /// The last of its siblings among the shown rows: its tree line stops at the row's middle.
    last: bool,
    /// Bit k: the ancestor at depth k has siblings below, so its line runs through this row.
    lines: u64,
}

#[derive(Default)]
pub struct App {
    /// Inside a page (DrawFiddle's Visual view): the page is the app's window, so no title bar,
    /// Screens panel or status line; only Layers and Properties.
    pub(crate) embedded: bool,
    /// Remembered between runs; saved on every change when `persist` (the window, not the tests).
    settings: Settings,
    persist: bool,
    /// The window's root, and the layer under it that holds everything built again for a theme.
    root: Handle<SkiaLayout>,
    content_host: Handle<SkiaLayout>,
    shell: Handle<SkiaShell>,
    /// The Settings screen is shown, and which of its sections.
    on_settings: bool,
    on_help: bool,
    reopen_settings: bool,
    /// The theme change's picture over the window while it opens, and the theme it opens to.
    wipe: Option<ControlId>,
    wipe_to: Option<view::Theme>,
    /// The color picker's color while a color property is edited in it, the color it opened with,
    /// and whether its hex field is being typed in (the field then keeps what is typed).
    picker: Option<color::Hsva>,
    /// The color picker is open for Settings' Selection color, not for a property.
    picking_selection: bool,
    /// The gradient being edited in the gradient editor, and its selected stop.
    gradient: Option<gradient::Grad>,
    gradient_stop: usize,
    picker_old: Option<color::Hsva>,
    picker_typing: bool,
    /// The Language button's middle, canvas pixels: a language change opens from there.
    language_anchor: Option<drawnui::skia::Point>,
    language_button: Handle<SkiaShape>,
    settings_scroll: Handle<SkiaScroll>,
    section: Section,
    /// The Connection row whose text was copied last.
    copied: Option<usize>,
    /// The address apps on other devices connect to, while the hub lets them.
    device_url: Option<String>,
    /// Phones on a cable the hub watches: (Android, iPhone and iPad).
    pub(crate) cables: (u32, u32),
    settings_button: Handle<SkiaShape>,
    back_button: Handle<SkiaShape>,
    help_button: Handle<SkiaShape>,
    help_back_button: Handle<SkiaShape>,
    /// The clickable control under a pressed mouse button (its pressed look).
    pressed: Option<ControlId>,
    /// Counts hover changes: a change of the state, so the hover looks are drawn again.
    hover_changes: u32,
    light_segment: Handle<SkiaLayout>,
    inbox_rx: Option<Receiver<Incoming>>,
    to_hub: Option<ToHub>,
    /// Page mode: messages to the hosting page (`drag.out`).
    pub(crate) to_page: Option<ToPage>,
    /// Page mode: where the editor's frame sits in the page (CSS pixels), from `frame.offset`.
    pub(crate) frame_offset: (f32, f32),
    /// Page mode: a Controls drag carried out of the editor's frame (the page shows where it lands).
    drag_out: bool,
    connected: bool,
    apps: Vec<ConnectedApp>,
    /// Apps that connected before and are not connected now, newest first (the hub remembers them).
    recent: Vec<designer_protocol::RecentApp>,
    /// What a run or restart started from here is doing, per project: state and the hub's message.
    runs: std::collections::HashMap<String, (designer_protocol::RunState, String)>,
    /// The app whose tree shows.
    app: Option<AppId>,
    screens: Vec<Screen>,
    tree: Tree,
    /// Every shown control in tree order; `rows` is this minus what folded nodes hide.
    outline: Vec<Row>,
    rows: Vec<Row>,
    /// Folded nodes, and the nodes seen before (a new node off screen starts folded).
    collapsed: std::collections::HashSet<u64>,
    seen: std::collections::HashSet<u64>,
    selected: Option<usize>,
    /// Panel widths in points, set by dragging the dividers; 0 = the default.
    screens_w: f32,
    properties_w: f32,
    /// In a page: the side panels' widths at start, from the frame's address (`controlsWidth`,
    /// `propertiesWidth`); 0 = the page defaults. A width dragged since wins.
    pub(crate) page_widths: (f32, f32),
    /// The selected control's properties, set ones first.
    props: Vec<designer_protocol::Prop>,
    /// The property being edited, its value as typed, and what the last edit did.
    edit_prop: Option<String>,
    edit_value: String,
    /// The values to pick from for the edited property (an enum's cases, True / False), else empty.
    edit_choices: Vec<String>,
    /// Live edits not written to code yet, per control and property: their value.
    pending: std::collections::HashMap<(u64, String), String>,
    /// Write-to-code requests on their way: request id -> the control and property.
    /// Writes to code on the way: request, then the control, its path, the property and the
    /// value before the write (Undo puts it back).
    code_requests: std::collections::HashMap<u32, (u64, String, String, String, String)>,
    /// Each property's value before its first live edit, while the edit is not in code.
    live_original: std::collections::HashMap<(u64, String), String>,
    /// Code writes that Undo can take back, newest last, by the control's path in the tree (it
    /// outlives the control's id across a hot reload) and property: request and value before.
    written: std::collections::HashMap<(String, String), Vec<(u32, String)>>,
    /// The properties the app's code sets on a control, as the hub last read them: (node, names);
    /// `None` names = no code found for it.
    code_set: Option<(u64, Option<Vec<String>>)>,

    /// Undos of code writes on the way: request, then the control, property and value to set live.
    undo_requests: std::collections::HashMap<u32, (u64, String, String)>,
    /// The last tap on a Layers row (node id, time): a second one soon after folds the row.
    last_tree_tap: Option<(u64, f64)>,
    /// The properties row under the mouse (its code button shows).
    hover_prop: Option<usize>,
    /// The open choices dropdown, and the menu row under the mouse.
    popup: Option<PopupId>,
    menu_hover: Option<String>,
    /// The question asked before Restart, Run or closing with changes not in code (Manual): what
    /// waits for its answer.
    confirm: Option<writes::AfterFlush>,
    /// A flush of the changes not in code on its way.
    flushing: Option<writes::Flush>,
    /// Writes of a flush: their request, the flush's one step of the history.
    flush_groups: std::collections::HashMap<u32, u32>,
    /// Always: changes of new steps waiting to be written (an edit still open, a drag on).
    always_due: Vec<writes::Unwritten>,
    /// Wakes the window's inbox (Always writes from there).
    pub(crate) waker: Option<drawnui::Waker>,
    /// The edited property takes a number: the field gets - / + steppers that move it by `edit_step`.
    edit_step: Option<f64>,
    /// The choices combine (a flags enum): each one toggles on or off.
    edit_flags: bool,
    /// The edited property's value and its live edit when the edit started: Esc puts them back.
    edit_original: Option<(String, Option<String>)>,
    /// The properties list: group headers and property rows; `open_groups` holds the user's
    /// fold choices, the defaults are in `group_open`.
    prop_rows: Vec<PropRow>,
    toggled_groups: std::collections::HashSet<String>,
    /// How the properties list is ordered: Set (changed first), A to Z (default) or Groups.
    props_order: PropsOrder,
    /// The next click in the app picks a control.
    pick_mode: bool,
    /// The controls each app can create (its Controls panel).
    controls: std::collections::HashMap<AppId, Vec<ControlKind>>,
    /// The Controls panel's search text and rows; folded groups.
    controls_search: String,
    control_rows: Vec<CtlRow>,
    folded_controls: std::collections::HashSet<String>,
    /// Place mode: the type the next click in the app places.
    placing: Option<String>,
    /// Insert requests on the way: request, type.
    insert_requests: std::collections::HashMap<u32, String>,
    /// Requests whose app error the status line shows (insert, remove).
    tree_requests: std::collections::HashSet<u32>,
    /// A control the app just inserted: selected once a tree has it.
    select_inserted: Option<Arriving>,
    /// The app reloaded (a hot swap, its controls built again): the tree here holds ids that are
    /// gone or another control's, so nothing that names a control is sent until its new tree.
    pub(crate) tree_stale: bool,
    /// A control just moved here: selected when the app's next tree has it, so the Properties
    /// header and its status are about it (not about whatever was selected before).
    pub(crate) select_moved: Option<u64>,
    /// Pastes on the way (request, the values Write gives the copy) and taken-out controls being
    /// put back (request, their row's id).
    paste_props: std::collections::HashMap<u32, Copied>,
    /// Added controls the hub puts back after a rebuild, until their new id comes (old ids).
    replaying: std::collections::HashSet<u64>,
    /// Copies asked of the app, and what each is for.
    copy_requests: std::collections::HashMap<u32, CopyFor>,
    restore_requests: std::collections::HashMap<u32, u64>,
    /// Controls taken out here, in the order they were.
    removed: Vec<Removed>,
    /// Removals being written to code (request, row id) and taken back (request, row id).
    remove_code: std::collections::HashMap<u32, u64>,
    undo_removes: std::collections::HashMap<u32, u64>,
    /// Copy and Copy style: kept in the designer, not on the system clipboard.
    pub(crate) clip: Option<Copied>,
    pub(crate) style: Option<Vec<(String, String)>>,
    /// Controls added here, by tree path (it outlives ids across a hot reload).
    // ponytail: a later insert before one of them shifts its path; key by more than the path if that bites.
    inserted: std::collections::HashMap<String, Inserted>,
    /// Inserted controls being written to code (request, path) and taken back (request, node, path).
    insert_code: std::collections::HashMap<u32, u64>,
    undo_inserts: std::collections::HashMap<u32, (u64, String)>,
    drag: Option<Drag>,
    pub(crate) drop: Option<DropAt>,
    /// The left column's share for Screens while dragged (0 = half).
    screens_share: f32,
    status: String,
    /// The last status line reports a refusal or an error.
    status_error: bool,
    next_request: u32,
    /// The window has had its first size (desktop window settings are applied then).
    window_ready: bool,

    inbox: Handle<Inbox>,
    left_column: Handle<SkiaLayout>,
    controls_list: Handle<SkiaLayout>,
    controls_search_field: Handle<SkiaEditor>,
    /// The Controls search field has the keyboard (its frame reads accent).
    search_focused: bool,
    apps_list: Handle<SkiaLayout>,
    screens_list: Handle<SkiaLayout>,
    tree_list: Handle<SkiaLayout>,
    props_list: Handle<SkiaLayout>,
    props_scroll: Handle<SkiaScroll>,
    /// The one edit field, laid over the edited row.
    edit_overlay: Handle<SkiaLayout>,
    editor: Handle<SkiaEditor>,
    layers_scroll: Handle<SkiaScroll>,
    /// Layers' "No controls yet" (a connected app with no controls of its own).
    empty_tree: Handle<SkiaLayout>,
    /// How wide the Layers rows need to be to show their deepest and longest one, points: estimated
    /// from the tree (depth and text lengths), so no row outside the viewport is measured.
    pub(crate) layers_width: f32,

    /// The menu bar: its titles, the menu open and its popup, the submenu open (its line's
    /// command) and its popup, the lit line of each, the title under the mouse, and Alt pressed
    /// with nothing else (it opens File when let go).
    bar_titles: [Handle<SkiaShape>; view::menubar::MENU_COUNT],
    bar_open: Option<usize>,
    bar_popup: Option<PopupId>,
    bar_sub: Option<(view::menubar::Cmd, PopupId)>,
    bar_line: Option<usize>,
    sub_line: Option<usize>,
    bar_hover: Option<usize>,
    alt_alone: bool,
    /// Controls moved here, by `#id` before the write and by place after; moves asked of the app
    /// (request: control, where it was, a new action of the history), their writes and the undos
    /// of those writes.
    moved: std::collections::HashMap<String, moves::MovedRow>,
    move_requests: std::collections::HashMap<u32, (u64, (u64, u32), bool)>,
    move_code: std::collections::HashMap<u32, u64>,
    undo_moves: std::collections::HashMap<u32, (u64, String)>,
    /// The gap the Layers rows open under a drag, and a press on a Layers row that may become one.
    gap: moves::Gap,
    row_press: Option<(u64, drawnui::skia::Point)>,
    /// Esc ended a drag while the button is still down: its release is no tap.
    swallow_up: bool,
    /// Layers: how far the list is scrolled down (points), the row under the mouse, the last row
    /// the mouse was on, and the mouse on that row's Undo / Write strip.
    layers_offset: f32,
    /// How far Layers is scrolled sideways (points; negative when scrolled right).
    layers_offset_x: f32,
    /// The drop marks over Layers: the gap, and the bar on a layout that takes the control.
    gap_box: Handle<SkiaShape>,
    into_bar: Handle<SkiaShape>,
    /// How far Properties is scrolled down (points; negative when scrolled).
    props_offset: f32,
    hover_row: Option<usize>,
    strip_row: Option<usize>,
    strip_hovered: bool,
    /// The keyboard is on the bar with no menu open: the title it is on. And where the keyboard
    /// was before the bar took it (given back when the bar lets go).
    bar_title: Option<usize>,
    /// The keyboard drives the bar (strong look on what it is on; the mouse resting on a title moves nothing).
    bar_keys: bool,
    /// The mouse's last place and how many times it moved; the count when the keyboard last acted.
    pointer_at: Option<(f32, f32)>,
    pointer_moves: u32,
    keys_moves: u32,
    bar_return: Option<Option<drawnui::ControlId>>,
    /// The screen reader's focus before the bar took the keyboard, and the lines of the open menu
    /// and submenu (the focus goes onto the lit one).
    bar_return_focus: Option<drawnui::ControlId>,
    bar_line_handles: Vec<Handle<SkiaShape>>,
    sub_line_handles: Vec<Handle<SkiaShape>>,

    /// Undo history per app (Ctrl+Z, Ctrl+Y).
    histories: std::collections::HashMap<AppId, history::History>,
    /// The open edit: its control, property and the value it started from (one step when it ends).
    edit_start: Option<(u64, String, String)>,
    /// Requests sent by Undo and Redo of the history: their answers are no new actions.
    history_quiet: std::collections::HashSet<u32>,
    /// Write all's writes: request, the first request of the same Write all (one step).
    write_groups: std::collections::HashMap<u32, u32>,
    /// Code writes taken back by Ctrl+Z: the change stays live.
    keep_live: std::collections::HashMap<u32, String>,
    /// Adds done again by Redo: request, the control's id before.
    redo_adds: std::collections::HashMap<u32, u64>,
    /// Pastes and Duplicates on the way: request, which one.
    insert_verbs: std::collections::HashMap<u32, AddVerb>,
}

impl App {
    fn send(&self, msg: HubToApp) {
        if let (Some(to_hub), Some(app)) = (&self.to_hub, self.app) {
            to_hub(EditorMsg::ToApp { app, msg });
        }
    }

    fn request(&mut self) -> u32 {
        self.next_request += 1;
        self.next_request
    }

    /// Sets the edited property on the live control, and with `code` also in the app's source.
    /// Sets the edited property to the edited value in the running app only; the row then marks
    /// it as not written to code.
    fn apply_edit(&mut self) {
        self.apply_edit_live(true);
    }

    /// Sets the edited value in the app; `fetch` asks for the properties again after (not while a
    /// color is dragged: one request per move is enough).
    fn apply_edit_live(&mut self, fetch: bool) {
        let (Some(node), Some(prop)) = (self.selected_node().map(|n| n.id), self.edit_prop.clone()) else { return };
        let item = self.selected_node().and_then(|n| n.template_index);
        let value = self.edit_value.clone();
        let request = self.request();
        self.send(HubToApp::SetProperty { request, node, name: prop.clone(), value: value.clone() });
        if fetch {
            self.send(HubToApp::GetProps { node });
        }
        // A recycled cell: the live value is on this one cell; its code is the template of every item.
        self.status = match item {
            Some(index) => i18n::tr_args("status-live-cell", &[("prop", prop.clone().into()), ("value", value.clone().into()), ("index", (index as i64).into())]),
            None => i18n::tr_args("status-live", &[("prop", prop.clone().into()), ("value", value.clone().into())]),
        };
        self.status_error = false;
        if let Some(p) = self.props.iter().find(|p| p.name == prop) {
            self.live_original.entry((node, prop.clone())).or_insert_with(|| p.value.clone());
        }
        self.pending.insert((node, prop), value);
    }

    /// Writes property `index` of `props` into the app's code: its live edit when it has one,
    /// else its current value.
    fn write_prop(&mut self, index: usize, cx: &mut Cx) -> Option<u32> {
        let (Some(node), Some(p)) = (self.selected_node().map(|n| n.id), self.props.get(index)) else { return None };
        let prop = p.name.clone();
        let value = self.pending.get(&(node, prop.clone())).cloned().unwrap_or_else(|| p.value.clone());
        let request = self.write_value(node, &prop, &value);
        self.status = i18n::tr_args("status-writing", &[("prop", prop.clone().into())]);
        self.status_error = false;
        cx.items_reset(self.props_list);
        request
    }

    /// Writes `value` of `node`'s `prop` into the app's code; the write's request.
    fn write_value(&mut self, node: u64, prop: &str, value: &str) -> Option<u32> {
        let app = self.app?;
        let before = self.live_original.get(&(node, prop.to_owned())).cloned().unwrap_or_else(|| value.to_owned());
        let request = self.request();
        self.code_requests.insert(request, (node, self.node_path(node), prop.to_owned(), before, value.to_owned()));
        self.send_hub(EditorMsg::SetCode { request, app, node, prop: prop.to_owned(), value: value.to_owned() });
        Some(request)
    }

    /// Ends the edit in place without applying anything more.
    /// Esc: the edit ends and the value is what it was before it, also after +/- or a flag
    /// already applied it live in the app.
    fn cancel_edit(&mut self, cx: &mut Cx) {
        self.edit_start = None;
        if let (Some(node), Some(prop), Some((value, pending))) = (self.selected_node().map(|n| n.id), self.edit_prop.clone(), self.edit_original.take()) {
            let key = (node, prop.clone());
            if self.pending.get(&key) != pending.as_ref() {
                let request = self.request();
                self.send(HubToApp::SetProperty { request, node, name: prop.clone(), value: pending.clone().unwrap_or(value) });
                self.send(HubToApp::GetProps { node });
                match pending {
                    Some(v) => {
                        self.pending.insert(key, v);
                    }
                    None => {
                        self.pending.remove(&key);
                        self.live_original.remove(&key);
                    }
                }
                self.status = i18n::tr_args("status-cancelled", &[("prop", prop.clone().into())]);
                self.status_error = false;
            }
        }
        cx.focus(None::<ControlId>);
        self.end_edit(cx);
    }

    fn end_edit(&mut self, cx: &mut Cx) {
        // What the edit changed is one step of the history.
        self.flush_edit();
        self.edit_start = None;
        self.edit_prop = None;
        self.edit_original = None;
        (self.picker, self.picker_old, self.picker_typing) = (None, None, false);
        self.gradient = None;
        self.menu_hover = None;
        self.place_editor(cx);
        if let Some(popup) = self.popup.take() {
            cx.close_popup_window(popup);
        }
        cx.items_reset(self.props_list);
    }

    fn selected_node(&self) -> Option<&Node> {
        self.selected.and_then(|row| self.rows.get(row)).map(|row| &self.tree.nodes[row.node])
    }

    fn apply(&mut self, incoming: Incoming, cx: &mut Cx) {
        match incoming {
            Incoming::Theme(id) => self.set_theme_now(view::Theme::by_id(&id), cx),
            Incoming::FrameOffset(x, y) => self.frame_offset = (x, y),
            Incoming::Place { type_name, x, y } => self.place_at(type_name, x, y),
            Incoming::Flush(request) => self.page_flush(request, cx),
            Incoming::Connected(on) => {
                self.connected = on;
                if on && self.settings.allow_devices {
                    self.send_hub(EditorMsg::AllowDevices { on: true });
                }
            }
            Incoming::Hub(HubToEditor::Cables { android, ios }) => self.cables = (android, ios),
            Incoming::Hub(HubToEditor::Devices { url, refusal }) => {
                // The switch follows the hub, also when an AI agent turned it on or off (MCP), and is
                // remembered: the next start opens the port again.
                if refusal.is_none() && self.settings.allow_devices != url.is_some() {
                    self.settings.allow_devices = url.is_some();
                    self.save_settings();
                }
                self.device_url = url;
                if let Some(r) = refusal {
                    self.status = i18n::tr_args("status-devices-failed", &[("reason", i18n::tr_pairs(&r.key, &r.args).into())]);
                    self.status_error = true;
                }
            }
            Incoming::Hub(HubToEditor::Apps { apps, recent }) => {
                self.recent = recent;
                let names: Vec<String> = apps.iter().map(|a| format!("{} (pid {})", a.info.name, a.info.pid)).collect();
                log(&format!("apps: {}", if names.is_empty() { "none".to_owned() } else { names.join(", ") }));
                // The app that connected last is shown (the title bar has one pill: the last app); the
                // shown one stays while it is connected and no other connects.
                let fresh = apps.iter().filter(|a| !self.apps.iter().any(|b| b.id == a.id)).map(|a| a.id).max();
                self.apps = apps;
                self.forget_gone_apps();
                let keep = self.app.is_some_and(|id| self.apps.iter().any(|a| a.id == id));
                if fresh.is_some() || !keep {
                    self.app = fresh.or_else(|| self.apps.iter().map(|a| a.id).max());
                    self.screens.clear();
                    self.set_tree(Tree::default());
                    if self.app.is_some() {
                        self.send(HubToApp::GetTree);
                    }
                    self.app_changed(cx);
                }
                cx.items_reset(self.apps_list);
                cx.items_reset(self.screens_list);
            }
            Incoming::Hub(HubToEditor::FromApp { app, msg }) if Some(app) == self.app => match msg {
                AppMsg::Tree(tree) => {
                    log(&format!("tree: {} nodes, revision {}", tree.nodes.len(), tree.revision));
                    self.tree_stale = false;
                    let before = self.selected_node().map(|n| (self.node_path(n.id), n.type_name.clone()));
                    self.set_tree(tree);
                    // The app built its controls again (a hot reload, a DrawFiddle run after a write):
                    // the selection moves to the control at the same place, when it has the same type.
                    if self.selected.is_none()
                        && let Some((path, type_name)) = before
                        && let Some(id) = self.node_at_path(&path).filter(|&id| self.tree.nodes.iter().any(|n| n.id == id && n.type_name == type_name))
                        && let Some(row) = self.rows.iter().position(|r| self.tree.nodes[r.node].id == id)
                    {
                        self.select_row(row, cx);
                    }
                    // A control just inserted: selected, revealed, and marked as added here.
                    if let Some(arriving) = self.select_inserted.take_if(|a| self.tree.nodes.iter().any(|n| n.id == a.id)) {
                        let id = arriving.id;
                        if arriving.added {
                            if let Some(verb) = arriving.verb
                                && let Some(node) = self.tree.nodes.iter().find(|n| n.id == id).cloned()
                                && let Some(parent) = node.parent
                            {
                                let index = self.live_index(parent, id);
                                self.record(Step::Add { node: id, parent, index, type_name: node.type_name, spec: arriving.defaults.clone(), verb, gone: false });
                            }
                            self.inserted.insert(format!("#{id}"), Inserted { written: None, defaults: arriving.defaults });
                        }
                        self.reveal_and_select(id, cx);
                        let name = self.selected_node().map(|n| short_type(&n.type_name).to_owned()).unwrap_or_default();
                        self.status = i18n::tr_args(if arriving.added { "status-inserted" } else { "status-restored" }, &[("type", name.into())]);
                        self.status_error = false;
                    }
                    // A control just moved: selected, so what the panel says is about it.
                    if let Some(id) = self.select_moved.take_if(|id| self.tree.nodes.iter().any(|n| n.id == *id)) {
                        let status = std::mem::take(&mut self.status);
                        self.reveal_and_select(id, cx);
                        self.status = status;
                    }
                }
                AppMsg::Controls { controls } => {
                    self.controls.insert(app, controls);
                    self.rebuild_control_rows();
                    cx.items_reset(self.controls_list);
                }
                // Someone else's insert (the hub putting a change back after a rebuild): see Replayed.
                AppMsg::Inserted { request, .. } if request != 0 && !self.insert_requests.contains_key(&request) => {}
                AppMsg::Inserted { request, node, .. } => {
                    // A click in place mode (request 0) ends the mode; a drop names its request.
                    let type_name = if request == 0 { self.placing.take() } else { self.insert_requests.remove(&request) };
                    let name = type_name.as_deref().map(short_type).unwrap_or_default().to_owned();
                    let defaults = self.paste_props.remove(&request);
                    // A taken-out control is back: its dimmed row goes.
                    let restored = self.restore_requests.remove(&request);
                    // Redo adds a control again: the history follows it to its new id.
                    let again = self.redo_adds.remove(&request);
                    let verb = self.insert_verbs.remove(&request).unwrap_or(AddVerb::Add);
                    match node {
                        Some(id) => {
                            self.tree_requests.remove(&request);
                            if let Some(row) = restored {
                                self.removed.retain(|r| r.node.id != row);
                            }
                            if let Some(old) = restored.or(again) {
                                self.history_remap(old, id);
                            }
                            let verb = (restored.is_none() && again.is_none()).then_some(verb);
                            self.select_inserted = Some(Arriving { id, defaults, added: restored.is_none(), verb });
                        }
                        None => {
                            self.status = i18n::tr_args("status-not-inserted", &[("type", name.into())]);
                            self.status_error = true;
                        }
                    }
                }
                AppMsg::Copied { request, spec } if self.copy_requests.contains_key(&request) => self.copied(request, spec),
                AppMsg::Moved { request, node, parent, index } => self.moved_answer(request, node, parent, index),
                AppMsg::Error { request: Some(request), message } if self.tree_requests.remove(&request) => {
                    self.copy_requests.remove(&request);
                    self.status = i18n::tr_args("status-app-refused", &[("reason", message.into())]);
                    self.status_error = true;
                }
                AppMsg::Screens { screens } => {
                    self.screens = screens;
                    cx.items_reset(self.screens_list);
                }
                AppMsg::Picked { node, .. } => {
                    self.pick_mode = false;
                    if let Some(node) = node {
                        self.reveal_and_select(node, cx);
                    }
                }
                AppMsg::HotReload { stage } => {
                    // Its ids are old now: no move or drop names them; a drag under way ends.
                    self.tree_stale = true;
                    if self.drag.is_some_and(|d| d.active) {
                        self.end_drag(true, cx);
                        self.swallow_up = true;
                    }
                    self.send(HubToApp::GetTree);
                    self.status = i18n::tr(if stage == "reloaded" { "status-hot-reloaded" } else { "status-hot-applied" });
                    self.status_error = false;
                }
                AppMsg::Props { node, mut props } if self.selected_node().is_some_and(|n| n.id == node) => {
                    props.sort_by(|a, b| (!a.is_set, &a.name).cmp(&(!b.is_set, &b.name)));
                    // The Layers row shows a label's text: keep it in step with an edit of Text.
                    if let (Some(row), Some(text)) = (self.selected, props.iter().find(|p| p.name == "Text").map(|p| p.value.clone()))
                        && let Some(n) = self.rows.get(row).map(|r| r.node)
                    {
                        let text = (!text.is_empty()).then_some(text);
                        if self.tree.nodes[n].text != text {
                            self.tree.nodes[n].text = text;
                            cx.items_changed(self.tree_list, row);
                        }
                    }
                    self.props = props;
                    self.rebuild_prop_rows();
                    // The rows show the values the app just sent (after an Undo too): bind them again.
                    cx.items_reset(self.props_list);
                }
                _ => {}
            },
            Incoming::Hub(HubToEditor::AppRunState { project, state, message, refusal }) => {
                if state == designer_protocol::RunState::Running {
                    self.runs.remove(&project);
                } else {
                    // The hub's own words in the window's language; a build's error lines stay as they came.
                    let message = match refusal {
                        Some(r) => {
                            let head = i18n::tr_pairs(&r.key, &r.args);
                            match message.strip_prefix(&r.to_string()) {
                                Some(tail) => format!("{head}{tail}"),
                                None => head,
                            }
                        }
                        None => message,
                    };
                    self.runs.insert(project, (state, message));
                }
                cx.items_reset(self.apps_list);
            }
            Incoming::Hub(HubToEditor::CodeEdited { request, uri, range, .. }) => {
                if let Some((node, path, prop, before, value)) = self.code_requests.remove(&request) {
                    self.pending.remove(&(node, prop.clone()));
                    self.live_original.remove(&(node, prop.clone()));
                    self.written.entry((path.clone(), prop.clone())).or_default().push((request, before));
                    self.record_write(request, node, path, prop, value);
                }
                // An added control is in the code now: Undo takes that write back.
                // Written: from now on found by its place, which outlives its id across a hot reload.
                if let Some(id) = self.insert_code.remove(&request)
                    && let Some(mut ins) = self.inserted.remove(&format!("#{id}"))
                {
                    ins.written = Some(request);
                    let path = self.node_path(id);
                    self.inserted.insert(path.clone(), ins);
                    self.history_written_at(request, &path);
                    if !self.history_quiet.remove(&request) {
                        let type_name = self.tree.nodes.iter().find(|n| n.id == id).map(|n| n.type_name.clone()).unwrap_or_default();
                        let step = Step::WriteAdd { node: id, path, type_name, request };
                        if !self.record_flushed(request, step.clone()) {
                            self.record(step);
                        }
                    }
                }
                // A moved control is moved in the code too: found by its place from now on.
                if let Some(id) = self.move_code.remove(&request)
                    && let Some(mut m) = self.moved.remove(&format!("#{id}"))
                {
                    m.written = Some(request);
                    let path = self.node_path(id);
                    self.moved.insert(path.clone(), m);
                    self.history_written_at(request, &path);
                    if !self.history_quiet.remove(&request) {
                        let type_name = self.tree.nodes.iter().find(|n| n.id == id).map(|n| n.type_name.clone()).unwrap_or_default();
                        let step = Step::WriteMove { node: id, path, type_name, request };
                        if !self.record_flushed(request, step.clone()) {
                            self.record(step);
                        }
                    }
                }
                // A taken-out control is out of the code too.
                if let Some(row) = self.remove_code.remove(&request)
                    && let Some(r) = self.removed.iter_mut().find(|r| r.node.id == row)
                {
                    r.written = Some(request);
                    let type_name = r.node.type_name.clone();
                    if !self.history_quiet.remove(&request) {
                        let step = Step::WriteDelete { node: row, type_name, request };
                        if !self.record_flushed(request, step.clone()) {
                            self.record(step);
                        }
                    }
                }
                let file = std::path::Path::new(&uri).file_name().map_or(uri.clone(), |f| f.to_string_lossy().into_owned());
                self.status = i18n::tr_args("status-written", &[("file", file.into()), ("line", (range.start.line + 1).into())]);
                self.status_error = false;
                self.flush_answered(request, true, cx);
            }
            Incoming::Hub(HubToEditor::CodeRefused { request, reason, refusal, .. }) => {
                let reason = refusal.map_or(reason, |r| i18n::tr_pairs(&r.key, &r.args));
                (self.history_quiet.remove(&request), self.keep_live.remove(&request), self.write_groups.remove(&request));
                if let Some((node, prop, before)) = self.undo_requests.remove(&request) {
                    // Still in the code: Undo can try again.
                    self.written.entry((self.node_path(node), prop)).or_default().push((request, before));
                    self.status = i18n::tr_args("status-not-undone", &[("reason", reason.clone().into())]);
                } else if self.undo_inserts.remove(&request).is_some() || self.undo_removes.remove(&request).is_some() || self.undo_moves.remove(&request).is_some() {
                    // The added control stays written: Undo can try again.
                    self.status = i18n::tr_args("status-not-undone", &[("reason", reason.clone().into())]);
                } else {
                    self.insert_code.remove(&request);
                    self.remove_code.remove(&request);
                    self.move_code.remove(&request);
                    self.code_requests.remove(&request);
                    self.status = i18n::tr_args("status-not-written", &[("reason", reason.clone().into())]);
                }
                self.status_error = true;
                self.flush_answered(request, false, cx);
            }
            // The app built its controls again and the hub put the changes not written back on them.
            Incoming::Hub(HubToEditor::Replayed { app, moved, replaying, lost }) if Some(app) == self.app => self.replayed(moved, replaying, lost),
            // The names the code gives controls helpers made (read off the code by the hub after a tree).
            Incoming::Hub(HubToEditor::MadeBy { app, revision, names }) if Some(app) == self.app && revision == self.tree.revision => {
                for (id, name) in names {
                    if let Some(n) = self.tree.nodes.iter_mut().find(|n| n.id == id && n.made_by.is_none()) {
                        n.made_by = Some(name);
                    }
                }
                self.rebuild_rows();
                cx.items_reset(self.tree_list);
            }
            Incoming::Hub(HubToEditor::CodeSet { app, node, names }) if Some(app) == self.app => {
                self.code_set = Some((node, names));
                if self.selected_node().is_some_and(|n| n.id == node) {
                    self.rebuild_prop_rows();
                    cx.items_reset(self.props_list);
                }
            }
            Incoming::Hub(HubToEditor::CodeUndone { request, uri }) => {
                // An added control out of the code: out of the app too (by its id: after a hot
                // reload that id is gone and nothing else is taken out).
                // A taken-out control is back in the code: back in the app too.
                // Ctrl+Z took back only the write: the change stays live, not written.
                let kept = self.keep_live.remove(&request);
                let keep = kept.is_some();
                // A moved control back at its old place in the code: there live too, unless only
                // the write goes back.
                if let Some((node, key)) = self.undo_moves.remove(&request)
                    && let Some(m) = self.moved.remove(&key)
                {
                    if keep {
                        self.moved.insert(format!("#{node}"), moves::MovedRow { written: None, ..m });
                    } else {
                        let index = self.drop_index(node, m.from.0, m.from.1);
                        self.move_node(node, m.from.0, index, false);
                    }
                }
                if let Some(row) = self.undo_removes.remove(&request) {
                    if let Some(r) = self.removed.iter_mut().find(|r| r.node.id == row) {
                        r.written = None;
                    }
                    if !keep {
                        self.restore_live(row);
                    }
                }
                if keep
                    && let Some((node, path)) = self.undo_inserts.remove(&request)
                {
                    if let Some(ins) = self.inserted.remove(&path) {
                        self.inserted.insert(format!("#{node}"), Inserted { written: None, ..ins });
                    }
                } else if let Some((node, path)) = self.undo_inserts.remove(&request) {
                    self.inserted.remove(&path);
                    self.remove_live(node);
                    let file = std::path::Path::new(&uri).file_name().map_or(uri.clone(), |f| f.to_string_lossy().into_owned());
                    self.status = i18n::tr_args("status-removed-code", &[("file", file.into())]);
                    self.status_error = false;
                }
                if keep
                    && let Some((node, prop, before)) = self.undo_requests.remove(&request)
                {
                    // The live value is sent again: the hub keeps it live through the rebuild.
                    if let Some(value) = kept.filter(|v| *v != before) {
                        let set = self.request();
                        self.send(HubToApp::SetProperty { request: set, node, name: prop.clone(), value: value.clone() });
                        self.live_original.insert((node, prop.clone()), before);
                        self.pending.insert((node, prop), value);
                    }
                } else if let Some((node, prop, before)) = self.undo_requests.remove(&request) {
                    // The app shows it at once; hot reload brings the same value from the code.
                    let set = self.request();
                    self.send(HubToApp::SetProperty { request: set, node, name: prop.clone(), value: before.clone() });
                    self.send(HubToApp::GetProps { node });
                    let file = std::path::Path::new(&uri).file_name().map_or(uri.clone(), |f| f.to_string_lossy().into_owned());
                    self.status = i18n::tr_args("status-undone-code", &[("prop", prop.clone().into()), ("value", before.clone().into()), ("file", file.into())]);
                    self.status_error = false;
                }
            }
            Incoming::Hub(_) => {}
        }
        cx.items_reset(self.tree_list);
        cx.items_reset(self.props_list);
    }

    /// Takes a new tree, keeping the selected control selected when it is still there.
    fn set_tree(&mut self, mut tree: Tree) {
        let keep = self.selected_node().map(|n| n.id);
        self.forget_rebuilt(&tree);
        self.lay_back_removed(&mut tree);
        self.tree = tree;
        self.outline.clear();
        // Parts the library made for itself (a button's label, shell chrome) are left out: no code
        // of the app declares them. Their app-made children hang under the nearest shown ancestor.
        // So is a page's own wrapper (DrawFiddle's C# snippet): the snippet's root is the top row.
        let mut depth: std::collections::HashMap<u64, Option<usize>> = std::collections::HashMap::new();
        for (index, node) in self.tree.nodes.iter().enumerate() {
            let parent_depth = node.parent.and_then(|p| depth.get(&p).copied().flatten());
            if (node.internal && !self.settings.show_internal) || node.page_root {
                depth.insert(node.id, parent_depth);
                continue;
            }
            let d = parent_depth.map_or(0, |d| d + 1);
            depth.insert(node.id, Some(d));
            if let Some(last) = self.outline.last_mut()
                && last.depth < d
            {
                last.has_children = true;
            }
            self.outline.push(Row { node: index, depth: d, has_children: false, last: false, lines: 0 });
            // A control first seen off screen (the page under a pushed one) starts folded.
            if self.seen.insert(node.id) && !node.visible {
                self.collapsed.insert(node.id);
            }
        }
        self.selected = None;
        self.rebuild_rows();
        if let Some(id) = keep {
            self.select_node(id);
        }
    }

    /// The outline minus what folded nodes hide.
    fn rebuild_rows(&mut self) {
        let selected = self.selected_node().map(|n| n.id);
        self.rows.clear();
        let mut fold_depth: Option<usize> = None;
        for row in &self.outline {
            if let Some(d) = fold_depth {
                if row.depth > d {
                    continue;
                }
                fold_depth = None;
            }
            self.rows.push(*row);
            if row.has_children && self.collapsed.contains(&self.tree.nodes[row.node].id) {
                fold_depth = Some(row.depth);
            }
        }
        // A row added or taken out here also holds its dot, Undo, Write and its "taken out".
        // Found by path once, not per row (a path is a walk of the tree).
        let marked: std::collections::HashSet<u64> = self
            .inserted
            .keys()
            .filter_map(|k| match k.strip_prefix('#') {
                Some(id) => id.parse().ok(),
                None => self.node_at_path(k),
            })
            .chain(self.removed.iter().map(|r| r.node.id))
            .collect();
        let changed = |id: u64| match marked.contains(&id).then(|| self.row_change(id)).flatten() {
            Some((removed, _)) => 12.0 + 60.0 + if removed { 6.0 + i18n::tr("row-removed-tag").chars().count() as f32 * 7.0 } else { 0.0 },
            None => 0.0,
        };
        self.layers_width = self.rows.iter().map(|row| view::layers_row_width(&self.tree.nodes[row.node], row.depth) + changed(self.tree.nodes[row.node].id)).fold(0.0, f32::max);
        // Tree lines, from the bottom up: a row is the last of its siblings when no later row sits
        // at its depth before one shallower; an ancestor's line runs on while one does.
        let mut later = [false; 64];
        for row in self.rows.iter_mut().rev() {
            let d = row.depth.min(63);
            row.last = !later[d];
            row.lines = (0..d).filter(|&k| later[k]).fold(0, |bits, k| bits | 1 << k);
            later[d] = true;
            later[d + 1..].iter_mut().for_each(|l| *l = false);
        }
        self.selected = None;
        if let Some(id) = selected {
            self.select_node(id);
        }
    }

    /// The left column's width (Screens and Controls; in a page Controls alone): as dragged, else in a
    /// page what the page asked or narrower than on the desktop, so Layers in the middle has the room.
    fn screens_width(&self) -> f32 {
        match (self.screens_w, self.page_widths.0) {
            (w, _) if w > 0.0 => w,
            (_, w) if self.embedded && w > 0.0 => w,
            _ if self.embedded => 150.0,
            _ => 160.0,
        }
    }

    fn properties_width(&self) -> f32 {
        match (self.properties_w, self.page_widths.1) {
            (w, _) if w > 0.0 => w,
            (_, w) if self.embedded && w > 0.0 => w,
            _ if self.embedded => 270.0,
            _ => 340.0,
        }
    }

    /// Remembers the side panels' widths (after a divider is let go).
    pub fn save_panel_widths(&mut self) {
        (self.settings.screens_width, self.settings.properties_width) = (self.screens_w, self.properties_w);
        self.save_settings();
    }

    /// Shows a Settings section (remembered).
    pub fn show_section(&mut self, section: Section) {
        if self.section != section {
            self.section = section;
            self.settings.section = section;
            self.save_settings();
        }
    }

    fn save_settings(&self) {
        if self.persist {
            self.settings.save();
        }
    }

    /// Opens the Settings screen over the designer (it slides in).
    fn open_settings(&mut self, cx: &mut Cx) {
        if !self.on_settings {
            self.end_edit(cx);
            if self.on_help {
                cx.go_back(self.shell, false);
            }
            cx.go_to(self.shell, "settings", true);
        }
    }

    /// Opens Help over the designer (it slides in).
    fn open_help(&mut self, cx: &mut Cx) {
        if !self.on_help {
            self.end_edit(cx);
            if self.on_settings {
                cx.go_back(self.shell, false);
            }
            cx.go_to(self.shell, "help", true);
        }
    }

    /// Back from Settings or Help to the designer.
    fn close_page(&mut self, cx: &mut Cx) {
        cx.go_back(self.shell, true);
    }

    /// Switches the theme, remembered. The window as it is now stays on top as a picture while the
    /// window is built again in the new theme under it; the picture then opens from where the
    /// theme was picked (`view::ThemeWipe`) and goes away.
    pub fn set_theme(&mut self, theme: view::Theme, cx: &mut Cx) {
        // The theme shown, or being opened: picking it again does nothing.
        if self.wipe_to.unwrap_or(self.settings.theme) == theme {
            return;
        }
        // Where it was clicked, in canvas pixels; from the keyboard, the window's middle.
        let from = cx.gesture().map(|g| g.location);
        self.wipe_to = Some(theme);
        self.start_wipe(from, cx, move |app| {
            app.settings.theme = theme;
            view::set_theme(theme);
        });
    }

    /// A theme the page sets (DrawFiddle): applied at once, never animated.
    pub fn set_theme_now(&mut self, theme: view::Theme, cx: &mut Cx) {
        if self.wipe_to.unwrap_or(self.settings.theme) == theme {
            return;
        }
        self.change_now(cx, move |app| {
            app.settings.theme = theme;
            view::set_theme(theme);
        });
    }

    /// A change of the whole window (theme, language) without the opening circle: one that is still
    /// opening ends, the change applies, the window is built again before the next frame.
    fn change_now(&mut self, cx: &mut Cx, apply: impl FnOnce(&mut App) + 'static) {
        if let Some(previous) = self.wipe.take() {
            cx.remove(previous);
        }
        self.wipe_to = None;
        cx.postpone_before_draw::<App>(move |app, cx| {
            apply(app);
            app.save_settings();
            app.rebuild_window(cx);
        });
    }

    /// Switches the language (None = the system's), remembered; the window opens in it as for a theme,
    /// from the Language button.
    pub fn set_language(&mut self, language: Option<String>, cx: &mut Cx) {
        if self.settings.language == language {
            return;
        }
        let from = self.language_anchor;
        self.start_wipe(from, cx, move |app| {
            i18n::set_language(language.as_deref().unwrap_or(i18n::system_language()));
            app.settings.language = language;
        });
    }

    /// The window as it is now stays on top as a picture while `apply` changes the look (theme, language)
    /// and the window is built again under it; the picture then opens from `from` (canvas pixels) and goes.
    fn start_wipe(&mut self, from: Option<drawnui::skia::Point>, cx: &mut Cx, apply: impl FnOnce(&mut App) + 'static) {
        // Settings, "Animate theme and language changes" off: at once.
        if self.settings.instant_changes {
            return self.change_now(cx, apply);
        }
        // Changed while another change still opens: that one ends now (its picture goes, the window shows
        // what it was opening), so the new picture captures a clean window and keeps no half-open circle.
        if let Some(previous) = self.wipe.take() {
            cx.remove(previous);
        }
        let mut wipe = Handle::<view::ThemeWipe>::default();
        cx.add_child(self.root, view::theme_wipe(from).assign(&mut wipe));
        let wipe_id: ControlId = wipe.into();
        self.wipe = Some(wipe_id);
        // The circle's own clock: it starts once the new window is built under the picture, and moves at
        // most 25 ms per drawn frame. Rebuilding the window and the shader's first use make one or two
        // long frames; timed by the wall clock, the circle would leap and appear already wide.
        let total = 5000.0;
        let (mut started, mut last, mut elapsed) = (false, 0.0_f32, 0.0_f32);
        let mut done = false;
        let mut apply = Some(apply);
        cx.animate(wipe, total, easing::linear, move |v, cx| {
            let now = v * total;
            if started {
                elapsed += (now - last).min(25.0);
            }
            last = now;
            // The first frames: the old window is captured by the wipe's paint; once it has it, the
            // window is built again with the change, under it.
            if !started && cx.find::<view::ThemeWipe>(wipe).is_some_and(|w| w.captured()) {
                started = true;
                if let Some(apply) = apply.take() {
                    cx.postpone_before_draw::<App>(move |app, cx| {
                        apply(app);
                        app.save_settings();
                        app.rebuild_window(cx);
                    });
                }
            }
            // Constant speed.
            let progress = (elapsed / view::THEME_WIPE_MS).clamp(0.0, 1.0);
            if let Some(mut w) = cx.get_mut(wipe) {
                w.control_mut().set_progress(progress);
                w.mark(Dirty::DRAW);
            }
            // Removing the picture also stops this animation.
            if (progress >= 1.0 || v >= 1.0) && !std::mem::replace(&mut done, true) {
                cx.postpone_before_draw::<App>(move |app, cx| {
                    cx.remove(wipe);
                    if app.wipe == Some(wipe_id) {
                        (app.wipe, app.wipe_to) = (None, None);
                    }
                });
            }
        });
    }

    /// The picker's color changed (a drag, a key, the hex field): set live in the app; `done` at the
    /// end of a drag or a typed color also asks for the properties again.
    pub fn set_picker(&mut self, color: color::Hsva, done: bool, cx: &mut Cx) {
        // Settings' Selection color.
        if self.picking_selection && self.edit_prop.is_none() {
            self.picker = Some(color);
            return self.set_selection_color(Some(color.value()), cx);
        }
        // In the gradient editor the picker edits the selected stop.
        if self.gradient.is_some() {
            return self.set_stop_color(color, done, cx);
        }
        let changed = self.picker != Some(color);
        self.picker = Some(color);
        if changed {
            self.edit_value = color.value();
            self.apply_edit_live(done);
        } else if done && let Some(node) = self.selected_node().map(|n| n.id) {
            self.send(HubToApp::GetProps { node });
        }
        // The edited row shows the new color at once.
        self.refresh_edited_row(cx);
    }

    /// Sets the selection box's color (`None`: the agents' own), remembered; the box in the app
    /// takes it at once.
    pub fn set_selection_color(&mut self, color: Option<String>, _cx: &mut Cx) {
        if self.settings.selection_color == color {
            return;
        }
        self.settings.selection_color = color;
        self.save_settings();
        let node = self.selected_node().map(|n| n.id);
        if node.is_some() {
            self.send(HubToApp::Highlight { node, color: self.settings.selection_color.clone() });
        }
    }

    /// Keeps the picker's color (a valid hex typed included) and closes it: the property edit ends,
    /// or Settings' Selection color picker closes.
    pub fn close_picker(&mut self, cx: &mut Cx) {
        if let Some(color) = self.picker {
            self.set_picker(color, true, cx);
        }
        cx.focus(None::<ControlId>);
        if self.edit_prop.is_some() {
            self.end_edit(cx);
        } else {
            if let Some(popup) = self.popup.take() {
                cx.close_popup_window(popup);
            }
            (self.picker, self.picker_old, self.picker_typing) = (None, None, false);
            self.picking_selection = false;
        }
    }

    /// Opens the color picker for the Selection color under `anchor` (points).
    pub fn open_selection_picker(&mut self, anchor: drawnui::skia::Rect, cx: &mut Cx) {
        if let Some(popup) = self.popup.take() {
            cx.close_popup_window(popup);
        }
        let current = color::Hsva::parse(self.settings.selection_color.as_deref().unwrap_or(view::DEFAULT_SELECTION)).or_else(|| color::Hsva::parse(view::DEFAULT_SELECTION)).expect("the default selection color parses");
        (self.picker, self.picker_old, self.picker_typing) = (Some(current), Some(current), false);
        self.picking_selection = true;
        self.popup = Some(cx.open_popup_window(color::color_picker(self), anchor, PopupWindowOptions { cpu: true, ..Default::default() }));
    }

    /// Opens the language menu under the Language button (`anchor` in points; `center` in canvas pixels).
    pub fn open_language_menu(&mut self, anchor: drawnui::skia::Rect, center: drawnui::skia::Point, cx: &mut Cx) {
        if let Some(popup) = self.popup.take() {
            cx.close_popup_window(popup);
        }
        self.language_anchor = Some(center);
        self.popup = Some(cx.open_popup_window(view::language_menu(self), anchor, PopupWindowOptions { cpu: true, ..Default::default() }));
    }

    /// A pick in the language menu: it closes, the window opens in the language.
    pub fn pick_language(&mut self, language: Option<String>, cx: &mut Cx) {
        if let Some(popup) = self.popup.take() {
            cx.close_popup_window(popup);
        }
        self.menu_hover = None;
        self.set_language(language, cx);
    }

    /// Builds the window's content again (a new theme), showing the same screen.
    fn rebuild_window(&mut self, cx: &mut Cx) {
        self.end_edit(cx);
        let on_settings = self.on_settings;
        cx.clear_children(self.content_host);
        let shell = view::shell(self);
        cx.add_child(self.content_host, shell);
        if let Some(mut root) = cx.get_mut(self.root) {
            root.set_background_color(view::window_background());
        }
        // The new shell opens Settings once it is up (see view::shell).
        self.reopen_settings = on_settings;
    }

    /// Shows or hides the parts the library builds in Layers (remembered).
    pub fn set_show_internal(&mut self, on: bool, cx: &mut Cx) {
        if self.settings.show_internal != on {
            self.settings.show_internal = on;
            self.save_settings();
            let tree = self.tree.clone();
            self.set_tree(tree);
            cx.items_reset(self.tree_list);
        }
    }

    /// Puts a Connection row's text on the clipboard.
    /// Lets apps on other devices connect, or stops it (remembered).
    pub fn set_write_mode(&mut self, mode: WriteMode) {
        if self.settings.write_mode != mode {
            self.settings.write_mode = mode;
            self.always_due.clear();
            self.save_settings();
        }
    }

    pub fn set_allow_devices(&mut self, on: bool) {
        if self.settings.allow_devices != on {
            self.settings.allow_devices = on;
            self.save_settings();
            if !on {
                self.device_url = None;
            }
            self.send_hub(EditorMsg::AllowDevices { on });
        }
    }

    fn send_hub(&self, msg: EditorMsg) {
        if let Some(to_hub) = &self.to_hub {
            to_hub(msg);
        }
    }

    fn copy(&mut self, row: usize, text: String, cx: &mut Cx) {
        cx.set_clipboard(text);
        self.copied = Some(row);
    }

    /// The hub's port this window uses (the web build talks to the default one).
    pub fn hub_port(&self) -> u16 {
        PORT.get().copied().unwrap_or(designer_protocol::DEFAULT_PORT)
    }

    /// A control's place in the tree: its index among its siblings at each level, from the root
    /// ("0/2/1"). The same control after a hot reload has a new id but the same path.
    fn node_path(&self, id: u64) -> String {
        let mut parts = vec![];
        let mut at = Some(id);
        while let Some(id) = at {
            let Some(node) = self.tree.nodes.iter().find(|n| n.id == id) else { break };
            // Counted as the code makes them: what a rebuild from the code brings back.
            parts.push(self.tree.nodes.iter().filter(|n| n.parent == node.parent && self.in_code_here(n.id)).position(|n| n.id == id).unwrap_or(0).to_string());
            at = node.parent;
        }
        parts.reverse();
        parts.join("/")
    }

    /// Whether the code makes control `id`, as far as changes made here go: not added here and
    /// unwritten, not taken out here and written. Paths count only these, so a path is the place
    /// the control gets when the app builds its controls again from the code.
    fn in_code_here(&self, id: u64) -> bool {
        (self.inserted.is_empty() || !self.inserted.contains_key(&format!("#{id}"))) && !self.removed.iter().any(|r| r.node.id == id && r.written.is_some())
    }

    /// The control at `path` (see [`Self::node_path`]) in the current tree.
    fn node_at_path(&self, path: &str) -> Option<u64> {
        path_in(&self.tree, path, |id| self.in_code_here(id))
    }

    /// Whether Undo has something to take back for property `i`: a live edit, or a write to code.
    pub fn can_undo(&self, i: usize) -> bool {
        let (Some(node), Some(p)) = (self.selected_node().map(|n| n.id), self.props.get(i)) else { return false };
        self.live_original.contains_key(&(node, p.name.clone())) || self.written.get(&(self.node_path(node), p.name.clone())).is_some_and(|w| !w.is_empty())
    }

    /// Undo on property `i`: a live edit goes back to the value before it; else the newest write
    /// to code is taken back in the file (and in the app).
    fn undo_prop(&mut self, i: usize, cx: &mut Cx) {
        let (Some(node), Some(prop)) = (self.selected_node().map(|n| n.id), self.props.get(i).map(|p| p.name.clone())) else { return };
        // Undo on the row being edited ends that edit: its field, picker or gradient editor would
        // keep showing the value just taken back.
        if self.edit_prop.as_deref() == Some(prop.as_str()) {
            self.end_edit(cx);
        }
        let key = (node, prop.clone());
        if let Some(original) = self.live_original.remove(&key) {
            // Its live edits since its last write leave the history (Ctrl+Z does not take them back again).
            let mut written = false;
            self.forget(|s| match s {
                _ if written => true,
                Step::WriteValues { node: n, writes, .. } if *n == node && writes.iter().any(|w| w.0 == prop) => {
                    written = true;
                    true
                }
                Step::Values { node: n, values, .. } if *n == node => {
                    values.retain(|v| v.0 != prop);
                    !values.is_empty()
                }
                _ => true,
            });
            self.pending.remove(&key);
            // The row shows the value it goes back to at once; the app's answer confirms it.
            if let Some(p) = self.props.get_mut(i) {
                p.value = original.clone();
            }
            let request = self.request();
            self.send(HubToApp::SetProperty { request, node, name: prop.clone(), value: original.clone() });
            self.send(HubToApp::GetProps { node });
            self.status = i18n::tr_args("status-undone-app", &[("prop", prop.clone().into()), ("value", original.clone().into())]);
            self.status_error = false;
        } else if let Some((request, before)) = self.written.get_mut(&(self.node_path(node), prop.clone())).and_then(|w| w.pop()) {
            self.forget(|s| match s {
                Step::WriteValues { writes, .. } => {
                    writes.retain(|w| w.2 != request);
                    !writes.is_empty()
                }
                _ => true,
            });
            self.undo_requests.insert(request, (node, prop.clone(), before));
            if let Some(to_hub) = &self.to_hub {
                to_hub(EditorMsg::UndoCode { request });
            }
            self.status = i18n::tr_args("status-undoing", &[("prop", prop.clone().into())]);
            self.status_error = false;
        }
        cx.items_reset(self.props_list);
    }

    /// A tap on Layers row `index` at `time_ms`: selects it; a second tap within half a second
    /// folds or unfolds it.
    fn tap_tree_row(&mut self, index: usize, time_ms: f64, cx: &mut Cx) {
        let Some(id) = self.rows.get(index).map(|r| self.tree.nodes[r.node].id) else { return };
        if self.last_tree_tap.take().is_some_and(|(last, t)| last == id && time_ms - t < 500.0) {
            if self.rows[index].has_children {
                self.toggle_row(index, cx);
            }
            return;
        }
        self.last_tree_tap = Some((id, time_ms));
        self.select_row(index, cx);
    }

    /// Folds or unfolds the row at `index`.
    fn toggle_row(&mut self, index: usize, cx: &mut Cx) {
        let Some(row) = self.rows.get(index) else { return };
        let id = self.tree.nodes[row.node].id;
        if !self.collapsed.remove(&id) {
            self.collapsed.insert(id);
        }
        self.rebuild_rows();
        cx.items_reset(self.tree_list);
    }

    /// The platforms of the connected apps, in the order they connected, each once.
    pub(crate) fn platforms(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for p in self.apps.iter().filter_map(|a| a.info.platform.clone()) {
            if !out.contains(&p) {
                out.push(p);
            }
        }
        out
    }

    /// Opens the apps menu under `anchor`, in points.
    pub fn open_app_menu(&mut self, anchor: drawnui::skia::Rect, cx: &mut Cx) {
        if let Some(popup) = self.popup.take() {
            cx.close_popup_window(popup);
        }
        let menu = view::app_menu(self);
        self.popup = Some(cx.open_popup_window(menu, anchor, PopupWindowOptions { cpu: true, ..Default::default() }));
    }

    fn close_popup(&mut self, cx: &mut Cx) {
        if let Some(popup) = self.popup.take() {
            cx.close_popup_window(popup);
        }
        if std::mem::take(&mut self.picking_selection) {
            (self.picker, self.picker_old) = (None, None);
        }
        self.menu_hover = None;
    }

    /// A pick in the apps menu: a connected app is shown and comes to the front, a recent one runs.
    pub fn pick_app(&mut self, slot: usize, cx: &mut Cx) {
        self.close_popup(cx);
        if slot < self.apps.len() {
            self.show_app(slot, cx);
        } else if let Some(r) = self.recent.get(slot - self.apps.len()) {
            self.run_project(r.project.clone(), cx);
        }
        cx.items_reset(self.apps_list);
    }

    /// The title bar has one pill, the last app: the shown one, or else the most recent one to run.
    fn pill_count(&self) -> usize {
        usize::from(self.apps.len() + self.recent.len() > 0)
    }

    /// Where the one pill points in "connected apps, then recent ones": the shown app, else the first.
    fn pill_slot(&self) -> usize {
        self.app.and_then(|id| self.apps.iter().position(|a| a.id == id)).unwrap_or(0)
    }

    /// A tap on the pill: a connected app is brought to the front, a recent one runs.
    fn tap_pill(&mut self, _cell: usize, cx: &mut Cx) {
        let index = self.pill_slot();
        if index < self.apps.len() {
            return self.show_app(index, cx);
        }
        if let Some(r) = self.recent.get(index - self.apps.len()) {
            self.run_project(r.project.clone(), cx);
        }
    }

    /// Builds and starts a recent app with the designer attached; the changes not in code are written
    /// first (Auto, Always) or the designer asks (Manual).
    fn run_project(&mut self, project: String, cx: &mut Cx) {
        if self.runs.contains_key(&project) {
            return; // already on its way
        }
        if !self.changes_first(writes::AfterFlush::Run(project.clone()), cx) {
            self.run_now(project, cx);
        }
    }

    pub(crate) fn run_now(&mut self, project: String, cx: &mut Cx) {
        if self.runs.contains_key(&project) {
            return;
        }
        self.runs.insert(project.clone(), (designer_protocol::RunState::Building, String::new()));
        if let Some(to_hub) = &self.to_hub {
            to_hub(EditorMsg::RunApp { project });
        }
        cx.items_reset(self.apps_list);
    }

    /// Stops a connected app and runs it again (rebuilt, with the designer attached); the changes
    /// not in code are written first (Auto, Always) or the designer asks (Manual).
    fn restart_pill(&mut self, _cell: usize, cx: &mut Cx) {
        if self.apps.get(self.pill_slot()).is_none() {
            return;
        }
        if !self.changes_first(writes::AfterFlush::Restart, cx) {
            self.restart_now(cx);
        }
    }

    pub(crate) fn restart_now(&mut self, cx: &mut Cx) {
        let Some(a) = self.apps.get(self.pill_slot()) else { return };
        let (app, project) = (a.id, a.info.project.clone().unwrap_or_default());
        self.runs.insert(project, (designer_protocol::RunState::Building, String::new()));
        if let Some(to_hub) = &self.to_hub {
            to_hub(EditorMsg::RestartApp { app });
        }
        cx.items_reset(self.apps_list);
    }

    /// Shows another connected app, and brings its window to the front.
    fn show_app(&mut self, index: usize, cx: &mut Cx) {
        let Some((chosen, pid)) = self.apps.get(index).map(|a| (a.id, a.info.pid)) else { return };
        if self.app != Some(chosen) {
            self.app = Some(chosen);
            self.screens.clear();
            self.set_tree(Tree::default());
            self.send(HubToApp::GetTree);
            self.reset_lists(cx);
            self.app_changed(cx);
        }
        allow_foreground(pid);
        self.send(HubToApp::Focus);
    }

    /// Selects a row of the tree: highlights the control in the app and asks for its properties.
    fn select_row(&mut self, index: usize, cx: &mut Cx) {
        // The control already shown: its properties stay as they are (no reload, no flicker).
        if self.selected == Some(index) && !self.props.is_empty() {
            return;
        }
        // Another control: an open edit of the old one ends, its field hidden and no longer
        // holding the keyboard, its dropdown, color picker or gradient editor closed.
        if self.edit_prop.is_some() || self.popup.is_some() {
            if cx.focused() == Some(self.editor.into()) {
                cx.focus(None::<ControlId>);
            }
            self.end_edit(cx);
        }
        self.selected = Some(index);
        // A taken-out control's row: nothing in the app to box or edit.
        if let Some(id) = self.rows.get(index).map(|r| self.tree.nodes[r.node].id).filter(|&id| self.is_removed(id)) {
            return self.show_removed(id, cx);
        }
        self.flush_edit();
        self.edit_start = None;
        self.props.clear();
        self.prop_rows.clear();
        self.edit_prop = None;
        // While a control waits to be placed the line keeps saying how.
        self.status = if self.placing.is_some() { i18n::tr("place-hint") } else { String::new() };
        let node = self.selected_node().map(|n| n.id);
        self.send(HubToApp::Highlight { node, color: self.settings.selection_color.clone() });
        if let Some(node) = node {
            self.send(HubToApp::GetProps { node });
        }
        cx.items_reset(self.tree_list);
        cx.items_reset(self.props_list);
    }

    /// No control selected (a click on Layers' empty space): an open edit ends, Properties empties
    /// and the box in the app goes away.
    pub(crate) fn deselect(&mut self, cx: &mut Cx) {
        if self.selected.is_none() {
            return;
        }
        if self.edit_prop.is_some() || self.popup.is_some() {
            if cx.focused() == Some(self.editor.into()) {
                cx.focus(None::<ControlId>);
            }
            self.end_edit(cx);
        }
        self.selected = None;
        self.props.clear();
        self.prop_rows.clear();
        self.send(HubToApp::Highlight { node: None, color: self.settings.selection_color.clone() });
        cx.items_reset(self.tree_list);
        cx.items_reset(self.props_list);
    }

    /// A tap on row `index` of the properties list: a header folds or unfolds its group, a
    /// property starts editing.
    fn tap_prop_row(&mut self, index: usize, cx: &mut Cx) {
        match self.prop_rows.get(index).cloned() {
            Some(PropRow::Header { group, .. }) => {
                if !self.toggled_groups.remove(&group) {
                    self.toggled_groups.insert(group);
                }
                let mut groups: Vec<String> = self.toggled_groups.iter().cloned().collect();
                groups.sort();
                self.settings.toggled_groups = groups;
                self.save_settings();
                self.rebuild_prop_rows();
                cx.items_reset(self.props_list);
            }
            Some(PropRow::Prop(i)) => self.edit_prop_at(i, None, None, cx),
            None => {}
        }
    }

    /// Switches the properties list's order (for this run: every start opens on Set).
    fn set_props_order(&mut self, order: PropsOrder, cx: &mut Cx) {
        if self.props_order != order {
            self.props_order = order;
            self.rebuild_prop_rows();
            cx.items_reset(self.props_list);
        }
    }

    /// Whether a group shows its properties: Layout, Text and Look open, the rest folded, until
    /// the user taps the header.
    fn group_open(&self, group: &str) -> bool {
        let default = !matches!(group, "Transform" | "Behavior");
        default != self.toggled_groups.contains(group)
    }

    /// Whether property `i` is in the Set group: edited live or written here, set by the code that
    /// creates the control, or simply not at its default (a value that differs from the default
    /// is "set", wherever it came from: a layout alias's Fill, a style). Values set but equal to the
    /// default (IsEnabled true, ExpandDirtyRegion) stay in Unset.
    pub fn prop_changed(&self, i: usize) -> bool {
        let node = self.selected_node().map_or(0, |n| n.id);
        let Some(p) = self.props.get(i) else { return false };
        if self.pending.contains_key(&(node, p.name.clone())) || self.written.contains_key(&(self.node_path(node), p.name.clone())) {
            return true;
        }
        let in_code = matches!(&self.code_set, Some((n, Some(names))) if *n == node && names.iter().any(|name| *name == p.name));
        let not_default = match p.is_default {
            Some(default) => !default,
            None => p.is_set,
        };
        in_code || not_default
    }

    /// Builds the rows of the properties list for the current order.
    fn rebuild_prop_rows(&mut self) {
        self.prop_rows.clear();
        let mut az: Vec<usize> = (0..self.props.len()).collect();
        az.sort_by(|&a, &b| self.props[a].name.to_lowercase().cmp(&self.props[b].name.to_lowercase()));
        let groups: Vec<(String, Vec<usize>)> = match self.props_order {
            PropsOrder::Set => {
                let (changed, others): (Vec<usize>, Vec<usize>) = az.into_iter().partition(|&i| self.prop_changed(i));
                vec![("Changed".to_owned(), changed), ("Others".to_owned(), others)]
            }
            PropsOrder::Groups => {
                let group_of = |p: &designer_protocol::Prop| p.group.clone().unwrap_or_else(|| "Behavior".to_owned());
                GROUPS.iter().map(|g| (g.to_string(), az.iter().copied().filter(|&i| group_of(&self.props[i]) == *g).collect())).collect()
            }
        };
        for (group, members) in groups {
            if members.is_empty() {
                continue;
            }
            self.prop_rows.push(PropRow::Header { group: group.clone(), count: members.len() });
            if self.group_open(&group) {
                self.prop_rows.extend(members.into_iter().map(PropRow::Prop));
            }
        }
    }

    /// Starts editing property `index` of `props` in its row: a field (steppers for a number), or a
    /// dropdown of its choices under `anchor` (the row's value cell).
    /// `tap`: where the tap that started it was, in points; the caret goes there when it hit the
    /// value, else after the value.
    fn edit_prop_at(&mut self, index: usize, anchor: Option<drawnui::skia::Rect>, tap: Option<drawnui::skia::Point>, cx: &mut Cx) {
        let Some(p) = self.props.get(index).cloned() else { return };
        if let Some(popup) = self.popup.take() {
            cx.close_popup_window(popup);
        }
        self.picking_selection = false;
        self.edit_prop = Some(p.name.clone());
        self.edit_value = p.value.clone();
        let node = self.selected_node().map_or(0, |n| n.id);
        self.edit_original = Some((p.value.clone(), self.pending.get(&(node, p.name.clone())).cloned()));
        let (name, shown) = (p.name.clone(), p.value.clone());
        self.flush_edit();
        self.edit_start = Some((node, name, shown));
        self.edit_choices = match p.type_name.as_str() {
            "System.Boolean" => vec!["True".to_owned(), "False".to_owned()],
            _ => p.choices.clone().unwrap_or_default(),
        };
        self.edit_step = numeric_step(&p.type_name, &p.name);
        self.edit_flags = p.flags;
        // A gradient opens its editor under its value.
        if p.type_name.ends_with(".SkiaGradient")
            && let Some(anchor) = anchor
        {
            let value = p.value.clone();
            self.edit_choices.clear();
            self.open_gradient(&value, anchor, cx);
            cx.items_reset(self.props_list);
            self.place_editor(cx);
            return;
        }
        // A color opens the picker under its value.
        if p.type_name.ends_with(".Color")
            && let Some(anchor) = anchor
            && let Some(color) = color::Hsva::parse(&p.value)
        {
            self.edit_choices.clear();
            (self.picker, self.picker_old, self.picker_typing) = (Some(color), Some(color), false);
            self.popup = Some(cx.open_popup_window(color::color_picker(self), anchor, PopupWindowOptions { cpu: true, ..Default::default() }));
            cx.items_reset(self.props_list);
            self.place_editor(cx);
            return;
        }
        if !self.edit_choices.is_empty() {
            if let Some(anchor) = anchor {
                // Drawn on the CPU: a transparent window on Windows, so the panel's round corners show.
                self.popup = Some(cx.open_popup_window(view::choices_popup(self), anchor, PopupWindowOptions { cpu: true, ..Default::default() }));
            }
        } else {
            let value = self.edit_value.clone();
            if let Some(mut editor) = cx.get_mut(self.editor) {
                editor.set_text(value.as_str());
                // The field is reused for every edit: the caret goes after the value, as in any text field.
                editor.set_cursor_position(usize::MAX);
            }
            cx.focus(Some(self.editor));
            // A tap on the value: the caret where it was, once the field is laid out over the row.
            if let Some(at) = tap.filter(|p| anchor.is_some_and(|a| p.x >= a.left && p.x <= a.right && p.y >= a.top && p.y <= a.bottom)) {
                cx.postpone_before_draw(move |app: &mut App, cx| {
                    if let Some(index) = cx.editor_cursor_at(app.editor, at) {
                        if let Some(mut editor) = cx.get_mut(app.editor) {
                            editor.set_cursor_position(index);
                        }
                    }
                });
            }
        }
        cx.items_reset(self.props_list);
        self.place_editor(cx);
    }

    /// Lays the edit field over the edited row, where the list has scrolled it; hidden when no
    /// value is edited as text or the row is scrolled out of sight.
    fn place_editor(&mut self, cx: &mut Cx) {
        let row = self.edit_prop.as_ref().filter(|_| self.edit_choices.is_empty() && self.picker.is_none() && self.gradient.is_none()).and_then(|name| {
            self.prop_rows.iter().position(|r| matches!(r, PropRow::Prop(i) if self.props.get(*i).is_some_and(|p| &p.name == name)))
        });
        // The scroll's offset is 0 at the start and negative when scrolled down; inside its own
        // on_scrolled the scroll is not reachable, the offset it gave is kept in `props_offset`.
        let offset = cx.find::<SkiaScroll>(self.props_scroll).map_or(self.props_offset, |s| s.viewport_offset_y());
        let viewport = cx.base(self.props_scroll).map_or(f32::MAX, |b| b.rect.height() / b.scale.max(0.1));
        if let Some(mut me) = cx.get_mut(self.edit_overlay) {
            match row {
                Some(r) => {
                    let top = view::PROPS_PAD + r as f32 * view::ROW + offset;
                    me.set_margin(Thickness::new(148.0, top, view::ACTIONS, 0.0));
                    me.set_is_visible(top > -view::ROW && top < viewport);
                }
                None => me.set_is_visible(false),
            }
        }
    }

    /// Sets the edited property to `value` in the app at once (a choice, a stepper click).
    fn set_edit_value(&mut self, value: String, cx: &mut Cx) {
        self.edit_value = value.clone();
        if let Some(mut editor) = cx.get_mut(self.editor) {
            editor.set_text(value.as_str());
        }
        self.apply_edit();
        cx.items_reset(self.props_list);
    }

    /// A tap on choice `value`: sets it, or for a flags enum turns it on or off among the others.
    fn pick_choice(&mut self, value: String, cx: &mut Cx) {
        if !self.edit_flags {
            // A single choice ends the edit: the dropdown closes.
            self.set_edit_value(value, cx);
            return self.end_edit(cx);
        }
        let mut on: Vec<String> = flag_parts(&self.edit_value);
        if let Some(i) = on.iter().position(|v| v.eq_ignore_ascii_case(&value)) {
            on.remove(i);
        } else {
            on.push(value);
        }
        // Keep the enum's own order, so the code reads `Bold, Italic` whatever was tapped first.
        let order = &self.edit_choices;
        on.sort_by_key(|v| order.iter().position(|c| c.eq_ignore_ascii_case(v)).unwrap_or(usize::MAX));
        self.set_edit_value(if on.is_empty() { "None".to_owned() } else { on.join(", ") }, cx);
    }

    /// Whether choice `value` shows as selected for the edited value.
    fn choice_on(&self, value: &str) -> bool {
        if self.edit_flags {
            flag_parts(&self.edit_value).iter().any(|v| v.eq_ignore_ascii_case(value))
        } else {
            value.eq_ignore_ascii_case(self.edit_value.trim())
        }
    }

    /// Moves the edited number by `steps` steppers.
    fn step_edit_value(&mut self, steps: f64, cx: &mut Cx) {
        let (Some(step), Ok(now)) = (self.edit_step, self.edit_value.trim().parse::<f64>()) else { return };
        let next = ((now + steps * step) / step).round() * step;
        self.set_edit_value(format_number(next), cx);
    }

    /// Turns picking on or off: while on, the next click in the app selects a control here.
    fn toggle_pick(&mut self) {
        self.cancel_place();
        self.pick_mode = !self.pick_mode;
        self.send(HubToApp::PickMode { on: self.pick_mode });
    }

    /// Selects control `id`, unfolding its ancestors and scrolling it into view, and asks for its
    /// properties (a pick in the app).
    fn reveal_and_select(&mut self, id: u64, cx: &mut Cx) {
        let parents: std::collections::HashMap<u64, Option<u64>> = self.tree.nodes.iter().map(|n| (n.id, n.parent)).collect();
        let mut at = parents.get(&id).copied().flatten();
        while let Some(parent) = at {
            self.collapsed.remove(&parent);
            at = parents.get(&parent).copied().flatten();
        }
        self.rebuild_rows();
        if let Some(index) = self.rows.iter().position(|row| self.tree.nodes[row.node].id == id) {
            self.select_row(index, cx);
            cx.scroll_to_index(self.layers_scroll, index, RelativePositionType::Center, 250);
        }
    }

    fn reset_lists(&self, cx: &mut Cx) {
        cx.items_reset(self.apps_list);
        cx.items_reset(self.screens_list);
        cx.items_reset(self.tree_list);
        cx.items_reset(self.props_list);
    }

    fn select_node(&mut self, id: u64) {
        self.selected = self.rows.iter().position(|row| self.tree.nodes[row.node].id == id);
    }

    /// The controls the shown app can create.
    fn kinds(&self) -> &[ControlKind] {
        self.app.and_then(|a| self.controls.get(&a)).map_or(&[], Vec::as_slice)
    }

    /// The group a control type is listed under: the app's own classes under "Own", the rest by
    /// the agent's group ("Other" when it names none of the panel's).
    fn kind_group(k: &ControlKind) -> &str {
        if k.own {
            "Own"
        } else if CONTROL_GROUPS[..6].contains(&k.group.as_str()) {
            &k.group
        } else {
            "Other"
        }
    }

    /// The Controls panel's rows: each group that has a match, in the panel's order, its types
    /// A to Z under its header (none while folded).
    fn rebuild_control_rows(&mut self) {
        let search = self.controls_search.trim().to_lowercase();
        let kinds = self.kinds();
        let mut rows = vec![];
        for group in CONTROL_GROUPS {
            let mut members: Vec<usize> = (0..kinds.len())
                .filter(|&i| Self::kind_group(&kinds[i]) == group && (search.is_empty() || short_type(&kinds[i].type_name).to_lowercase().contains(&search)))
                .collect();
            if members.is_empty() {
                continue;
            }
            members.sort_by_key(|&i| short_type(&kinds[i].type_name).to_lowercase());
            rows.push(CtlRow::Header { group: group.to_owned(), count: members.len() });
            // A search shows every match, folded or not.
            if !search.is_empty() || !self.folded_controls.contains(group) {
                rows.extend(members.into_iter().map(CtlRow::Item));
            }
        }
        self.control_rows = rows;
    }

    /// The search text changed: the list shows the matches.
    fn set_controls_search(&mut self, text: &str, cx: &mut Cx) {
        if self.controls_search != text {
            self.controls_search = text.to_owned();
            self.rebuild_control_rows();
            cx.items_reset(self.controls_list);
        }
    }

    /// A tap (or Enter) on Controls row `index`: a header folds or unfolds its group; a control
    /// arms place mode, and a second tap on it cancels it.
    fn tap_control_row(&mut self, index: usize, cx: &mut Cx) {
        match self.control_rows.get(index).cloned() {
            Some(CtlRow::Header { group, .. }) => {
                if !self.folded_controls.remove(&group) {
                    self.folded_controls.insert(group);
                }
                let mut groups: Vec<String> = self.folded_controls.iter().cloned().collect();
                groups.sort();
                self.settings.folded_control_groups = groups;
                self.save_settings();
                self.rebuild_control_rows();
                cx.items_reset(self.controls_list);
            }
            Some(CtlRow::Item(k)) => {
                let Some(type_name) = self.kinds().get(k).map(|k| k.type_name.clone()) else { return };
                if self.placing.as_deref() == Some(type_name.as_str()) {
                    self.cancel_place();
                } else {
                    self.arm_place(type_name);
                }
            }
            None => {}
        }
    }

    /// The next click in the app places a control of `type_name` (Pick, if on, stops).
    fn arm_place(&mut self, type_name: String) {
        if self.pick_mode {
            self.pick_mode = false;
            self.send(HubToApp::PickMode { on: false });
        }
        self.status = i18n::tr("place-hint");
        self.status_error = false;
        self.send(HubToApp::PlaceMode { type_name: Some(type_name.clone()) });
        self.placing = Some(type_name);
    }

    /// A control of `type_name` put at the app's canvas point `x`, `y` (pixels), where a place-mode
    /// click there puts it: the page's drop of a drag carried out of the editor. Its `inserted`
    /// (request 0) is recorded as a placed control's.
    fn place_at(&mut self, type_name: String, x: f32, y: f32) {
        self.cancel_place();
        self.placing = Some(type_name.clone());
        self.send(HubToApp::PlaceAt { type_name, x, y });
    }

    /// Page mode: a Controls drag outside the editor's frame, told to the page in its own CSS pixels
    /// (`phase`: "move", "drop" or "cancel").
    fn post_drag_out(&self, phase: &str, at: drawnui::skia::Point) {
        let Some(DragWhat::Control(k)) = self.drag.map(|d| d.what) else { return };
        let (Some(post), Some(kind)) = (&self.to_page, self.kinds().get(k)) else { return };
        post(serde_json::json!({ "type": "drag.out", "typeName": kind.type_name, "x": self.frame_offset.0 + at.x, "y": self.frame_offset.1 + at.y, "phase": phase }));
    }

    /// Place mode ends without placing anything.
    fn cancel_place(&mut self) {
        if self.placing.take().is_some() {
            self.send(HubToApp::PlaceMode { type_name: None });
            self.status.clear();
        }
    }

    /// Whether control `id` can take a placed control: its type holds children (the app's Controls
    /// list says so), and it is no recycled cell's template parent (its cells come from a template).
    /// Control `id` is a cell made from a layout's items, or inside one: every cell is the same
    /// template code, so nothing goes into, beside or out of it (its properties stay editable).
    pub(crate) fn in_template(&self, id: u64) -> bool {
        let mut at = Some(id);
        while let Some(n) = at.and_then(|i| self.tree.nodes.iter().find(|n| n.id == i)) {
            if n.template_index.is_some() {
                return true;
            }
            at = n.parent;
        }
        false
    }

    /// Its place in the app's structure is not the designer's to change: a library part, or a
    /// control of a template's cell.
    pub(crate) fn locked(&self, node: &Node) -> bool {
        node.internal || self.in_template(node.id)
    }

    fn accepts_children(&self, id: u64) -> bool {
        let Some(node) = self.tree.nodes.iter().find(|n| n.id == id) else { return false };
        // A page's own wrapper (DrawFiddle's C# root layout) is not in the app's code: nothing goes
        // into it. The agent marks it; a React snippet has none (its root is the snippet's).
        // Nor into a part the library made for itself (a button's caption, a shell's layer): its
        // structure is the library's, only its properties are editable. Nor into a layout that makes
        // its children from items, even an empty one (`templated`; an agent that does not tell is
        // known by its cells' `template_index` below).
        if node.page_root || node.internal || node.templated || self.in_template(id) {
            return false;
        }
        !self.is_removed(id) && self.kinds().iter().any(|k| k.container && k.type_name == node.type_name) && !self.tree.nodes.iter().any(|n| n.parent == Some(id) && n.template_index.is_some())
    }

    /// Where a control dropped `y` points down the Layers list (its content, padding left out)
    /// goes: onto the middle of a container row = into it, last; else between two rows = before
    /// the lower one among its siblings (after the last row: after it). None where nothing can go.
    pub(crate) fn drop_at(&self, y: f32) -> Option<DropAt> {
        let at = |parent: u64, index: u32, row: usize, into: bool, depth: usize| Some(DropAt { parent, index, row, into, depth, top: 0.0 });
        // Indexes count the app's own children, not the library's parts (as the app's Children list
        // and the code do), nor the rows of controls taken out.
        let children = |id: u64| self.live_children(id);
        if self.rows.is_empty() {
            // An app with only its root (an empty DrawFiddle snippet): into the root.
            let root = self.tree.nodes.first().filter(|n| self.accepts_children(n.id))?;
            return at(root.id, children(root.id), 0, true, 0);
        }
        let pos = (y / view::ROW).max(0.0);
        let (r, frac) = (pos.floor() as usize, pos.fract());
        if let Some(row) = self.rows.get(r) {
            let node = &self.tree.nodes[row.node];
            if (0.25..0.75).contains(&frac) {
                if self.accepts_children(node.id) {
                    return at(node.id, children(node.id), r, true, row.depth);
                }
                // A container that takes nothing here (a layout of items, a library part, a page's
                // wrapper): no drop over it, never into its parent instead.
                if node.templated || self.kinds().iter().any(|k| k.container && k.type_name == node.type_name) {
                    return None;
                }
            }
        }
        let b = if r >= self.rows.len() { self.rows.len() } else if frac < 0.5 { r } else { r + 1 };
        let (row, after) = if b < self.rows.len() { (self.rows[b], 0) } else { (*self.rows.last()?, 1) };
        let node = &self.tree.nodes[row.node];
        // Never beside a library part or a control of a template's cell either: its place among its
        // siblings is not the app's.
        if self.locked(node) {
            return None;
        }
        let parent = node.parent.filter(|&p| self.accepts_children(p))?;
        let index = self.live_index(parent, node.id) + if self.is_removed(node.id) { 0 } else { after };
        at(parent, index, b, false, row.depth)
    }

    /// The drop under `at` (canvas pixels) while a control is dragged, placed in the Layers viewport.
    fn drop_target(&self, at: drawnui::skia::Point, cx: &Cx) -> Option<DropAt> {
        // A reload made the rows' ids old: nothing lands until the new tree.
        if self.tree_stale {
            return None;
        }
        let base = cx.base(self.layers_scroll)?;
        let (s, r) = (base.scale.max(0.1), base.rect);
        if at.x < r.left || at.x > r.right || at.y < r.top || at.y > r.bottom {
            return None;
        }
        // 0 at the start, negative when scrolled down.
        let offset = cx.find::<SkiaScroll>(self.layers_scroll).map_or(self.layers_offset, |sc| sc.viewport_offset_y());
        let mut y = (at.y - r.top) / s - offset - view::PROPS_PAD;
        // The rows below the open gap are pushed down by a row: over the gap the drop stays,
        // below it the row under the pointer is the one a row higher.
        if self.gap.target_open > 0.5 {
            let top = self.gap.target_pos * view::ROW;
            if y >= top && y < top + view::ROW {
                return self.drop;
            }
            if y >= top + view::ROW {
                y -= view::ROW;
            }
        }
        let mut drop = self.drop_at(y)?;
        if let Some(node) = self.dragged_node()
            && !self.can_move_into(node, drop.parent)
        {
            return None;
        }
        drop.top = view::PROPS_PAD + drop.row as f32 * view::ROW + offset;
        Some(drop)
    }

    /// The drag moved to `at` (canvas pixels, `scale` pixels per point).
    fn drag_to(&mut self, at: drawnui::skia::Point, scale: f32, cx: &mut Cx) {
        let Some(mut d) = self.drag else { return };
        d.at = drawnui::skia::Point::new(at.x / scale, at.y / scale);
        let starts = !d.active && (at.x - d.start.x).hypot(at.y - d.start.y) > 4.0 * scale;
        d.active |= starts;
        self.drag = Some(d);
        // The row lifted out of the list reads faded while it is carried.
        if starts && matches!(d.what, DragWhat::Row(_)) {
            cx.items_reset(self.tree_list);
        }
        // Page mode: a control carried out of the editor's frame goes on in the page (it may be
        // dropped on the app's canvas); back in, it is a drag over Layers again.
        if self.embedded && d.active && matches!(d.what, DragWhat::Control(_)) {
            let size = cx.canvas_size();
            if d.at.x < 0.0 || d.at.y < 0.0 || d.at.x >= size.width || d.at.y >= size.height {
                self.drag_out = true;
                self.post_drag_out("move", d.at);
                if self.drop.take().is_some() {
                    self.aim_gap(cx);
                }
                return;
            }
            if std::mem::take(&mut self.drag_out) {
                self.post_drag_out("cancel", d.at);
            }
        }
        let drop = if d.active { self.drop_target(at, cx) } else { None };
        if drop != self.drop {
            self.drop = drop;
            self.aim_gap(cx);
        }
    }

    /// The drag ends: a drop on Layers inserts the control there, live. True when it was a drag.
    fn end_drag(&mut self, cancelled: bool, cx: &mut Cx) -> bool {
        // Let go outside the editor's frame: the page places it (or not: off its canvas).
        if std::mem::take(&mut self.drag_out)
            && let Some(d) = self.drag
        {
            self.post_drag_out(if cancelled { "cancel" } else { "drop" }, d.at);
            self.drag = None;
            self.drop = None;
            self.gap.target_open = 0.0;
            return true;
        }
        let (Some(d), drop) = (self.drag.take(), self.drop.take()) else { return false };
        // Cancelled, the gap slides shut; dropped, the control's row takes its place at once.
        self.gap.target_open = 0.0;
        if !cancelled && drop.is_some() {
            self.gap.open = 0.0;
        }
        if d.active && matches!(d.what, DragWhat::Row(_)) {
            cx.items_reset(self.tree_list);
        }
        if let (false, Some(drop), DragWhat::Row(node)) = (cancelled, drop, d.what) {
            self.move_node(node, drop.parent, drop.index, true);
            return d.active;
        }
        let kind = match d.what {
            DragWhat::Control(k) => self.kinds().get(k).map(|k| k.type_name.clone()),
            DragWhat::Row(_) => None,
        };
        if let (false, Some(drop), Some(type_name)) = (cancelled, drop, kind) {
            self.cancel_place();
            let request = self.request();
            self.insert_requests.insert(request, type_name.clone());
            self.tree_requests.insert(request);
            self.send(HubToApp::Insert { request, parent: drop.parent, index: Some(drop.index), type_name, props: vec![], children: vec![] });
        }
        d.active
    }

    /// The designer added control `id`: `Some(false)` before it is written to code, `Some(true)`
    /// after; None for any other control.
    pub(crate) fn inserted_state(&self, id: u64) -> Option<bool> {
        if self.inserted.is_empty() {
            return None;
        }
        self.insert_key(id).and_then(|k| self.inserted.get(&k)).map(|i| i.written.is_some())
    }

    /// Undo on an added control: before Write it is taken out of the app; after, the code write
    /// is taken back first, then it is taken out (on `CodeUndone`).
    fn undo_insert(&mut self, id: u64, cx: &mut Cx) {
        let Some(path) = self.insert_key(id) else { return };
        let Some(ins) = self.inserted.get(&path).cloned() else { return };
        let type_name = self.tree.nodes.iter().find(|n| n.id == id).map(|n| short_type(&n.type_name).to_owned()).unwrap_or_default();
        match ins.written {
            None => {
                self.inserted.remove(&path);
                self.remove_live(id);
                self.status = i18n::tr_args("status-removed", &[("type", type_name.into())]);
            }
            Some(request) => {
                self.undo_inserts.insert(request, (id, path));
                self.send_hub(EditorMsg::UndoCode { request });
                self.status = i18n::tr_args("status-undoing", &[("prop", type_name.into())]);
            }
        }
        self.status_error = false;
        cx.items_reset(self.tree_list);
    }

    fn remove_live(&mut self, node: u64) {
        let request = self.request();
        self.tree_requests.insert(request);
        self.send(HubToApp::Remove { request, node });
    }

    /// Write on an added control: `new T { defaults }` into the code that makes its parent's children.
    fn write_insert(&mut self, id: u64, cx: &mut Cx) -> Option<u32> {
        let node = self.tree.nodes.iter().find(|n| n.id == id).cloned()?;
        let (Some(parent), Some(app)) = (node.parent, self.app) else { return None };
        // Counted among the children the code makes (the library's own parts left out).
        let index = self.live_index(parent, id);
        // A pasted copy as a whole (with what is inside it), else its type's defaults.
        let spec = self.insert_key(id).and_then(|k| self.inserted.get(&k)).and_then(|i| i.defaults.clone());
        let defaults = match &spec {
            Some(s) => s.props.iter().map(|p| (p.name.clone(), p.value.clone())).collect(),
            None => self.kinds().iter().find(|k| k.type_name == node.type_name).map(|k| k.defaults.clone()).unwrap_or_default(),
        };
        let request = self.request();
        self.insert_code.insert(request, id);
        self.send_hub(EditorMsg::InsertCode { request, app, node: id, parent, index, type_name: node.type_name.clone(), defaults, spec });
        self.status = i18n::tr_args("status-writing", &[("prop", short_type(&node.type_name).to_owned().into())]);
        self.status_error = false;
        cx.items_reset(self.tree_list);
        Some(request)
    }

    /// The left column's rows: Screens, its divider, Controls. Embedded (no Screens): Controls only.
    pub(crate) fn left_rows(&self) -> Vec<GridLength> {
        if self.embedded {
            return vec![GridLength::Absolute(0.0), GridLength::Absolute(0.0), GridLength::Star(1.0)];
        }
        let share = if self.screens_share > 0.0 { self.screens_share } else { 0.5 };
        vec![GridLength::Star(share), GridLength::Auto, GridLength::Star(1.0 - share)]
    }

    /// Remembers how the left column is split (after its divider is let go).
    pub fn save_screens_share(&mut self) {
        self.settings.screens_share = self.screens_share;
        self.save_settings();
    }

    /// Another app is shown (or none): its controls are asked for, a place under way ends.
    fn app_changed(&mut self, cx: &mut Cx) {
        self.placing = None;
        (self.drag, self.drop, self.select_inserted) = (None, None, None);
        if self.app.is_some() {
            self.send(HubToApp::GetControls);
        }
        self.rebuild_control_rows();
        cx.items_reset(self.controls_list);
    }
}

/// The control at `path` (see [`App::node_path`]) in `tree`.
/// The control at `path` (see [`App::node_path`]) in `tree`, counting the controls `counts` accepts.
fn path_in(tree: &Tree, path: &str, counts: impl Fn(u64) -> bool) -> Option<u64> {
    let mut at: Option<u64> = None;
    for part in path.split('/') {
        let index: usize = part.parse().ok()?;
        at = Some(tree.nodes.iter().filter(|n| n.parent == at && counts(n.id)).nth(index)?.id);
    }
    at
}

/// Whether a property is part of a control's look, which Copy style takes: colors, fonts, strokes,
/// corners, shadows, gradients, padding, opacity, layout and text alignment and the like. Never its
/// content (text, sources, items, bindings) nor its size and place (requests, margin, grid cells,
/// z order, translations). Names compare without case or `_` (`TextColor` and Rust's `text_color` alike).
pub(crate) fn is_style_prop(name: &str) -> bool {
    /// Content, size and place: never style, whatever their name holds.
    const NOT: &[&str] = &[
        "text", "source", "itemssource", "itemtemplate", "content", "children", "tag", "command", "commandparameter", "bindingcontext",
        "widthrequest", "heightrequest", "minimumwidthrequest", "minimumheightrequest", "maximumwidthrequest", "maximumheightrequest",
        "margin", "row", "column", "rowspan", "columnspan", "zindex", "translationx", "translationy", "translationz",
        "isvisible", "isenabled",
    ];
    /// Style whose name holds no look word: how the control and its text align.
    const ALIGN: &[&str] = &["horizontaloptions", "verticaloptions", "horizontaltextalignment", "verticaltextalignment"];
    /// What a look is made of: a property whose name holds one of these is style.
    const LOOK: &[&str] = &[
        "color", "font", "stroke", "corner", "radius", "shadow", "gradient", "padding", "opacity", "background", "fill", "blend",
        "linespacing", "lineheight", "characterspacing", "decoration", "underline", "strikeout", "bevel", "dash",
    ];
    let key: String = name.chars().filter(|c| *c != '_').collect::<String>().to_lowercase();
    ALIGN.contains(&key.as_str()) || (!NOT.contains(&key.as_str()) && LOOK.iter().any(|l| key.contains(l)))
}

impl App {
    /// Lays the taken-out controls' rows back where they were: the app's tree no longer has them. A
    /// tree of ours that still holds them (built again here) loses them and what they held first.
    /// The hub put the changes not written back onto the rebuilt controls: their marks (dots, Undo,
    /// Write) move to the new ids; what could not be kept is said in the status line.
    fn replayed(&mut self, moved: Vec<(u64, u64)>, replaying: Vec<u64>, lost: Vec<designer_protocol::Refusal>) {
        for (old, new) in moved {
            let rekey = |m: &mut std::collections::HashMap<(u64, String), String>| {
                let keys: Vec<(u64, String)> = m.keys().filter(|(n, _)| *n == old).cloned().collect();
                for k in keys {
                    if let Some(v) = m.remove(&k) {
                        m.insert((new, k.1), v);
                    }
                }
            };
            rekey(&mut self.pending);
            rekey(&mut self.live_original);
            self.history_remap(old, new);
            // A control just moved and moved again by the hub after a rebuild: selected under its new id.
            if self.select_moved == Some(old) {
                self.select_moved = Some(new);
            }
            if let Some(i) = self.inserted.remove(&format!("#{old}")) {
                self.inserted.insert(format!("#{new}"), i);
            }
            if let Some(m) = self.moved.remove(&format!("#{old}")) {
                self.moved.insert(format!("#{new}"), m);
            }
            for m in self.moved.values_mut().filter(|m| m.from.0 == old) {
                m.from.0 = new;
            }
            for r in &mut self.removed {
                if r.node.id == old {
                    r.node.id = new;
                }
                if r.parent_id == old {
                    r.parent_id = new;
                }
            }
            self.replaying.remove(&old);
        }
        self.replaying.extend(replaying);
        if let Some(first) = lost.first() {
            let what = i18n::tr_pairs(&first.key, &first.args);
            self.status = i18n::tr_args("status-not-kept", &[("count", lost.len().into()), ("what", what.into())]);
            self.status_error = true;
        }
    }

    /// The key of control `id`'s entry among the controls added here: `#id` before it is written,
    /// its place in the tree after.
    fn insert_key(&self, id: u64) -> Option<String> {
        let key = format!("#{id}");
        if self.inserted.contains_key(&key) {
            return Some(key);
        }
        let path = self.node_path(id);
        self.inserted.get(&path).filter(|i| i.written.is_some()).map(|_| path)
    }

    /// A new tree from the app: changes made here and not written are gone where the app built its
    /// controls again from its code (a hot reload): an added control whose id is gone, a taken-out
    /// one whose parent's id is gone (the code brought it back). Written ones stay (the code has them).
    fn forget_rebuilt(&mut self, tree: &Tree) {
        if self.inserted.is_empty() && self.removed.is_empty() && self.moved.is_empty() {
            return;
        }
        let ids: std::collections::HashSet<u64> = tree.nodes.iter().map(|n| n.id).collect();
        self.moved.retain(|k, m| m.written.is_some() || k.strip_prefix('#').and_then(|id| id.parse().ok()).is_some_and(|id: u64| ids.contains(&id)));
        let replaying = &self.replaying;
        self.inserted.retain(|k, i| i.written.is_some() || k.strip_prefix('#').and_then(|id| id.parse().ok()).is_some_and(|id: u64| ids.contains(&id) || replaying.contains(&id)));
        let held: std::collections::HashSet<u64> = self.removed.iter().map(|r| r.node.id).collect();
        self.removed.retain(|r| r.written.is_some() || ids.contains(&r.parent_id) || held.contains(&r.parent_id));
    }

    /// Lays the taken-out controls' rows back where they were: the app's tree no longer has them. A
    /// tree of ours that still holds them (built again here) loses them and what they held first.
    fn lay_back_removed(&mut self, tree: &mut Tree) {
        if self.removed.is_empty() {
            return;
        }
        let mut gone: std::collections::HashSet<u64> = self.removed.iter().map(|r| r.node.id).collect();
        tree.nodes.retain(|n| {
            let out = gone.contains(&n.id) || n.parent.is_some_and(|p| gone.contains(&p));
            if out {
                gone.insert(n.id);
            }
            !out
        });
        // Not written: under the same parent (the app did not build it again). Written: the code
        // and the app agree without it, the parent may be a new one; found by its place.
        // ponytail: a place shifts when a control above it is added or taken out meanwhile.
        let parents: Vec<Option<u64>> = self
            .removed
            .iter()
            .map(|r| if r.written.is_some() { path_in(tree, &r.parent_path, |id| self.in_code_here(id)) } else { Some(r.parent_id).filter(|p| tree.nodes.iter().any(|n| n.id == *p)) })
            .collect();
        for (r, parent) in self.removed.iter_mut().zip(parents) {
            let Some(parent) = parent else { continue };
            r.parent_id = parent;
            let Some(start) = tree.nodes.iter().position(|n| n.id == parent) else { continue };
            // A parent's subtree follows it (parents come before their children): the place of its
            // child `index`, or the subtree's end.
            let mut inside: std::collections::HashSet<u64> = [parent].into();
            let (mut at, mut kids) = (start + 1, 0);
            while let Some(n) = tree.nodes.get(at) {
                if !n.parent.is_some_and(|p| inside.contains(&p)) {
                    break;
                }
                if n.parent == Some(parent) {
                    if kids == r.index {
                        break;
                    }
                    kids += 1;
                }
                inside.insert(n.id);
                at += 1;
            }
            // Not shown in the app: the row reads dimmed.
            tree.nodes.insert(at, Node { parent: Some(parent), visible: false, ..r.node.clone() });
        }
    }

    /// Control `id` was taken out here (its row stays, dimmed).
    pub(crate) fn is_removed(&self, id: u64) -> bool {
        self.removed.iter().any(|r| r.node.id == id)
    }

    /// The app's own children of `parent` that are still in the app (no library parts, no rows of
    /// controls taken out): the indexes Insert and the code count.
    fn live_children(&self, parent: u64) -> u32 {
        self.tree.nodes.iter().filter(|n| n.parent == Some(parent) && !n.internal && !self.is_removed(n.id)).count() as u32
    }

    /// How many of those come before control `id`.
    fn live_index(&self, parent: u64, id: u64) -> u32 {
        self.tree.nodes.iter().filter(|n| n.parent == Some(parent)).take_while(|n| n.id != id).filter(|n| !n.internal && !self.is_removed(n.id)).count() as u32
    }

    /// What Layers shows on a control's row: `(taken out, written to code)` for one taken out here,
    /// `(false, written)` for one added here, None for the others.
    pub(crate) fn row_change(&self, id: u64) -> Option<(bool, bool)> {
        if let Some(r) = self.removed.iter().find(|r| r.node.id == id) {
            return Some((true, r.written.is_some()));
        }
        if let Some(key) = self.moved_key(id) {
            return Some((false, self.moved.get(&key).is_some_and(|m| m.written.is_some())));
        }
        self.inserted_state(id).map(|written| (false, written))
    }

    /// The selected control, when it is in the app (not a taken-out row).
    fn selected_live(&self) -> Option<&Node> {
        self.selected_node().filter(|n| !self.is_removed(n.id))
    }

    /// The selected control's Set group (what its code or a live edit sets), as Properties shows it.
    fn set_props(&self) -> Vec<(String, String)> {
        let node = self.selected_node().map_or(0, |n| n.id);
        (0..self.props.len())
            .filter(|&i| self.prop_changed(i))
            .map(|i| {
                let p = &self.props[i];
                (p.name.clone(), self.pending.get(&(node, p.name.clone())).cloned().unwrap_or_else(|| p.value.clone()))
            })
            .collect()
    }

    /// Copy: the app reads the selected control with everything inside it (its Set properties and
    /// its children's, with their types); `then_paste` (Duplicate) pastes it right after once read.
    fn copy_selected(&mut self, then_paste: bool) {
        let Some(node) = self.selected_live().map(|n| n.id) else { return };
        let request = self.request();
        self.copy_requests.insert(request, if then_paste { CopyFor::Duplicate } else { CopyFor::Clip });
        self.tree_requests.insert(request);
        self.send(HubToApp::Copy { request, node });
    }

    /// The app read a copy: kept for Paste (and pasted at once for Duplicate).
    fn copied(&mut self, request: u32, spec: Option<Copied>) {
        let purpose = self.copy_requests.remove(&request).unwrap_or(CopyFor::Clip);
        if let CopyFor::Removed(id) = purpose {
            if let (Some(spec), Some(r)) = (spec, self.removed.iter_mut().find(|r| r.node.id == id)) {
                r.props = spec.props.iter().map(|p| (p.name.clone(), p.value.clone())).collect();
                r.children = spec.children;
            }
            return;
        }
        let then_paste = matches!(purpose, CopyFor::Duplicate);
        let Some(spec) = spec else {
            self.status = i18n::tr("status-copy-gone");
            self.status_error = true;
            return;
        };
        self.tree_requests.remove(&request);
        fn inside(s: &Copied) -> usize {
            s.children.len() + s.children.iter().map(inside).sum::<usize>()
        }
        let (name, count) = (short_type(&spec.type_name).to_owned(), inside(&spec));
        self.status = if count > 0 {
            i18n::tr_args("status-copied-inside", &[("type", name.into()), ("count", count.into())])
        } else {
            i18n::tr_args("status-copied", &[("type", name.into()), ("count", spec.props.len().into())])
        };
        self.status_error = false;
        self.clip = Some(spec);
        if then_paste {
            self.paste(true);
        }
    }

    /// Copy style: the selected control's Set properties that make its look ([`is_style_prop`]).
    fn copy_style(&mut self) {
        let Some(type_name) = self.selected_live().map(|n| short_type(&n.type_name).to_owned()) else { return };
        let style: Vec<(String, String)> = self.set_props().into_iter().filter(|(name, _)| is_style_prop(name)).collect();
        self.status = i18n::tr_args("status-style-copied", &[("type", type_name.into()), ("count", style.len().into())]);
        self.status_error = false;
        self.style = Some(style);
    }

    /// Where Paste puts a control: into the selected control when it takes children (last), else
    /// right after it in its parent; `after` (Duplicate): always after it.
    pub(crate) fn paste_target(&self, after: bool) -> Option<(u64, u32)> {
        let node = self.selected_live().filter(|n| !self.locked(n))?;
        if !after && self.accepts_children(node.id) {
            return Some((node.id, self.live_children(node.id)));
        }
        let parent = node.parent.filter(|&p| self.accepts_children(p))?;
        Some((parent, self.live_index(parent, node.id) + 1))
    }

    /// Paste: a control of the copied type with the copied properties, live (Write puts it in code).
    fn paste(&mut self, after: bool) {
        let (Some(clip), Some((parent, index))) = (self.clip.clone(), self.paste_target(after)) else { return };
        self.cancel_place();
        let request = self.request();
        self.insert_verbs.insert(request, if after { AddVerb::Duplicate } else { AddVerb::Paste });
        self.insert_requests.insert(request, clip.type_name.clone());
        self.paste_props.insert(request, clip.clone());
        self.tree_requests.insert(request);
        let props = clip.props.iter().map(|p| (p.name.clone(), p.value.clone())).collect();
        self.send(HubToApp::Insert { request, parent, index: Some(index), type_name: clip.type_name, props, children: clip.children });
    }

    /// Duplicate: Copy, then Paste right after the selected control once the copy is read.
    fn duplicate(&mut self) {
        self.copy_selected(true);
    }

    /// Paste style: the copied look on the selected control, live, for the properties its type has.
    /// They become live edits, each with its dot, Undo and Write (and Write all).
    fn paste_style(&mut self, cx: &mut Cx) {
        let (Some(style), Some((node, type_name))) = (self.style.clone(), self.selected_live().map(|n| (n.id, short_type(&n.type_name).to_owned()))) else { return };
        let mut count = 0;
        let mut values = vec![];
        for (name, value) in style {
            let Some(before) = self.props.iter().find(|p| p.name == name).map(|p| p.value.clone()) else { continue };
            let shown = self.pending.get(&(node, name.clone())).cloned().unwrap_or_else(|| before.clone());
            let request = self.request();
            self.send(HubToApp::SetProperty { request, node, name: name.clone(), value: value.clone() });
            self.live_original.entry((node, name.clone())).or_insert(before);
            self.pending.insert((node, name.clone()), value.clone());
            values.push((name, shown, value));
            count += 1;
        }
        if count > 0 {
            self.record(Step::Values { node, values, style: true });
            self.send(HubToApp::GetProps { node });
            self.status = i18n::tr_args("status-style-pasted", &[("count", count.into())]);
        } else {
            self.status = i18n::tr_args("status-style-none", &[("type", type_name.into())]);
        }
        self.status_error = false;
        self.rebuild_prop_rows();
        cx.items_reset(self.props_list);
    }

    /// How many live edits of the selected control are not in code yet.
    pub(crate) fn pending_count(&self) -> usize {
        let node = self.selected_node().map_or(0, |n| n.id);
        self.pending.keys().filter(|(n, _)| *n == node).count()
    }

    /// Write all: every live edit of the selected control into code, one write each.
    fn write_all(&mut self, cx: &mut Cx) {
        let node = self.selected_node().map_or(0, |n| n.id);
        let edited: Vec<usize> = (0..self.props.len()).filter(|&i| self.pending.contains_key(&(node, self.props[i].name.clone()))).collect();
        let mut group = None;
        for i in edited {
            // One step of the history for all of them.
            if let Some(request) = self.write_prop(i, cx) {
                self.write_groups.insert(request, *group.get_or_insert(request));
            }
        }
    }

    /// Delete: the selected control out of the app, live. Its row stays where it was, dimmed, with
    /// Undo (back in the app) and Write (out of the code too). One added here goes as its Undo does.
    fn delete_selected(&mut self, cx: &mut Cx) {
        let Some(node) = self.selected_live().filter(|n| n.parent.is_some() && !self.locked(n)).cloned() else { return };
        let parent = node.parent.unwrap_or_default();
        if self.inserted_state(node.id) == Some(false) {
            let spec = self.inserted.get(&format!("#{}", node.id)).and_then(|i| i.defaults.clone());
            let index = self.live_index(parent, node.id);
            self.record(Step::Add { node: node.id, parent, index, type_name: node.type_name.clone(), spec, verb: AddVerb::Add, gone: true });
            return self.undo_insert(node.id, cx);
        }
        self.record(Step::Delete { node: node.id, type_name: node.type_name.clone() });
        self.delete_node(node.id, cx);
    }

    /// Control `id` out of the app, live, its row kept dimmed where it was.
    fn delete_node(&mut self, id: u64, cx: &mut Cx) {
        let Some(node) = self.tree.nodes.iter().find(|n| n.id == id && !self.is_removed(n.id)).cloned() else { return };
        let Some(parent) = node.parent else { return };
        let index = self.tree.nodes.iter().filter(|n| n.parent == Some(parent)).position(|n| n.id == node.id).unwrap_or(0);
        // Its Set properties when it is the one shown (Redo: also when another one is).
        let props = if self.selected_node().is_some_and(|n| n.id == id) { self.set_props() } else { vec![] };
        let parent_path = self.node_path(parent);
        self.remove_live(node.id);
        // The app reads it whole before it goes (it answers in order): Undo puts it back with its children.
        let request = self.request();
        self.copy_requests.insert(request, CopyFor::Removed(node.id));
        self.send(HubToApp::Copy { request, node: node.id });
        self.removed.push(Removed { node: node.clone(), parent_path, index, props, children: vec![], written: None, parent_id: parent });
        let tree = self.tree.clone();
        self.set_tree(tree);
        self.show_removed(node.id, cx);
    }

    /// The selected row is a taken-out control: nothing to edit or box in the app; the line says so.
    fn show_removed(&mut self, id: u64, cx: &mut Cx) {
        let Some(r) = self.removed.iter().find(|r| r.node.id == id) else { return };
        let name = short_type(&r.node.type_name).to_owned();
        self.status = i18n::tr_args(if r.written.is_some() { "status-removed-written" } else { "status-removed-live" }, &[("type", name.into())]);
        self.status_error = false;
        (self.props, self.prop_rows, self.edit_prop) = (vec![], vec![], None);
        self.send(HubToApp::Highlight { node: None, color: self.settings.selection_color.clone() });
        cx.items_reset(self.tree_list);
        cx.items_reset(self.props_list);
    }

    /// Undo on a row: a taken-out control comes back, an added one goes.
    fn undo_row(&mut self, id: u64, cx: &mut Cx) {
        // What the row takes back leaves the history (Ctrl+Z does not take it back again).
        self.forget(|s| !matches!(s, Step::Add { .. } | Step::WriteAdd { .. } | Step::Delete { .. } | Step::WriteDelete { .. } | Step::Move { .. } | Step::WriteMove { .. }) || s.node() != id);
        if self.moved_key(id).is_some() {
            return self.undo_move(id, cx);
        }
        let Some(written) = self.removed.iter().find(|r| r.node.id == id).map(|r| r.written) else { return self.undo_insert(id, cx) };
        match written {
            None => self.restore_live(id),
            Some(request) => {
                self.undo_removes.insert(request, id);
                self.send_hub(EditorMsg::UndoCode { request });
                let name = self.tree.nodes.iter().find(|n| n.id == id).map(|n| short_type(&n.type_name).to_owned()).unwrap_or_default();
                self.status = i18n::tr_args("status-undoing", &[("prop", name.into())]);
                self.status_error = false;
            }
        }
        cx.items_reset(self.tree_list);
    }

    /// Write on a row: a taken-out control out of the code, an added one into it.
    fn write_row(&mut self, id: u64, cx: &mut Cx) {
        if self.moved_key(id).is_some() {
            self.write_move(id, cx);
        } else if !self.is_removed(id) {
            self.write_insert(id, cx);
        } else {
            self.write_removal(id, cx);
        }
    }

    /// A taken-out control out of the code too; the write's request.
    fn write_removal(&mut self, id: u64, cx: &mut Cx) -> Option<u32> {
        let app = self.app?;
        let request = self.request();
        self.remove_code.insert(request, id);
        self.send_hub(EditorMsg::RemoveCode { request, app, node: id });
        let name = self.tree.nodes.iter().find(|n| n.id == id).map(|n| short_type(&n.type_name).to_owned()).unwrap_or_default();
        self.status = i18n::tr_args("status-writing", &[("prop", name.into())]);
        self.status_error = false;
        cx.items_reset(self.tree_list);
        Some(request)
    }

    /// A taken-out control back in the app where its row is, with its Set properties.
    fn restore_live(&mut self, id: u64) {
        let (Some(r), Some(app)) = (self.removed.iter().find(|r| r.node.id == id).cloned(), self.app) else { return };
        let request = self.request();
        self.restore_requests.insert(request, id);
        self.insert_requests.insert(request, r.node.type_name.clone());
        self.tree_requests.insert(request);
        // The hub puts it where it was and knows the code still has it.
        self.send_hub(EditorMsg::Restore { request, app, node: id, props: r.props, children: r.children });
    }

    /// The Layers menu (right click, the Menu key, Shift+F10) under `anchor`, in points.
    pub(crate) fn open_layers_menu(&mut self, anchor: drawnui::skia::Rect, cx: &mut Cx) {
        self.close_popup(cx);
        self.popup = Some(cx.open_popup_window(view::layers_menu(self), anchor, PopupWindowOptions { cpu: true, ..Default::default() }));
    }

    /// The Layers menu from the keyboard: under the selected row.
    fn open_layers_menu_at_selection(&mut self, cx: &mut Cx) -> bool {
        let (Some(row), Some(base)) = (self.selected, cx.base(self.layers_scroll)) else { return false };
        let (s, r) = (base.scale.max(0.1), base.rect);
        let offset = cx.find::<SkiaScroll>(self.layers_scroll).map_or(self.layers_offset, |sc| sc.viewport_offset_y());
        let top = r.top / s + view::PROPS_PAD + row as f32 * view::ROW + offset;
        let left = r.left / s + view::row_text_left(self.rows[row].depth);
        self.open_layers_menu(drawnui::skia::Rect::new(left, top, left + 120.0, top + view::ROW), cx);
        true
    }

    /// A pick in the Layers menu: it closes, then the action runs.
    pub(crate) fn menu_action(&mut self, action: MenuAction, cx: &mut Cx) {
        self.close_popup(cx);
        self.run_action(action, cx);
    }

    pub(crate) fn run_action(&mut self, action: MenuAction, cx: &mut Cx) {
        match action {
            MenuAction::Undo => self.undo_step(cx),
            MenuAction::Redo => self.redo_step(cx),
            MenuAction::Copy => self.copy_selected(false),
            MenuAction::Paste => self.paste(false),
            MenuAction::Duplicate => self.duplicate(),
            MenuAction::CopyStyle => self.copy_style(),
            MenuAction::PasteStyle => self.paste_style(cx),
            MenuAction::Delete => self.delete_selected(cx),
        }
        cx.items_reset(self.tree_list);
    }

    /// Whether a Layers menu item can act now (a disabled one reads disabled).
    pub(crate) fn action_enabled(&self, action: MenuAction) -> bool {
        let live = self.selected_live();
        match action {
            MenuAction::Undo => self.undo_label().is_some(),
            MenuAction::Redo => self.redo_label().is_some(),
            MenuAction::Copy | MenuAction::CopyStyle => live.is_some(),
            MenuAction::Paste => self.clip.is_some() && self.paste_target(false).is_some(),
            MenuAction::Duplicate => self.paste_target(true).is_some(),
            MenuAction::PasteStyle => self.style.as_ref().is_some_and(|s| !s.is_empty()) && live.is_some(),
            MenuAction::Delete => live.is_some_and(|n| n.parent.is_some() && !self.locked(n)),
        }
    }
}

/// The Layers menu's items, in its order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MenuAction {
    Undo,
    Redo,
    Copy,
    Paste,
    Duplicate,
    CopyStyle,
    PasteStyle,
    Delete,
}

impl MenuAction {
    pub const ALL: [MenuAction; 8] = [MenuAction::Undo, MenuAction::Redo, MenuAction::Copy, MenuAction::Paste, MenuAction::Duplicate, MenuAction::CopyStyle, MenuAction::PasteStyle, MenuAction::Delete];

    /// The key that does it without the menu (W3C code name), with Ctrl (Command on macOS) and Shift.
    pub fn shortcut(self) -> (&'static str, bool, bool) {
        match self {
            MenuAction::Undo => ("KeyZ", true, false),
            MenuAction::Redo => ("KeyY", true, false),
            MenuAction::Copy => ("KeyC", true, false),
            MenuAction::Paste => ("KeyV", true, false),
            MenuAction::Duplicate => ("KeyD", true, false),
            MenuAction::CopyStyle => ("KeyC", true, true),
            MenuAction::PasteStyle => ("KeyV", true, true),
            MenuAction::Delete => ("Delete", false, false),
        }
    }
}

/// Zero-size control the link thread wakes; it drains the inbox in the frame (no polling).
struct Inbox;

impl Control for Inbox {
    fn on_props_changed(&mut self, cx: &mut Cx) {
        cx.postpone_before_draw::<App>(|app, cx| {
            let Some(rx) = app.inbox_rx.take() else { return };
            while let Ok(incoming) = rx.try_recv() {
                app.apply(incoming, cx);
            }
            app.inbox_rx = Some(rx);
            app.write_if_due(cx);
            // The window's close button with changes not in code: the question first.
            #[cfg(not(target_os = "emscripten"))]
            if crate::platform::take_close_asked() && app.confirm.is_none() && !app.changes_first(writes::AfterFlush::Close, cx) {
                app.go_on(writes::AfterFlush::Close, cx);
            }
        });
    }
}

/// Windows lets only the process the user just clicked hand the foreground on: this window has
/// the click, so it allows the app's process to come to the front when it gets `Focus`.
/// Elsewhere the app brings itself forward.
fn allow_foreground(pid: u32) {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(pid);
    }
    #[cfg(not(windows))]
    let _ = pid;
}

/// The flags a flags-enum value holds ("Bold, Italic" -> [Bold, Italic]; "None" -> []).
fn flag_parts(value: &str) -> Vec<String> {
    value.split(',').map(str::trim).filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("None")).map(str::to_owned).collect()
}

/// How far one stepper click moves a number property, or None when it is not a number.
fn numeric_step(type_name: &str, name: &str) -> Option<f64> {
    match type_name {
        "System.Double" | "System.Single" | "System.Decimal" => {
            Some(if matches!(name, "Opacity" | "Scale" | "ScaleX" | "ScaleY" | "AnchorX" | "AnchorY") || name.ends_with("Ratio") { 0.1 } else { 1.0 })
        }
        "System.Int32" | "System.Int64" | "System.Int16" | "System.Byte" => Some(1.0),
        _ => None,
    }
}

/// A number as the agent reads it back: no trailing zeros, no float noise.
fn format_number(v: f64) -> String {
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn short_type(type_name: &str) -> &str {
    type_name.rsplit(['.', '+']).next().unwrap_or(type_name)
}

/// A line for the host's log: the VS Code extension's output channel on the web; nothing on the desktop.
fn log(_text: &str) {
    #[cfg(target_os = "emscripten")]
    crate::web::log(_text);
}

/// The window's keys (the tests use the same): Esc cancels an edit, the field lets go of the
/// keyboard and the value is what it was.
fn keys(ui: Ui<App>) -> Ui<App> {
    ui.on_key_down(|app: &mut App, key, cx| {
        // The menu bar's: Alt and a title's letter, Left and Right in an open menu.
        if !app.on_settings && !app.on_help && app.bar_key(key, cx) {
            return true;
        }
        // Esc during a drag over Layers: nothing lands, the gap slides shut.
        if key.key == "Escape" && app.drag.is_some_and(|d| d.active) {
            app.end_drag(true, cx);
            app.swallow_up = true;
            return true;
        }
        if key.key == "Escape" && app.edit_prop.is_some() {
            app.cancel_edit(cx);
            return true;
        }
        // Esc while a control waits to be placed: nothing is placed.
        if key.key == "Escape" && app.placing.is_some() {
            app.cancel_place();
            return true;
        }
        // Enter in the color picker (its hex field too) or the gradient editor: the color is kept
        // and the picker closes.
        if matches!(key.key, "Enter" | "NumpadEnter") && app.picker.is_some() {
            app.close_picker(cx);
            return true;
        }
        // Esc on Settings or Help goes back to the designer.
        if key.key == "Escape" && (app.on_settings || app.on_help) {
            app.close_page(cx);
            return true;
        }
        // The Layers menu's keys, on the designer page, while no field has the keyboard (a field
        // keeps its own Ctrl+C, Delete...).
        if app.on_settings || app.on_help || cx.focused().is_some_and(|f| cx.find::<SkiaEditor>(f).is_some()) {
            return false;
        }
        let m = key.modifiers();
        // The Menu key and Shift+F10 open it for the selected row.
        if key.key == "ContextMenu" || (key.key == "F10" && m.shift && !m.ctrl && !m.alt) {
            return app.open_layers_menu_at_selection(cx);
        }
        let command = m.ctrl || m.meta;
        // The menu bar's own keys (Restart, Settings, Pick, Help); its Edit keys are the Layers menu's.
        if !m.alt
            && let Some(cmd) = [view::menubar::Cmd::Restart, view::menubar::Cmd::Settings, view::menubar::Cmd::Pick, view::menubar::Cmd::Help].into_iter().find(|c| c.shortcut() == Some((key.key, command, m.shift)))
        {
            cmd.run(app, cx);
            return true;
        }
        // Alt+Up / Alt+Down move the selected control among its siblings.
        if m.alt && !command && !m.shift && matches!(key.key, "ArrowUp" | "ArrowDown") {
            return app.move_selected_by(if key.key == "ArrowUp" { -1 } else { 1 });
        }
        // Ctrl+Shift+Z redoes too (Command+Shift+Z is the macOS one).
        let shortcut = if (key.key, command, m.shift) == ("KeyZ", true, true) { MenuAction::Redo.shortcut() } else { (key.key, command, m.shift) };
        if let Some(action) = MenuAction::ALL.into_iter().find(|a| a.shortcut() == shortcut).filter(|_| !m.alt) {
            if app.action_enabled(action) {
                app.run_action(action, cx);
            }
            return true;
        }
        false
    })
    .on_key_up(|app: &mut App, key, cx| !app.on_settings && !app.on_help && app.bar_key_up(key, cx))
    .on_popup_window_closed(|app: &mut App, id, cx| app.bar_popup_closed(id, cx))
}

fn make() -> Box<dyn drawnui::App> {
    let (to_ui, inbox_rx) = channel();
    // The desktop sends through the socket thread's channel; the web page owns the socket itself.
    #[cfg(not(target_os = "emscripten"))]
    let (to_hub, from_ui): (ToHub, _) = {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (Box::new(move |msg| {
            let _ = tx.send(msg);
        }), rx)
    };
    #[cfg(target_os = "emscripten")]
    let to_hub: ToHub = Box::new(crate::web::send);
    #[allow(unused_mut)]
    let mut settings = Settings::load();
    #[allow(unused_mut)]
    let mut page_widths = (0.0, 0.0);
    // A page that hosts the editor (DrawFiddle) gives its theme in the frame's address, so the first
    // frame is already in it (`?theme=midnight`), and may turn the change animation off (`?animateTheme=0`).
    #[cfg(target_os = "emscripten")]
    {
        if let Some(theme) = crate::web::param("theme") {
            settings.theme = view::Theme::by_id(&theme);
        }
        if crate::web::param("animateTheme").as_deref() == Some("0") {
            settings.instant_changes = true;
        }
        // `?controlsWidth=<px>&propertiesWidth=<px>`: the side panels at start; Layers takes the rest.
        page_widths = (
            crate::web::param("controlsWidth").and_then(|w| w.parse().ok()).unwrap_or(0.0),
            crate::web::param("propertiesWidth").and_then(|w| w.parse().ok()).unwrap_or(0.0),
        );
        // `?writeMode=manual|auto|always`: when the changes made here are written to the page's code.
        if let Some(mode) = crate::web::param("writeMode").as_deref().and_then(WriteMode::from_name) {
            settings.write_mode = mode;
        }
    }
    view::set_theme(settings.theme);
    i18n::set_language(settings.language.as_deref().unwrap_or(i18n::system_language()));
    let state = App {
        screens_w: settings.screens_width,
        properties_w: settings.properties_width,
        page_widths,
        toggled_groups: settings.toggled_groups.iter().cloned().collect(),
        screens_share: settings.screens_share,
        folded_controls: settings.folded_control_groups.iter().cloned().collect(),
        section: settings.section,
        settings,
        persist: true,
        #[cfg(target_os = "emscripten")]
        embedded: crate::web::embedded(),
        #[cfg(target_os = "emscripten")]
        to_page: Some(Box::new(crate::web::post_page)),
        inbox_rx: Some(inbox_rx),
        to_hub: Some(to_hub),
        ..Default::default()
    };
    let mut inbox = Handle::<Inbox>::default();
    let ui = Ui::new(state, |app| {
        let root = view::build(app);
        inbox = app.inbox;
        root
    })
    .font("Default", "assets/OpenSans-Regular.ttf")
    .font("Semibold", "assets/OpenSans-Semibold.ttf")
    .background(view::window_background())
    // Icon buttons show their screen-reader name as a tooltip: one text for both. The look is
    // DrawnUI's default bubble.
    .tooltips_from_accessibility(true);
    let ui = keys(ui)
    // The first size is the moment the window exists: take maximizing away then, once.
    .on_canvas_resized(|app: &mut App, _size, _cx| {
        if !std::mem::replace(&mut app.window_ready, true) {
            #[cfg(not(target_os = "emscripten"))]
            crate::platform::no_maximize();
            #[cfg(not(target_os = "emscripten"))]
            crate::platform::remember_window_place();
        }
    });
    let mut ui = ui;
    ui.state.waker = Some(ui.waker());
    let waker = ui.waker();
    #[cfg(not(target_os = "emscripten"))]
    {
        let waker = waker.clone();
        crate::platform::on_close_asked(move || waker.invalidate(inbox, Dirty::APPLY));
    }
    let wake = move || waker.invalidate(inbox, Dirty::APPLY);
    #[cfg(not(target_os = "emscripten"))]
    crate::net::start(*PORT.get().expect("port set before run"), to_ui, from_ui, wake);
    #[cfg(target_os = "emscripten")]
    crate::web::start(to_ui, wake);
    Box::new(ui)
}

/// Opens the window (desktop) or registers the app for the page to start (web; `port` unused there).
/// The window title; the desktop window is found by it.
pub(crate) const TITLE: &str = "DrawnUI Designer";

pub fn run(port: u16) {
    let _ = PORT.set(port);
    // The last size, so the window opens at it (its place is restored once it exists).
    #[cfg(not(target_os = "emscripten"))]
    if let Some((width, height)) = crate::platform::saved_size() {
        return drawnui::run_sized(TITLE, width, height, make);
    }
    drawnui::run(TITLE, make);
}

/// Renders the window with recorded data, for judging the design without a live app:
/// `cargo test -p designer-desktop --bin drawnui-designer shots -- --ignored` writes
/// target/designer-shots/*.png.
#[cfg(all(test, not(target_os = "emscripten")))]
mod shots {
    use super::*;
    use designer_protocol::{AppInfo, Prop};
    use drawnui::testing::Headless;

    fn assets(path: &str) -> Option<Vec<u8>> {
        std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).ok()
    }

    fn shot(state: App, name: &str) {
        shot_at(state, name, 1.0);
    }

    /// The window at `scale` pixels per point (1.25 = a 125 % Windows display).
    fn shot_at(state: App, name: &str, scale: f32) {
        let font = |name: &str| assets(&format!("assets/{name}")).expect("font file");
        let ui = Ui::new(state, view::build)
            .font_bytes("Default", &font("OpenSans-Regular.ttf"))
            .font_bytes("Semibold", &font("OpenSans-Semibold.ttf"))
            .background(view::window_background());
        let mut host = Headless::new(ui, (1280.0 * scale) as i32, (820.0 * scale) as i32, scale);
        host.settle();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/designer-shots");
        std::fs::create_dir_all(&dir).expect("shots folder");
        std::fs::create_dir_all(&dir).unwrap();
        host.save_png(dir.join(format!("{name}.png")).to_str().unwrap());
    }

    impl App {
        /// The parts of the state a design shot shows, copied for a second shot.
        fn clone_for_shot(&self) -> App {
            let mut app = App { connected: true, apps: self.apps.clone(), app: self.app, screens: self.screens.clone(), ..Default::default() };
            app.set_tree(self.tree.clone());
            app.selected = self.selected;
            app.props = self.props.clone();
            app.rebuild_prop_rows();
            app
        }
    }

    fn connected() -> App {
        connected_to("hellowpf/layouts.json", "HelloWpf")
    }

    /// Inside a page (DrawFiddle) no title bar, Screens or status line: Controls | Layers | Properties.
    #[test]
    fn embedded_shows_controls_in_the_left_column() {
        let mut state = with_controls(connected());
        state.embedded = true;
        let mut host = host(state);
        host.frame_after(16.0);
        let controls = host.rect(host.ui.state.controls_list);
        let tree = host.rect(host.ui.state.tree_list);
        let props = host.rect(host.ui.state.props_list);
        assert!(controls.left < 20.0 && controls.top < 90.0, "Controls at the top left, under its header and search: {controls:?}");
        assert!(tree.left >= 149.0 && tree.top < 40.0, "Layers right of Controls (150 wide in a page), at the top: {tree:?}");
        assert!(props.right >= 1279.0, "Properties at the right edge: {props:?}");
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        assert!(!host.ui.accessibility_nodes().iter().any(|n| n.label == "Screens"), "no Screens");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/designer-shots");
        std::fs::create_dir_all(&dir).unwrap();
        host.save_png(dir.join("27-embedded.png").to_str().unwrap());
    }

    /// In a page the side panels start narrower than on the desktop, so Layers in the middle has the
    /// room (a 770 px frame: Controls 150, Properties 270); the page may set them; a dragged width wins.
    #[test]
    fn in_a_page_layers_gets_the_room() {
        let page = |widths: (f32, f32), dragged: f32| {
            let mut state = with_controls(with_props());
            state.embedded = true;
            state.page_widths = widths;
            state.properties_w = dragged;
            let font = |name: &str| assets(&format!("assets/{name}")).expect("font file");
            let ui = Ui::new(state, view::build).font_bytes("Default", &font("OpenSans-Regular.ttf")).font_bytes("Semibold", &font("OpenSans-Semibold.ttf")).background(view::window_background());
            let mut host = Headless::new(keys(ui), 770, 640, 1.0);
            host.settle();
            host
        };
        let mut host = page((0.0, 0.0), 0.0);
        let (controls, tree, props) = (host.rect(host.ui.state.controls_list), host.rect(host.ui.state.tree_list), host.rect(host.ui.state.props_list));
        assert!((controls.width() - 150.0).abs() < 2.0 && (props.width() - 270.0).abs() < 2.0, "{controls:?} {props:?}");
        assert!(tree.width() >= 300.0, "Layers has the rest: {tree:?}");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/designer-shots");
        std::fs::create_dir_all(&dir).unwrap();
        host.save_png(dir.join("29-embedded-770.png").to_str().unwrap());
        // The page's own widths, and a width dragged since.
        let host = page((180.0, 300.0), 0.0);
        assert!((host.rect(host.ui.state.controls_list).width() - 180.0).abs() < 2.0 && (host.rect(host.ui.state.props_list).width() - 300.0).abs() < 2.0);
        let host = page((180.0, 300.0), 320.0);
        assert!((host.rect(host.ui.state.props_list).width() - 320.0).abs() < 2.0, "dragged: the person's width");
        // The desktop window: as before.
        let desktop = self::host(with_controls(with_props()));
        assert!((desktop.rect(desktop.ui.state.props_list).width() - 340.0).abs() < 2.0);
    }

    /// In a page the changes not in code are a strip of their own under the Layers header; the
    /// header's title and what is on its right never overlap, down to a narrow Layers.
    #[test]
    fn layers_header_and_the_changes_strip_never_overlap() {
        let shot = |props: f32, width: i32, name: &str| {
            let mut state = with_controls(with_props());
            state.embedded = true;
            state.properties_w = props;
            for (node, prop, value) in [(117u64, "FontSize", "30"), (118, "Text", "Changed")] {
                state.live_original.insert((node, prop.into()), "x".into());
                state.pending.insert((node, prop.into()), value.into());
            }
            let font = |name: &str| assets(&format!("assets/{name}")).expect("font file");
            let ui = Ui::new(state, view::build).font_bytes("Default", &font("OpenSans-Regular.ttf")).font_bytes("Semibold", &font("OpenSans-Semibold.ttf")).background(view::window_background());
            let mut host = Headless::new(keys(ui), width, 520, 1.0);
            host.settle();
            host.ui.set_accessibility_enabled(true);
            host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/designer-shots");
            std::fs::create_dir_all(&dir).unwrap();
            host.save_png(dir.join(name).to_str().unwrap());
            host
        };
        for (props, layers, name) in [(270.0, 345.0, "30-layers-345.png"), (355.0, 260.0, "31-layers-260.png")] {
            let host = shot(props, 770, name);
            let tree = host.rect(host.ui.state.layers_scroll);
            assert!((tree.width() - layers).abs() < 12.0, "Layers {layers}: {tree:?}");
            let nodes = host.ui.accessibility_nodes().to_vec();
            let rect = |label: &str| nodes.iter().find(|n| n.label == label && n.rect.left >= tree.left - 1.0).map(|n| n.rect).unwrap_or_else(|| panic!("no {label}"));
            let (title, pick, strip) = (rect("Layers"), rect("Pick a control in the app"), rect("Write all 2 changes to code"));
            assert!(pick.left >= title.right, "{layers}: Pick right of the title: {title:?} {pick:?}");
            assert!(strip.top >= title.bottom && strip.right <= tree.right + 1.0, "{layers}: Write all under the header, inside Layers: {strip:?}");
            assert!(tree.top >= strip.bottom - 1.0, "{layers}: the rows start under the strip: {tree:?} {strip:?}");
        }
        // The narrowest the panels leave Layers in this frame, and a narrow desktop window: looked at by eye.
        shot(480.0, 770, "32-layers-narrow.png");
        let mut desktop = self::host(with_changes(WriteMode::Manual));
        desktop.ui.set_accessibility_enabled(true);
        desktop.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let nodes = desktop.ui.accessibility_nodes().to_vec();
        let at = |label: &str| nodes.iter().find(|n| n.label == label).map(|n| n.rect).unwrap_or_else(|| panic!("no {label}"));
        assert!(at("Pick a control in the app (Ctrl+P)").right <= at("Write all 2 changes to code").left, "the title bar's two sides apart");
        // A narrow desktop window: the left side is cut, Settings and Help keep their place.
        let font = |name: &str| assets(&format!("assets/{name}")).expect("font file");
        let ui = Ui::new(with_changes(WriteMode::Manual), view::build).font_bytes("Default", &font("OpenSans-Regular.ttf")).font_bytes("Semibold", &font("OpenSans-Semibold.ttf")).background(view::window_background());
        let mut narrow = Headless::new(keys(ui), 640, 200, 1.0);
        narrow.settle();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/designer-shots");
        narrow.save_png(dir.join("33-titlebar-640.png").to_str().unwrap());
    }

    /// A control a helper made reads by the helper's name, its type after it, dimmed: in its Layers
    /// row, for screen readers, and in Properties' header.
    #[test]
    fn a_row_reads_the_name_the_code_gives_the_control() {
        let (mut host, _sent, to_ui) = label_selected();
        let revision = host.ui.state.tree.revision;
        feed(&mut host, &to_ui, HubToEditor::MadeBy { app: 1, revision, names: vec![(117, "Heading".into())] });
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let labels: Vec<String> = host.ui.accessibility_nodes().iter().map(|n| n.label.clone()).collect();
        assert!(labels.iter().any(|l| l.starts_with("Heading, SkiaLabel")), "{labels:?}");
        // Properties' header: the name, then the type under it.
        assert_eq!(view::selection_names(&host.ui.state).0, "Heading");
        assert!(view::selection_names(&host.ui.state).1.starts_with("SkiaLabel"), "{:?}", view::selection_names(&host.ui.state));
        // A name for another tree (an older revision) is not taken.
        feed(&mut host, &to_ui, HubToEditor::MadeBy { app: 1, revision: revision + 7, names: vec![(118, "Other".into())] });
        assert!(host.ui.state.tree.nodes.iter().all(|n| n.made_by.as_deref() != Some("Other")));
    }

    /// The controls an app can create, as its agent lists them: some of every group, one with a
    /// group the panel does not know (it goes to Other), and one of the app's own classes.
    fn with_controls(mut app: App) -> App {
        let kind = |t: &str, group: &str, container: bool| ControlKind { type_name: t.into(), group: group.into(), own: false, defaults: vec![], container };
        let mut label = kind("DrawnUi.Draw.SkiaLabel", "Text", false);
        label.defaults = vec![("Text".into(), "Label".into())];
        let mut own = kind("HelloWpf.Pages.ChipCell", "Layout", false);
        own.own = true;
        let kinds = vec![
            label,
            kind("DrawnUi.Draw.SkiaStack", "Layout", true),
            kind("DrawnUi.Draw.SkiaLayer", "Layout", true),
            kind("DrawnUi.Draw.SkiaWrap", "Layout", true),
            kind("DrawnUi.Draw.SkiaGrid", "Layout", true),
            kind("DrawnUi.Draw.SkiaRow", "Layout", true),
            kind("DrawnUi.Draw.SkiaRichLabel", "Text", false),
            kind("DrawnUi.Draw.SkiaMarkdownLabel", "Text", false),
            kind("DrawnUi.Draw.SkiaImage", "Media", false),
            kind("DrawnUi.Draw.SkiaSvg", "Media", false),
            kind("DrawnUi.Draw.SkiaLottie", "Media", false),
            kind("DrawnUi.Draw.SkiaButton", "Input", false),
            kind("DrawnUi.Draw.SkiaSwitch", "Input", false),
            kind("DrawnUi.Draw.SkiaSlider", "Input", false),
            kind("DrawnUi.Draw.SkiaEditor", "Input", false),
            kind("DrawnUi.Draw.SkiaShape", "Shapes", true),
            kind("DrawnUi.Draw.SkiaHotspot", "Gestures", false),
            own,
        ];
        app.controls.insert(app.app.unwrap(), kinds);
        app.rebuild_control_rows();
        app
    }

    /// Hub messages through the inbox, a few frames after (a focused field never lets `settle` end).
    fn feed(host: &mut Headless<App>, to_ui: &std::sync::mpsc::Sender<Incoming>, msg: HubToEditor) {
        to_ui.send(Incoming::Hub(msg)).unwrap();
        host.ui.waker().invalidate(host.ui.state.inbox, Dirty::APPLY);
        for _ in 0..4 {
            host.frame_after(16.0);
        }
    }

    /// What the window sent to the hub, and a channel for what the hub says.
    fn wired(mut state: App) -> (Headless<App>, std::sync::Arc<std::sync::Mutex<Vec<EditorMsg>>>, std::sync::mpsc::Sender<Incoming>) {
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::<EditorMsg>::new()));
        let (to_ui, rx) = std::sync::mpsc::channel();
        state.inbox_rx = Some(rx);
        let log = sent.clone();
        state.to_hub = Some(Box::new(move |m| log.lock().unwrap().push(m)));
        (host(state), sent, to_ui)
    }

    /// Where an accessibility node is, by its name.
    fn at_label(host: &mut Headless<App>, label: &str) -> (f32, f32) {
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let r = host.ui.accessibility_nodes().iter().find(|n| n.label == label).unwrap_or_else(|| panic!("no \"{label}\"")).rect;
        (r.center_x(), r.center_y())
    }

    #[test]
    fn the_controls_panel_lists_the_groups_in_order() {
        let mut host = host(with_controls(connected()));
        let headers: Vec<String> = host.ui.state.control_rows.iter().filter_map(|r| if let CtlRow::Header { group, .. } = r { Some(group.clone()) } else { None }).collect();
        assert_eq!(headers, ["Layout", "Text", "Media", "Input", "Shapes", "Other", "Own"]);
        let names = |app: &App| -> Vec<String> { app.control_rows.iter().filter_map(|r| if let CtlRow::Item(k) = r { Some(short_type(&app.kinds()[*k].type_name).to_owned()) } else { None }).collect() };
        let all = names(&host.ui.state);
        assert_eq!(&all[..5], ["SkiaGrid", "SkiaLayer", "SkiaRow", "SkiaStack", "SkiaWrap"], "A to Z in a group");
        assert_eq!(all.last().map(String::as_str), Some("ChipCell"), "the app's own classes last");
        assert!(all.iter().position(|n| n == "SkiaHotspot") < all.iter().position(|n| n == "ChipCell"), "an unknown group goes to Other");
        // Screen readers: a list named Controls, rows read their name and group, headers fold.
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        host.settle();
        let nodes = host.ui.accessibility_nodes().to_vec();
        assert!(nodes.iter().any(|n| n.role == Aria::LIST && n.label == "Controls"));
        assert!(nodes.iter().any(|n| n.role == Aria::LIST_ITEM && n.label == "SkiaLabel, Text" && n.can_interact), "{:?}", nodes.iter().map(|n| n.label.clone()).collect::<Vec<_>>());
        assert!(nodes.iter().filter(|n| n.can_interact && n.label.trim().is_empty()).count() == 0, "every control that takes input has a name");
        // A header folds its group.
        let (x, y) = at_label(&mut host, "Layout, 5 controls, open");
        host.tap(x, y);
        assert!(!names(&host.ui.state).contains(&"SkiaStack".to_owned()), "Layout folded");
        assert!(host.ui.state.settings.folded_control_groups.contains(&"Layout".to_owned()));
        // The divider over Controls moves it up and down.
        let col = host.rect(host.ui.state.left_column);
        let y = col.top + col.height() / 2.0;
        host.pan((80.0, y), (80.0, y - 100.0), 200.0, 10);
        host.settle();
        let share = host.ui.state.screens_share;
        assert!(share > 0.3 && share < 0.4, "Screens takes less of the column: {share}");
    }

    #[test]
    fn search_filters_the_controls() {
        let mut host = host(with_controls(connected()));
        let field = host.rect(host.ui.state.controls_search_field);
        let at = (field.center_x(), field.center_y());
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        host.type_text("LAB");
        for _ in 0..3 {
            host.frame_after(16.0);
        }
        let rows: Vec<String> = host.ui.state.control_rows.iter().map(|r| match r {
            CtlRow::Header { group, .. } => format!("#{group}"),
            CtlRow::Item(k) => short_type(&host.ui.state.kinds()[*k].type_name).to_owned(),
        }).collect();
        assert_eq!(rows, ["#Text", "SkiaLabel", "SkiaMarkdownLabel", "SkiaRichLabel"], "any case, by name");
        host.type_text("zz");
        host.frame_after(16.0);
        assert!(host.ui.state.control_rows.is_empty(), "nothing matches");
    }

    #[test]
    fn a_click_on_a_control_arms_place_mode() {
        let (mut host, sent, to_ui) = wired(with_controls(connected()));
        let at = at_label(&mut host, "SkiaLabel, Text");
        host.tap(at.0, at.1);
        assert!(sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::ToApp { msg: HubToApp::PlaceMode { type_name: Some(t) }, .. } if t == "DrawnUi.Draw.SkiaLabel")), "{:?}", sent.lock().unwrap());
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let pressed = host.ui.accessibility_nodes().iter().find(|n| n.label == "SkiaLabel, Text").and_then(|n| n.is_pressed);
        assert_eq!(pressed, Some(true), "the row reads pressed");
        assert_eq!(host.ui.state.status, "Click in the app where it goes. Esc cancels.");
        // A second click cancels; another row re-arms; Esc cancels.
        host.tap(at.0, at.1);
        assert!(host.ui.state.placing.is_none() && matches!(sent.lock().unwrap().last(), Some(EditorMsg::ToApp { msg: HubToApp::PlaceMode { type_name: None }, .. })));
        let stack = at_label(&mut host, "SkiaStack, Layout");
        host.tap(stack.0, stack.1);
        assert_eq!(host.ui.state.placing.as_deref(), Some("DrawnUi.Draw.SkiaStack"));
        host.press_key("Escape");
        host.settle();
        assert!(host.ui.state.placing.is_none(), "Esc cancels");
        // The click in the app: the new control is selected once the tree has it, marked as added.
        host.tap(at.0, at.1);
        let mut tree = host.ui.state.tree.clone();
        let parent = 116;
        let after = tree.nodes.iter().position(|n| n.id == parent).unwrap() + 1;
        tree.nodes.insert(after, Node { id: 9999, parent: Some(parent), type_name: "DrawnUi.Draw.SkiaLabel".into(), visible: true, ..Default::default() });
        tree.revision += 1;
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Inserted { request: 0, node: Some(9999), parent: Some(parent), index: Some(0) } });
        assert!(host.ui.state.placing.is_none(), "the click ends place mode");
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Tree(tree) });
        assert_eq!(host.ui.state.selected_node().map(|n| n.id), Some(9999), "the new control is selected");
        assert_eq!(host.ui.state.inserted_state(9999), Some(false), "added, not in code");
        assert!(host.ui.state.status.contains("added in the app"), "{}", host.ui.state.status);
    }

    /// The Layers row of the control `id`, with its Undo and Write (the tests tap them).
    fn added_control(state: App) -> (Headless<App>, std::sync::Arc<std::sync::Mutex<Vec<EditorMsg>>>, std::sync::mpsc::Sender<Incoming>) {
        let (mut host, sent, to_ui) = wired(state);
        let mut tree = host.ui.state.tree.clone();
        let after = tree.nodes.iter().position(|n| n.id == 116).unwrap() + 1;
        tree.nodes.insert(after, Node { id: 9999, parent: Some(116), type_name: "DrawnUi.Draw.SkiaLabel".into(), visible: true, ..Default::default() });
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Inserted { request: 0, node: Some(9999), parent: Some(116), index: Some(0) } });
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Tree(tree) });
        sent.lock().unwrap().clear();
        (host, sent, to_ui)
    }

    /// A narrow Layers panel (265 pt) whose rows are wider than it: an added control's Undo and
    /// Write sit at the right edge of the visible list, stay there while it scrolls sideways, and
    /// act; the Properties header has them too. Screen readers hear what each one does.
    #[test]
    fn a_rows_undo_and_write_stay_in_a_narrow_layers_panel() {
        let mut state = with_controls(connected());
        (state.screens_w, state.properties_w) = (160.0, 1280.0 - 160.0 - 265.0 - 2.0 * 6.0);
        let (mut host, sent, to_ui) = added_control(state);
        let layers = host.rect(host.ui.state.layers_scroll);
        assert!((layers.width() - 265.0).abs() < 20.0, "Layers is about 265 pt: {layers:?}");
        assert!(host.ui.state.layers_width > layers.width(), "its rows are wider than it");
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let buttons = |host: &Headless<App>, label: &str| host.ui.accessibility_nodes().iter().filter(|n| n.label == label && n.role == Aria::BUTTON).map(|n| n.rect).collect::<Vec<_>>();
        let inside = |r: &drawnui::skia::Rect, area: &drawnui::skia::Rect| r.left >= area.left && r.right <= area.right && r.top >= area.top && r.bottom <= area.bottom;
        // The selected (added) row: Undo and Write in the Layers viewport, on the row's line.
        let row = row_of(&host, 9999);
        let row_y = layers.top + view::PROPS_PAD + row as f32 * view::ROW + view::ROW / 2.0;
        for label in ["Undo SkiaLabel", "Write SkiaLabel to code"] {
            let found = buttons(&host, label);
            let in_layers: Vec<_> = found.iter().filter(|r| inside(r, &layers)).collect();
            assert_eq!(in_layers.len(), 1, "{label} once in the visible Layers: {found:?} in {layers:?}");
            assert!((in_layers[0].center_y() - row_y).abs() < 2.0, "{label} on its row");
            assert!(found.iter().any(|r| r.left > layers.right), "{label} in the Properties header too: {found:?}");
        }
        // Scrolled sideways, they stay put.
        let before = buttons(&host, "Write SkiaLabel to code").into_iter().find(|r| inside(r, &layers)).unwrap();
        host.wheel_horizontal(layers.center_x(), layers.center_y(), -200.0);
        host.settle();
        assert!(host.ui.tree.find::<SkiaScroll>(host.ui.state.layers_scroll).is_some_and(|s| s.viewport_offset_x() != 0.0), "the list moved sideways");
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let after = buttons(&host, "Write SkiaLabel to code").into_iter().find(|r| inside(r, &layers)).expect("still in the viewport");
        assert_eq!((before.left, before.top), (after.left, after.top), "pinned to the viewport's right edge");
        // Scrolled down, they follow their row.
        host.wheel(layers.center_x(), layers.center_y(), -1.0);
        host.settle();
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let moved = host.ui.tree.find::<SkiaScroll>(host.ui.state.layers_scroll).map_or(0.0, |s| s.viewport_offset_y());
        assert!(moved != 0.0, "the list moved down");
        let row_rect = host.ui.accessibility_nodes().iter().find(|n| n.role == Aria::LIST_ITEM && n.label.starts_with("SkiaLabel") && n.label.contains(&i18n::tr("row-added"))).map(|n| n.rect).unwrap_or_else(|| panic!("the added row: moved {moved} {:?}", host.ui.accessibility_nodes().iter().filter(|n| n.role == Aria::LIST_ITEM).map(|n| n.label.clone()).collect::<Vec<_>>()));
        let after = buttons(&host, "Write SkiaLabel to code").into_iter().find(|r| inside(r, &layers)).expect("still shown");
        assert!((after.center_y() - row_rect.center_y()).abs() < 2.0, "on its row: {after:?} {row_rect:?}");
        host.wheel(layers.center_x(), layers.center_y(), 1.0);
        host.settle();
        assert_eq!(host.ui.state.layers_offset, 0.0, "back at the top");
        // A click on it writes the control.
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let after = buttons(&host, "Write SkiaLabel to code").into_iter().find(|r| inside(r, &layers)).expect("shown");
        host.tap(after.center_x(), after.center_y());
        assert!(sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::InsertCode { node: 9999, .. })), "{:?}", sent.lock().unwrap());
        let request = *host.ui.state.insert_code.keys().next().unwrap();
        let range = designer_protocol::source::EditorRange::default();
        feed(&mut host, &to_ui, HubToEditor::CodeEdited { request, node: 116, uri: "Page.cs".into(), range });
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        assert!(buttons(&host, "Write SkiaLabel to code").is_empty(), "written: no Write left");
        // The header's Undo takes the write back.
        sent.lock().unwrap().clear();
        let undo = buttons(&host, "Undo SkiaLabel").into_iter().find(|r| r.left > layers.right).expect("the header's Undo");
        host.tap(undo.center_x(), undo.center_y());
        assert!(sent.lock().unwrap().contains(&EditorMsg::UndoCode { request }));
        // Another changed row under the mouse shows its own pair over its text; the mouse can go onto them.
        let (x, y) = row_at(row_of(&host, 117));
        host.tap(x, y);
        host.key_down("Delete", Default::default());
        host.settle();
        let (x, y) = row_at(row_of(&host, 116));
        host.tap(x, y);
        let deleted_y = layers.top + view::PROPS_PAD + row_of(&host, 117) as f32 * view::ROW + view::ROW / 2.0 + host.ui.state.layers_offset;
        host.hover(layers.left + 40.0, deleted_y);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let strip = buttons(&host, "Undo SkiaLabel").into_iter().find(|r| inside(r, &layers) && (r.center_y() - deleted_y).abs() < 2.0).expect("the hovered row's Undo");
        host.hover(strip.center_x(), strip.center_y());
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        assert!(buttons(&host, "Undo SkiaLabel").iter().any(|r| (r.center_y() - deleted_y).abs() < 2.0), "it stays while the mouse is on it");
        sent.lock().unwrap().clear();
        host.tap(strip.center_x(), strip.center_y());
        assert!(sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::Restore { node: 117, .. })), "{:?}", sent.lock().unwrap());
    }

    /// Properties scrolled down: the edit field opens over the row tapped, not where the row would
    /// be unscrolled.
    #[test]
    fn the_edit_field_sits_on_its_row_in_a_scrolled_list() {
        let mut state = with_props();
        state.props_order = PropsOrder::Set;
        for i in 0..111 {
            state.props.push(Prop { name: format!("Other{i:03}"), type_name: "System.Double".into(), value: "0".into(), is_default: Some(true), ..Default::default() });
        }
        state.rebuild_prop_rows();
        let mut host = host(state);
        let list = host.rect(host.ui.state.props_scroll);
        host.wheel(list.center_x(), list.center_y(), -2.0);
        host.settle();
        assert!(host.ui.tree.find::<SkiaScroll>(host.ui.state.props_scroll).is_some_and(|s| s.viewport_offset_y() < 0.0), "scrolled");
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let row = host.ui.accessibility_nodes().iter().find(|n| n.label.starts_with("Other0")).map(|n| n.rect).expect("a row on screen");
        host.pan((row.center_x() + 60.0, row.center_y()), (row.center_x() + 60.0, row.center_y()), 16.0, 1);
        host.frame_after(16.0);
        let field = host.rect(host.ui.state.edit_overlay);
        assert!((field.center_y() - row.center_y()).abs() < 2.0, "over its row: {field:?} {row:?}");
    }

    #[test]
    fn undo_before_writing_removes_the_control_live() {
        let (mut host, sent, _to_ui) = added_control(with_controls(connected()));
        let at = at_label(&mut host, "Undo SkiaLabel");
        host.hover(at.0, at.1);
        host.frame_after(16.0);
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        assert!(sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::ToApp { msg: HubToApp::Remove { node: 9999, .. }, .. })), "{:?}", sent.lock().unwrap());
        assert_eq!(host.ui.state.inserted_state(9999), None);
    }

    #[test]
    fn write_sends_the_added_control_to_code_and_undo_takes_it_back() {
        let (mut host, sent, to_ui) = added_control(with_controls(connected()));
        let at = at_label(&mut host, "Write SkiaLabel to code");
        host.hover(at.0, at.1);
        host.frame_after(16.0);
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        let request = sent.lock().unwrap().iter().find_map(|m| match m {
            EditorMsg::InsertCode { request, node: 9999, parent: 116, index: 0, type_name, defaults, .. } if type_name == "DrawnUi.Draw.SkiaLabel" && defaults == &[("Text".to_owned(), "Label".to_owned())] => Some(*request),
            _ => None,
        });
        let request = request.unwrap_or_else(|| panic!("InsertCode: {:?}", sent.lock().unwrap()));
        let range = designer_protocol::source::EditorRange::default();
        feed(&mut host, &to_ui, HubToEditor::CodeEdited { request, node: 116, uri: "Page.cs".into(), range });
        assert_eq!(host.ui.state.inserted_state(9999), Some(true), "written");
        // Undo after the write: the code first, then the app.
        sent.lock().unwrap().clear();
        let at = at_label(&mut host, "Undo SkiaLabel");
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        assert!(sent.lock().unwrap().contains(&EditorMsg::UndoCode { request }), "{:?}", sent.lock().unwrap());
        feed(&mut host, &to_ui, HubToEditor::CodeUndone { request, uri: "Page.cs".into() });
        assert!(sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::ToApp { msg: HubToApp::Remove { node: 9999, .. }, .. })));
        assert_eq!(host.ui.state.inserted_state(9999), None);
        // A refused write says why in the status line.
        let (mut host, _sent, to_ui) = added_control(with_controls(connected()));
        let at = at_label(&mut host, "Write SkiaLabel to code");
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        let request = *host.ui.state.insert_code.keys().next().unwrap();
        let refusal = designer_protocol::Refusal::new("src-insert-no-list", &[]);
        feed(&mut host, &to_ui, HubToEditor::CodeRefused { request, node: 116, reason: refusal.to_string(), refusal: Some(refusal) });
        assert!(host.ui.state.status_error && host.ui.state.status.starts_with("Not written to code"), "{}", host.ui.state.status);
        assert_eq!(host.ui.state.inserted_state(9999), Some(false), "still only in the app");
    }

    #[test]
    fn a_drag_onto_layers_inserts_where_it_drops() {
        let (mut host, sent, _to_ui) = wired(with_controls(connected()));
        let from = at_label(&mut host, "SkiaLabel, Text");
        let list = host.rect(host.ui.state.layers_scroll);
        let row_of = |host: &Headless<App>, id: u64| host.ui.state.rows.iter().position(|r| host.ui.state.tree.nodes[r.node].id == id).unwrap();
        let y_of = |row: usize, frac: f32| list.top + view::PROPS_PAD + (row as f32 + frac) * view::ROW;
        let inserts = |sent: &std::sync::Mutex<Vec<EditorMsg>>| -> Vec<(u64, Option<u32>, String)> {
            sent.lock().unwrap().iter().filter_map(|m| match m {
                EditorMsg::ToApp { msg: HubToApp::Insert { parent, index, type_name, .. }, .. } => Some((*parent, *index, type_name.clone())),
                _ => None,
            }).collect()
        };
        // Between the first two labels (117, 118) of the page's SkiaStack (116): before 118.
        let between = (list.left + 120.0, y_of(row_of(&host, 118), 0.0));
        host.pan(from, between, 300.0, 12);
        host.frame_after(16.0);
        assert_eq!(inserts(&sent), [(116, Some(1), "DrawnUi.Draw.SkiaLabel".to_owned())]);
        assert!(host.ui.state.drag.is_none() && host.ui.state.drop.is_none() && host.ui.state.placing.is_none(), "a drop is no click");
        // Onto the middle of the SkiaStack's row: in it, last.
        sent.lock().unwrap().clear();
        let onto = (list.left + 120.0, y_of(row_of(&host, 116), 0.5));
        host.pan(from, onto, 300.0, 12);
        host.frame_after(16.0);
        let kids = host.ui.state.tree.nodes.iter().filter(|n| n.parent == Some(116)).count() as u32;
        assert_eq!(inserts(&sent), [(116, Some(kids), "DrawnUi.Draw.SkiaLabel".to_owned())]);
        // Let go outside Layers: nothing.
        sent.lock().unwrap().clear();
        host.pan(from, (from.0 + 40.0, from.1), 300.0, 12);
        host.frame_after(16.0);
        assert!(inserts(&sent).is_empty());
        // Never into a recycled cell's template parent, nor beside its cells.
        let mut state = with_controls(connected());
        let wrap = state.tree.nodes.iter().find(|n| n.type_name.ends_with("SkiaWrap")).map(|n| n.id).unwrap();
        for n in state.tree.nodes.iter_mut().filter(|n| n.parent == Some(wrap)) {
            n.template_index = Some(0);
        }
        let tree = state.tree.clone();
        state.set_tree(tree);
        state.collapsed.clear();
        state.rebuild_rows();
        let row = state.rows.iter().position(|r| state.tree.nodes[r.node].id == wrap).unwrap();
        let drop = state.drop_at((row as f32 + 0.5) * view::ROW);
        assert!(drop.is_none_or(|d| !d.into), "no drop into a template's parent: {drop:?}");
        assert_eq!(state.drop_at((row as f32 + 1.0) * view::ROW + 1.0), None, "nor beside its cells");
    }

    fn ctrl(shift: bool) -> drawnui::Modifiers {
        drawnui::Modifiers { ctrl: true, shift, ..Default::default() }
    }

    /// The point of Layers row `row` (the panel starts after Screens, under the title bar and header).
    fn row_at(row: usize) -> (f32, f32) {
        (160.0 + 160.0, 40.0 + 34.0 + view::PROPS_PAD + row as f32 * view::ROW + view::ROW / 2.0)
    }

    fn row_of(host: &Headless<App>, id: u64) -> usize {
        host.ui.state.rows.iter().position(|r| host.ui.state.tree.nodes[r.node].id == id).unwrap_or_else(|| panic!("no row for {id}"))
    }


    /// The label of the page's SkiaStack (116) selected with its five Set properties, the controls
    /// of `with_controls` (SkiaStack takes children).
    fn label_selected_state() -> App {
        with_controls(with_props())
    }

    fn label_selected() -> (Headless<App>, std::sync::Arc<std::sync::Mutex<Vec<EditorMsg>>>, std::sync::mpsc::Sender<Incoming>) {
        let state = label_selected_state();
        let label = state.selected_node().unwrap().id;
        assert_eq!((label, state.selected_node().unwrap().parent), (117, Some(116)), "the fixture's first label");
        wired(state)
    }

    #[test]
    fn a_right_click_opens_the_layers_menu_at_the_row() {
        let (mut host, _sent, _to_ui) = label_selected();
        let (x, y) = row_at(row_of(&host, 118));
        host.right_click(x, y);
        assert_eq!(host.ui.state.selected_node().map(|n| n.id), Some(118), "the row right-clicked is the one acted on");
        assert!(host.ui.state.popup.is_some());
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let nodes = host.ui.accessibility_nodes().to_vec();
        let menu = nodes.iter().find(|n| n.role == Aria::MENU).expect("a menu");
        assert_eq!(menu.label, "Control actions");
        assert!((menu.rect.left - x).abs() < 2.0 && (menu.rect.top - y).abs() < 2.0, "where the click was: {:?}", menu.rect);
        let items = menu_lines(&mut host);
        let names: Vec<&str> = items.iter().map(|i| i.0.as_str()).collect();
        assert_eq!(names, ["Undo", "Redo", "Copy", "Paste", "Duplicate", "Copy style", "Paste style", "Delete"]);
        let enabled: Vec<bool> = items.iter().map(|i| i.1).collect();
        assert_eq!(enabled, [false, false, true, false, true, true, false, true], "nothing done, nothing copied: Undo, Redo, Paste and Paste style read disabled");
        assert_eq!(items[2].2, "Ctrl+C", "the key is read with the item");
        // The keyboard walks the menu; Esc closes it.
        host.ui.set_keyboard_navigation(true);
        host.press_key("ArrowDown");
        host.press_key("Escape");
        host.settle();
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        assert!(!host.ui.accessibility_nodes().iter().any(|n| n.role == Aria::MENU), "Esc closes the menu");
        // The root cannot be deleted.
        let (x, y) = row_at(0);
        host.right_click(x, y);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        assert!(host.ui.accessibility_nodes().iter().any(|n| n.label == "Delete" && !n.can_interact), "Delete on the root reads disabled");
    }

    #[test]
    fn the_menu_key_and_shift_f10_open_the_menu_for_the_selected_row() {
        let (mut host, _sent, _to_ui) = label_selected();
        host.key_down("ContextMenu", Default::default());
        host.settle();
        assert!(host.ui.state.popup.is_some(), "the Menu key");
        host.press_key("Escape");
        host.settle();
        host.key_down("F10", drawnui::Modifiers { shift: true, ..Default::default() });
        host.settle();
        assert!(host.ui.state.popup.is_some(), "Shift+F10");
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let menu = host.ui.accessibility_nodes().iter().find(|n| n.role == Aria::MENU).map(|n| n.rect).unwrap();
        let (_, y) = row_at(host.ui.state.selected.unwrap());
        assert!(menu.top >= y, "under the selected row: {menu:?}");
    }

    /// A chip as the app reads it: a shape with a label inside.
    fn chip() -> Copied {
        let p = |name: &str, value: &str, t: &str| designer_protocol::SpecProp { name: name.into(), value: value.into(), type_name: t.into() };
        Copied {
            type_name: "DrawnUi.Draw.SkiaShape".into(),
            props: vec![p("CornerRadius", "10", "DrawnUi.Draw.CornerRadius"), p("BackgroundColor", "#FF2B3035", "DrawnUi.Color")],
            children: vec![Copied { type_name: "DrawnUi.Draw.SkiaLabel".into(), props: vec![p("Text", "Start", "System.String")], children: vec![] }],
        }
    }

    /// The app answers the last Copy asked with `spec`.
    fn answer_copy(host: &mut Headless<App>, sent: &std::sync::Mutex<Vec<EditorMsg>>, to_ui: &std::sync::mpsc::Sender<Incoming>, node: u64, spec: Copied) {
        let request = sent.lock().unwrap().iter().rev().find_map(|m| match m {
            EditorMsg::ToApp { msg: HubToApp::Copy { request, node: n }, .. } if *n == node => Some(*request),
            _ => None,
        });
        let request = request.unwrap_or_else(|| panic!("no Copy of {node}: {:?}", sent.lock().unwrap()));
        feed(host, to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Copied { request, spec: Some(spec) } });
    }

    /// The Insert sent last, whole.
    fn last_insert(sent: &std::sync::Mutex<Vec<EditorMsg>>) -> Option<HubToApp> {
        sent.lock().unwrap().iter().rev().find_map(|m| match m {
            EditorMsg::ToApp { msg: msg @ HubToApp::Insert { .. }, .. } => Some(msg.clone()),
            _ => None,
        })
    }

    #[test]
    fn copy_then_paste_inserts_the_whole_control() {
        let (mut host, sent, to_ui) = label_selected();
        // Copy from the menu: the app reads the control with what it holds.
        let (x, y) = row_at(row_of(&host, 117));
        host.right_click(x, y);
        let copy = at_label(&mut host, "Copy");
        host.tap(copy.0, copy.1);
        answer_copy(&mut host, &sent, &to_ui, 117, chip());
        assert_eq!(host.ui.state.clip, Some(chip()));
        assert!(host.ui.state.status.contains("1 control inside"), "{}", host.ui.state.status);
        // Ctrl+V on a leaf: right after it in its parent, with its children.
        host.key_down("KeyV", ctrl(false));
        host.settle();
        let props: Vec<(String, String)> = chip().props.iter().map(|p| (p.name.clone(), p.value.clone())).collect();
        let Some(HubToApp::Insert { parent: 116, index: Some(1), type_name, props: sent_props, children, .. }) = last_insert(&sent) else { panic!("{:?}", last_insert(&sent)) };
        assert_eq!((type_name.as_str(), &sent_props, &children), ("DrawnUi.Draw.SkiaShape", &props, &chip().children));
        // On a container: inside it, last.
        let (x, y) = row_at(row_of(&host, 116));
        host.tap(x, y);
        host.key_down("KeyV", ctrl(false));
        host.settle();
        let kids = host.ui.state.live_children(116);
        assert!(matches!(last_insert(&sent), Some(HubToApp::Insert { parent: 116, index: Some(i), .. }) if i == kids));
        // The copy arrives: added, and Write sends it whole.
        let mut tree = host.ui.state.tree.clone();
        let at = tree.nodes.iter().position(|n| n.id == 117).unwrap();
        tree.nodes.insert(at + 1, Node { id: 9001, parent: Some(116), type_name: "DrawnUi.Draw.SkiaShape".into(), visible: true, ..Default::default() });
        tree.nodes.insert(at + 2, Node { id: 9002, parent: Some(9001), type_name: "DrawnUi.Draw.SkiaLabel".into(), visible: true, ..Default::default() });
        let request = *host.ui.state.paste_props.keys().min().unwrap();
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Inserted { request, node: Some(9001), parent: Some(116), index: Some(1) } });
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Tree(tree) });
        assert_eq!(host.ui.state.row_change(9001), Some((false, false)), "a pasted control is an added one");
        sent.lock().unwrap().clear();
        let write = at_label(&mut host, "Write SkiaShape to code");
        host.pan(write, write, 16.0, 1);
        host.frame_after(16.0);
        let spec = sent.lock().unwrap().iter().find_map(|m| if let EditorMsg::InsertCode { node: 9001, spec, .. } = m { Some(spec.clone()) } else { None });
        assert_eq!(spec, Some(Some(chip())), "InsertCode with the whole copy");
    }

    #[test]
    fn duplicate_pastes_a_copy_right_after() {
        let (mut host, sent, to_ui) = label_selected();
        host.key_down("KeyD", ctrl(false));
        host.settle();
        assert!(last_insert(&sent).is_none(), "nothing before the app read it");
        answer_copy(&mut host, &sent, &to_ui, 117, chip());
        assert!(matches!(last_insert(&sent), Some(HubToApp::Insert { parent: 116, index: Some(1), children, .. }) if children == chip().children), "after it, whole");
        assert!(host.ui.state.clip.is_some(), "Duplicate also copies");
    }

    /// Delete reads the control whole first: Undo puts it back with what it held.
    #[test]
    fn undo_of_a_delete_puts_back_what_it_held() {
        let (mut host, sent, to_ui) = label_selected();
        host.key_down("Delete", Default::default());
        host.settle();
        answer_copy(&mut host, &sent, &to_ui, 117, chip());
        sent.lock().unwrap().clear();
        let undo = at_label(&mut host, "Undo SkiaLabel");
        host.pan(undo, undo, 16.0, 1);
        host.frame_after(16.0);
        let back = sent.lock().unwrap().iter().find_map(|m| if let EditorMsg::Restore { node: 117, children, .. } = m { Some(children.clone()) } else { None });
        assert_eq!(back, Some(chip().children));
    }

    #[test]
    fn copy_style_keeps_the_look_only() {
        for (name, look) in [("TextColor", true), ("FontSize", true), ("CornerRadius", true), ("stroke_width", true), ("Padding", true), ("Opacity", true), ("BackgroundColor", true), ("Text", false), ("Margin", false), ("WidthRequest", false), ("HorizontalOptions", true), ("VerticalOptions", true), ("vertical_options", true), ("HorizontalTextAlignment", true), ("ItemsSource", false), ("ZIndex", false), ("TranslationX", false), ("Row", false)] {
            assert_eq!(is_style_prop(name), look, "{name}");
        }
        let (mut host, _sent, _to_ui) = label_selected();
        host.key_down("KeyC", ctrl(true));
        host.settle();
        let names: Vec<String> = host.ui.state.style.clone().unwrap().into_iter().map(|p| p.0).collect();
        assert_eq!(names, ["FontSize", "HorizontalOptions", "TextColor"], "the look and the alignment, no Text or Margin");
    }

    #[test]
    fn paste_style_sets_what_the_target_has_and_write_all_writes_them() {
        let (mut host, sent, to_ui) = label_selected();
        host.key_down("KeyC", ctrl(true));
        host.settle();
        // The SkiaStack has TextColor and no FontSize: one property set.
        let (x, y) = row_at(row_of(&host, 116));
        host.tap(x, y);
        let prop = |name: &str, t: &str, v: &str| designer_protocol::Prop { name: name.into(), type_name: t.into(), value: v.into(), ..Default::default() };
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Props { node: 116, props: vec![prop("TextColor", "DrawnUi.Color", "#FF000000"), prop("Spacing", "System.Double", "8")] } });
        sent.lock().unwrap().clear();
        host.key_down("KeyV", ctrl(true));
        host.settle();
        let set: Vec<(u64, String, String)> = sent.lock().unwrap().iter().filter_map(|m| if let EditorMsg::ToApp { msg: HubToApp::SetProperty { node, name, value, .. }, .. } = m { Some((*node, name.clone(), value.clone())) } else { None }).collect();
        assert_eq!(set, [(116, "TextColor".to_owned(), "#FFFFFFFF".to_owned())]);
        assert_eq!(host.ui.state.pending_count(), 1, "a live edit with its dot");
        // A target with both: two edits, and Write all writes both.
        let (x, y) = row_at(row_of(&host, 118));
        host.tap(x, y);
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Props { node: 118, props: vec![prop("FontSize", "System.Double", "12"), prop("TextColor", "DrawnUi.Color", "#FF000000"), prop("Text", "System.String", "x")] } });
        host.key_down("KeyV", ctrl(true));
        host.settle();
        assert_eq!(host.ui.state.pending_count(), 2);
        sent.lock().unwrap().clear();
        let write_all = at_label(&mut host, "Write all 2 changes to code");
        host.tap(write_all.0, write_all.1);
        let mut written: Vec<String> = sent.lock().unwrap().iter().filter_map(|m| if let EditorMsg::SetCode { node: 118, prop, .. } = m { Some(prop.clone()) } else { None }).collect();
        written.sort();
        assert_eq!(written, ["FontSize", "TextColor"]);
    }

    #[test]
    fn delete_takes_it_out_live_write_removes_it_from_code_and_undo_puts_it_back() {
        let (mut host, sent, to_ui) = label_selected();
        host.key_down("Delete", Default::default());
        host.settle();
        assert!(sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::ToApp { msg: HubToApp::Remove { node: 117, .. }, .. })), "{:?}", sent.lock().unwrap());
        // The app sends its tree without it: the row stays where it was, dimmed, marked.
        let mut tree = host.ui.state.tree.clone();
        tree.nodes.retain(|n| n.id != 117 && n.parent != Some(117));
        tree.revision += 1;
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Tree(tree) });
        let row = row_of(&host, 117);
        assert_eq!(host.ui.state.tree.nodes[host.ui.state.rows[row].node].parent, Some(116));
        assert_eq!(host.ui.state.live_index(116, 117), 0, "it stays first of the stack");
        assert_eq!(host.ui.state.row_change(117), Some((true, false)));
        assert!(host.ui.state.status.contains("taken out of the app, not in code yet"), "{}", host.ui.state.status);
        // Write: out of the code too.
        sent.lock().unwrap().clear();
        let write = at_label(&mut host, "Write SkiaLabel to code");
        host.pan(write, write, 16.0, 1);
        host.frame_after(16.0);
        let request = sent.lock().unwrap().iter().find_map(|m| if let EditorMsg::RemoveCode { request, node: 117, .. } = m { Some(*request) } else { None }).expect("RemoveCode");
        let range = designer_protocol::source::EditorRange::default();
        feed(&mut host, &to_ui, HubToEditor::CodeEdited { request, node: 117, uri: "Page.cs".into(), range });
        assert_eq!(host.ui.state.row_change(117), Some((true, true)));
        // Undo: the code back, then the control back in the app where it was, with its properties.
        sent.lock().unwrap().clear();
        let undo = at_label(&mut host, "Undo SkiaLabel");
        host.pan(undo, undo, 16.0, 1);
        host.frame_after(16.0);
        assert!(sent.lock().unwrap().contains(&EditorMsg::UndoCode { request }));
        feed(&mut host, &to_ui, HubToEditor::CodeUndone { request, uri: "Page.cs".into() });
        // The hub puts it back where it was (it knows the place) and knows the code has it.
        let back: Vec<(u64, usize)> = sent.lock().unwrap().iter().filter_map(|m| if let EditorMsg::Restore { node, props, .. } = m { Some((*node, props.len())) } else { None }).collect();
        assert_eq!(back, [(117, 5)], "with its five Set properties");
        let insert = *host.ui.state.restore_requests.keys().next().unwrap();
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Inserted { request: insert, node: Some(9100), parent: Some(116), index: Some(0) } });
        assert!(host.ui.state.removed.is_empty(), "its dimmed row goes");
        // Delete then Undo before any write: straight back in the app.
        let (mut host, sent, _to_ui) = label_selected();
        host.key_down("Delete", Default::default());
        host.settle();
        sent.lock().unwrap().clear();
        let undo = at_label(&mut host, "Undo SkiaLabel");
        host.pan(undo, undo, 16.0, 1);
        host.frame_after(16.0);
        assert!(sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::Restore { node: 117, .. })), "put back at once");
        // Screen readers hear what happened to the row.
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let row = host.ui.accessibility_nodes().iter().find(|n| n.role == Aria::LIST_ITEM && n.label.contains("taken out")).map(|n| n.label.clone());
        assert!(row.is_some(), "the row reads taken out");
    }

    /// The SetProperty values sent since the last clear: (node, property, value).
    fn sets(sent: &std::sync::Mutex<Vec<EditorMsg>>) -> Vec<(u64, String, String)> {
        sent.lock().unwrap().iter().filter_map(|m| if let EditorMsg::ToApp { msg: HubToApp::SetProperty { node, name, value, .. }, .. } = m { Some((*node, name.clone(), value.clone())) } else { None }).collect()
    }

    fn undo_names(host: &Headless<App>) -> Vec<String> {
        host.ui.state.histories.get(&1).map(|h| h.undo.iter().map(|s| s.label()).collect()).unwrap_or_default()
    }

    /// Ctrl+Z takes back the last action whatever it was, Ctrl+Y does it again, through the same
    /// messages as the rows' own Undo and Write; a row's Undo takes its entry out of the history.
    #[test]
    fn undo_and_redo_walk_the_history() {
        let (mut host, sent, to_ui) = label_selected();
        let range = designer_protocol::source::EditorRange::default();
        let key = |host: &mut Headless<App>, code: &str, shift: bool| {
            host.key_down(code, ctrl(shift));
            host.settle();
        };
        // 1. An edit: FontSize 28, + twice, Enter: one step.
        let list = host.rect(host.ui.state.props_list);
        let top = list.top + 4.0 + view::ROW + view::ROW / 2.0;
        let at = (1280.0 - 340.0 + 200.0, top);
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        let field = host.rect(host.ui.state.edit_overlay);
        let plus = (field.right - 6.0, field.center_y());
        host.pan(plus, plus, 16.0, 1);
        host.frame_after(16.0);
        host.pan(plus, plus, 16.0, 1);
        host.frame_after(16.0);
        host.press_key("Enter");
        host.settle();
        assert_eq!(undo_names(&host), ["Change FontSize"], "two clicks in one edit are one step");
        // Its look, for later.
        key(&mut host, "KeyC", true);
        // 2. Paste a copy after it.
        key(&mut host, "KeyC", false);
        answer_copy(&mut host, &sent, &to_ui, 117, chip());
        key(&mut host, "KeyV", false);
        let mut tree = host.ui.state.tree.clone();
        let at = tree.nodes.iter().position(|n| n.id == 117).unwrap();
        tree.nodes.insert(at + 1, Node { id: 9001, parent: Some(116), type_name: "DrawnUi.Draw.SkiaShape".into(), visible: true, ..Default::default() });
        let request = *host.ui.state.paste_props.keys().min().unwrap();
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Inserted { request, node: Some(9001), parent: Some(116), index: Some(1) } });
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Tree(tree) });
        // 3. Write it to code.
        let write = at_label(&mut host, "Write SkiaShape to code");
        host.pan(write, write, 16.0, 1);
        host.frame_after(16.0);
        let insert_write = sent.lock().unwrap().iter().find_map(|m| if let EditorMsg::InsertCode { request, node: 9001, .. } = m { Some(*request) } else { None }).expect("InsertCode");
        feed(&mut host, &to_ui, HubToEditor::CodeEdited { request: insert_write, node: 9001, uri: "Page.cs".into(), range });
        // 4. Delete the next label.
        let (x, y) = row_at(row_of(&host, 118));
        host.tap(x, y);
        host.key_down("Delete", Default::default());
        host.settle();
        // 5. Paste style on the third one.
        let (x, y) = row_at(row_of(&host, 119));
        host.tap(x, y);
        let prop = |name: &str, t: &str, v: &str| designer_protocol::Prop { name: name.into(), type_name: t.into(), value: v.into(), ..Default::default() };
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Props { node: 119, props: vec![prop("FontSize", "System.Double", "12"), prop("TextColor", "DrawnUi.Color", "#FF000000")] } });
        key(&mut host, "KeyV", true);
        assert_eq!(undo_names(&host), ["Change FontSize", "Paste SkiaShape", "Write SkiaShape", "Delete SkiaLabel", "Paste style"]);
        assert!(host.ui.state.action_enabled(MenuAction::Undo) && !host.ui.state.action_enabled(MenuAction::Redo));
        assert_eq!(view::action_label(&host.ui.state, MenuAction::Undo), "Undo Paste style", "the menus name the step");

        // Undo all, newest first.
        sent.lock().unwrap().clear();
        key(&mut host, "KeyZ", false);
        assert_eq!(host.ui.state.status, "Undone: Paste style");
        let mut back = sets(&sent);
        back.sort();
        assert_eq!(back, [(119, "FontSize".to_owned(), "12".to_owned()), (119, "TextColor".to_owned(), "#FF000000".to_owned())], "both values back, one step");
        assert_eq!(host.ui.state.pending_count(), 0, "no dots left");
        sent.lock().unwrap().clear();
        key(&mut host, "KeyZ", false);
        assert!(sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::Restore { node: 118, .. })), "the label back: {:?}", sent.lock().unwrap());
        let restore = *host.ui.state.restore_requests.keys().next().unwrap();
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Inserted { request: restore, node: Some(9300), parent: Some(116), index: Some(2) } });
        let mut tree = host.ui.state.tree.clone();
        for n in tree.nodes.iter_mut() {
            if n.id == 118 {
                (n.id, n.visible) = (9300, true);
            }
        }
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Tree(tree) });
        sent.lock().unwrap().clear();
        key(&mut host, "KeyZ", false);
        assert!(sent.lock().unwrap().contains(&EditorMsg::UndoCode { request: insert_write }));
        feed(&mut host, &to_ui, HubToEditor::CodeUndone { request: insert_write, uri: "Page.cs".into() });
        assert!(!sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::ToApp { msg: HubToApp::Remove { .. }, .. })), "only the write goes back");
        assert_eq!(host.ui.state.row_change(9001), Some((false, false)), "added, not written");
        sent.lock().unwrap().clear();
        key(&mut host, "KeyZ", false);
        assert!(sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::ToApp { msg: HubToApp::Remove { node: 9001, .. }, .. })), "the copy out");
        sent.lock().unwrap().clear();
        key(&mut host, "KeyZ", false);
        assert_eq!(sets(&sent), [(117, "FontSize".to_owned(), "28".to_owned())]);
        assert_eq!(host.ui.state.status, "Undone: Change FontSize");
        assert!(undo_names(&host).is_empty() && !host.ui.state.action_enabled(MenuAction::Undo));

        // Redo all, oldest first.
        sent.lock().unwrap().clear();
        key(&mut host, "KeyY", false);
        assert_eq!(sets(&sent), [(117, "FontSize".to_owned(), "30".to_owned())]);
        assert_eq!(host.ui.state.status, "Redone: Change FontSize");
        key(&mut host, "KeyZ", true);
        let Some(HubToApp::Insert { parent: 116, index: Some(1), children, .. }) = last_insert(&sent) else { panic!("{:?}", last_insert(&sent)) };
        assert_eq!(children, chip().children, "Ctrl+Shift+Z: the copy again, whole");
        let again = *host.ui.state.redo_adds.keys().next().unwrap();
        let mut tree = host.ui.state.tree.clone();
        let at = tree.nodes.iter().position(|n| n.id == 117).unwrap();
        tree.nodes.insert(at + 1, Node { id: 9201, parent: Some(116), type_name: "DrawnUi.Draw.SkiaShape".into(), visible: true, ..Default::default() });
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Inserted { request: again, node: Some(9201), parent: Some(116), index: Some(1) } });
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Tree(tree) });
        sent.lock().unwrap().clear();
        key(&mut host, "KeyY", false);
        let rewrite = sent.lock().unwrap().iter().find_map(|m| if let EditorMsg::InsertCode { request, node: 9201, .. } = m { Some(*request) } else { None }).expect("written again, the new id");
        feed(&mut host, &to_ui, HubToEditor::CodeEdited { request: rewrite, node: 9201, uri: "Page.cs".into(), range });
        sent.lock().unwrap().clear();
        key(&mut host, "KeyY", false);
        assert!(sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::ToApp { msg: HubToApp::Remove { node: 9300, .. }, .. })), "the label put back is taken out again: {:?}", sent.lock().unwrap());
        sent.lock().unwrap().clear();
        key(&mut host, "KeyY", false);
        let mut again = sets(&sent);
        again.sort();
        assert_eq!(again, [(119, "FontSize".to_owned(), "30".to_owned()), (119, "TextColor".to_owned(), "#FFFFFFFF".to_owned())]);
        assert_eq!(undo_names(&host), ["Change FontSize", "Paste SkiaShape", "Write SkiaShape", "Delete SkiaLabel", "Paste style"]);
        assert!(!host.ui.state.action_enabled(MenuAction::Redo));

        // The copy's own row Undo (it is written: the write and the copy go): its two steps leave the
        // history, Ctrl+Z goes on with the others.
        let undo = at_label(&mut host, "Undo SkiaShape");
        host.pan(undo, undo, 16.0, 1);
        host.frame_after(16.0);
        assert_eq!(undo_names(&host), ["Change FontSize", "Delete SkiaLabel", "Paste style"]);
        key(&mut host, "KeyZ", false);
        assert_eq!(host.ui.state.status, "Undone: Paste style");
        sent.lock().unwrap().clear();
        key(&mut host, "KeyZ", false);
        assert!(sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::Restore { node: 9300, .. })), "then the delete, not the copy again");
    }

    /// Ctrl+Z on a written value takes back the write only: the value stays live with its dot; the
    /// next Ctrl+Z takes the edit back. A row's own Undo takes its edit out of the history.
    #[test]
    fn undo_of_a_write_keeps_the_value_live() {
        let (mut host, sent, to_ui) = label_selected();
        let range = designer_protocol::source::EditorRange::default();
        let list = host.rect(host.ui.state.props_list);
        let top = list.top + 4.0 + view::ROW + view::ROW / 2.0;
        let at = (1280.0 - 340.0 + 200.0, top);
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        let field = host.rect(host.ui.state.edit_overlay);
        let plus = (field.right - 6.0, field.center_y());
        host.pan(plus, plus, 16.0, 1);
        host.frame_after(16.0);
        host.press_key("Enter");
        host.settle();
        let write_at = (list.right - view::BAR_LANE - 13.0, top);
        host.hover(write_at.0, write_at.1);
        host.frame_after(16.0);
        host.pan(write_at, write_at, 16.0, 1);
        host.frame_after(16.0);
        let request = sent.lock().unwrap().iter().find_map(|m| if let EditorMsg::SetCode { request, prop, .. } = m { (prop == "FontSize").then_some(*request) } else { None }).expect("SetCode");
        feed(&mut host, &to_ui, HubToEditor::CodeEdited { request, node: 117, uri: "Page.cs".into(), range });
        assert_eq!(undo_names(&host), ["Change FontSize", "Write FontSize"]);
        sent.lock().unwrap().clear();
        host.key_down("KeyZ", ctrl(false));
        host.settle();
        assert!(sent.lock().unwrap().contains(&EditorMsg::UndoCode { request }));
        feed(&mut host, &to_ui, HubToEditor::CodeUndone { request, uri: "Page.cs".into() });
        assert_eq!(sets(&sent), [(117, "FontSize".to_owned(), "29".to_owned())], "sent again: it stays live through the rebuild");
        assert_eq!(host.ui.state.pending.get(&(117, "FontSize".to_owned())).map(String::as_str), Some("29"), "its dot is back");
        sent.lock().unwrap().clear();
        host.key_down("KeyZ", ctrl(false));
        host.settle();
        assert_eq!(sets(&sent), [(117, "FontSize".to_owned(), "28".to_owned())]);
        assert!(host.ui.state.pending.is_empty());
        // Redo: the edit, then the write.
        host.key_down("KeyY", ctrl(false));
        host.settle();
        sent.lock().unwrap().clear();
        host.key_down("KeyY", ctrl(false));
        host.settle();
        assert!(sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::SetCode { prop, value, .. } if prop == "FontSize" && value == "29")));
        // A new edit, then the row's Undo: that edit leaves the history, Redo is gone.
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        let field = host.rect(host.ui.state.edit_overlay);
        let plus = (field.right - 6.0, field.center_y());
        host.pan(plus, plus, 16.0, 1);
        host.frame_after(16.0);
        host.press_key("Enter");
        host.settle();
        assert_eq!(undo_names(&host).last().map(String::as_str), Some("Change FontSize"));
        let n = undo_names(&host).len();
        let undo_at = (list.right - view::BAR_LANE - 13.0 - 28.0, top);
        host.pan(undo_at, undo_at, 16.0, 1);
        host.frame_after(16.0);
        assert_eq!(undo_names(&host).len(), n - 1, "the row's Undo took it out: {:?}", undo_names(&host));
    }

    /// A mouse press, moves and (unless `hold`) a release, one frame after each, as a hand drags.
    fn drag_steps(host: &mut Headless<App>, from: (f32, f32), to: (f32, f32), steps: u32, hold: bool) {
        let t = host.time_ms();
        host.ui.pointer(drawnui::PointerKind::Down, from.0, from.1, t);
        host.frame_after(16.0);
        for i in 1..=steps {
            let k = i as f32 / steps as f32;
            let t = host.time_ms();
            host.ui.pointer(drawnui::PointerKind::Move, from.0 + (to.0 - from.0) * k, from.1 + (to.1 - from.1) * k, t);
            host.frame_after(16.0);
        }
        if !hold {
            let t = host.time_ms();
            host.ui.pointer(drawnui::PointerKind::Up, to.0, to.1, t);
            host.frame_after(16.0);
        }
    }

    /// The Move sent last: (node, parent, index).
    fn last_move(sent: &std::sync::Mutex<Vec<EditorMsg>>) -> Option<(u32, u64, u64, u32)> {
        sent.lock().unwrap().iter().rev().find_map(|m| match m {
            EditorMsg::ToApp { msg: HubToApp::Move { request, node, parent, index }, .. } => Some((*request, *node, *parent, *index)),
            _ => None,
        })
    }

    /// The app moves `node` to `index` under `parent` and answers, then sends its tree.
    fn app_moves(host: &mut Headless<App>, to_ui: &std::sync::mpsc::Sender<Incoming>, request: u32, node: u64, parent: u64, index: u32) {
        let mut tree = host.ui.state.tree.clone();
        let inside: std::collections::HashSet<u64> = {
            let mut set: std::collections::HashSet<u64> = [node].into();
            for n in &tree.nodes {
                if n.parent.is_some_and(|p| set.contains(&p)) {
                    set.insert(n.id);
                }
            }
            set
        };
        let mut moving: Vec<Node> = tree.nodes.iter().filter(|n| inside.contains(&n.id)).cloned().collect();
        tree.nodes.retain(|n| !inside.contains(&n.id));
        moving[0].parent = Some(parent);
        let kids: Vec<usize> = tree.nodes.iter().enumerate().filter(|(_, n)| n.parent == Some(parent) && !n.internal).map(|(i, _)| i).collect();
        let at = match kids.get(index as usize) {
            Some(&i) => i,
            None => {
                // After the last child and what it holds.
                let last = kids.last().copied().unwrap_or_else(|| tree.nodes.iter().position(|n| n.id == parent).unwrap());
                let mut held: std::collections::HashSet<u64> = [tree.nodes[last].id].into();
                let mut end = last + 1;
                while end < tree.nodes.len() && tree.nodes[end].parent.is_some_and(|p| held.contains(&p)) {
                    held.insert(tree.nodes[end].id);
                    end += 1;
                }
                if kids.is_empty() { last + 1 } else { end }
            }
        };
        for (k, n) in moving.into_iter().enumerate() {
            tree.nodes.insert(at + k, n);
        }
        tree.revision += 1;
        feed(host, to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Moved { request, node: Some(node), parent: Some(parent), index: Some(index) } });
        feed(host, to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Tree(tree) });
    }

    /// A row dragged with the mouse: lifted, the rows open a gap where it lands, the control moves
    /// live there; its row has the dot, Undo and Write; Write moves it in the code; Ctrl+Z, Ctrl+Y.
    #[test]
    fn a_dragged_row_moves_its_control() {
        let (mut host, sent, to_ui) = label_selected();
        // 117 is the stack 116's first label; 118 and 119 follow. Drag 117 below 119: the gap that
        // opens pushes 119 down a row, so the pointer goes a row further to be below it.
        let from = row_at(row_of(&host, 117));
        let below = row_at(row_of(&host, 119));
        let to = (from.0, below.1 + view::ROW * 1.4);
        drag_steps(&mut host, from, to, 6, true);
        assert_eq!(host.ui.state.dragged_node(), Some(117), "the row is lifted");
        let drop = host.ui.state.drop.expect("a drop under the pointer");
        assert_eq!((drop.parent, drop.into), (116, false));
        // The gap eases open: a few frames later the rows below it are a row lower.
        for _ in 0..20 {
            host.frame_after(16.0);
        }
        let gap_row = host.ui.state.gap.target_pos as usize;
        assert!(host.ui.state.gap.open > 0.95, "open: {:?}", host.ui.state.gap);
        assert!((host.ui.state.row_shift(gap_row) - view::ROW).abs() < 0.5 && host.ui.state.row_shift(gap_row - 1) == 0.0, "the rows from the gap on go down a row");
        // Release: the control moves live, after 119 (index 2 once it left its place).
        let t = host.time_ms();
        host.ui.pointer(drawnui::PointerKind::Up, to.0, to.1, t);
        host.frame_after(16.0);
        let (request, node, parent, index) = last_move(&sent).expect("a Move");
        assert_eq!((node, parent, index), (117, 116, 2));
        assert_eq!(host.ui.state.gap.open, 0.0, "dropped: the gap is gone at once");
        // Another control selected meanwhile: the moved one takes the selection, so the panel's
        // status ("moved in the app") is about the selected control.
        host.ui.state.selected = Some(row_of(&host, 119));
        app_moves(&mut host, &to_ui, request, 117, 116, 2);
        assert_eq!(host.ui.state.selected_node().map(|n| n.id), Some(117), "the moved control is selected");
        assert_eq!(host.ui.state.row_change(117), Some((false, false)), "moved, not written: dot, Undo and Write");
        assert_eq!(host.ui.state.histories.get(&1).map(|h| h.undo.iter().map(|s| s.label()).collect::<Vec<_>>()), Some(vec!["Move SkiaLabel".to_owned()]));
        // Write: the hub moves it in the code to where it is now.
        sent.lock().unwrap().clear();
        host.ui.state.selected = Some(row_of(&host, 117));
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let write = at_label(&mut host, "Write SkiaLabel to code");
        host.tap(write.0, write.1);
        let code = sent.lock().unwrap().iter().find_map(|m| if let EditorMsg::MoveCode { request, node: 117, parent: 116, index: 2, .. } = m { Some(*request) } else { None }).expect("MoveCode");
        let range = designer_protocol::source::EditorRange::default();
        feed(&mut host, &to_ui, HubToEditor::CodeEdited { request: code, node: 117, uri: "Page.cs".into(), range });
        assert_eq!(host.ui.state.row_change(117), Some((false, true)), "written");
        // Ctrl+Z takes the write back (the move stays live), Ctrl+Z again moves it back.
        sent.lock().unwrap().clear();
        host.key_down("KeyZ", ctrl(false));
        host.settle();
        assert!(sent.lock().unwrap().contains(&EditorMsg::UndoCode { request: code }));
        feed(&mut host, &to_ui, HubToEditor::CodeUndone { request: code, uri: "Page.cs".into() });
        assert_eq!(host.ui.state.row_change(117), Some((false, false)));
        host.key_down("KeyZ", ctrl(false));
        host.settle();
        let (request, node, parent, index) = last_move(&sent).expect("moved back");
        assert_eq!((node, parent, index), (117, 116, 0));
        app_moves(&mut host, &to_ui, request, 117, 116, 0);
        assert_eq!(host.ui.state.row_change(117), None, "back home: no mark");
        assert_eq!(host.ui.state.status, "SkiaLabel moved in the app, not in code yet");
        // Ctrl+Y moves it again.
        host.key_down("KeyY", ctrl(false));
        host.settle();
        assert_eq!(last_move(&sent).map(|m| (m.1, m.2, m.3)), Some((117, 116, 2)));
    }

    /// After the app reloads (a hot swap: its controls built again, new ids), nothing is moved with
    /// the old ids: a drag under way ends, a new one drops nowhere, the tree is asked for. With the
    /// new tree, one drag gives one move with its ids; a move the hub puts back after a rebuild
    /// keeps the selection on the moved control.
    #[test]
    fn a_reload_makes_the_old_ids_unusable_until_the_new_tree() {
        let (mut host, sent, to_ui) = label_selected();
        let from = row_at(row_of(&host, 117));
        let below = row_at(row_of(&host, 119));
        let to = (from.0, below.1 + view::ROW * 1.4);
        // The app swaps in mid-drag: the drag ends, nothing lands, the tree is asked for.
        drag_steps(&mut host, from, to, 6, true);
        assert!(host.ui.state.drop.is_some());
        sent.lock().unwrap().clear();
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::HotReload { stage: "reloaded".into() } });
        assert!(host.ui.state.drag.is_none() && host.ui.state.tree_stale, "the drag ends");
        assert!(sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::ToApp { msg: HubToApp::GetTree, .. })), "the new tree is asked for");
        let t = host.time_ms();
        host.ui.pointer(drawnui::PointerKind::Up, to.0, to.1, t);
        host.frame_after(16.0);
        // Before the new tree: a drag drops nowhere, Alt+Down moves nothing.
        drag_steps(&mut host, from, to, 6, false);
        host.key_down("ArrowDown", drawnui::Modifiers { alt: true, ..Default::default() });
        host.settle();
        assert!(last_move(&sent).is_none(), "no move with the old ids");
        // The new tree (every id new): one drag, one move, its ids.
        let mut tree = host.ui.state.tree.clone();
        for n in tree.nodes.iter_mut() {
            n.id += 1000;
            n.parent = n.parent.map(|p| p + 1000);
        }
        tree.revision += 1;
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Tree(tree) });
        assert!(!host.ui.state.tree_stale);
        let from = row_at(row_of(&host, 1117));
        let below = row_at(row_of(&host, 1119));
        drag_steps(&mut host, from, (from.0, below.1 + view::ROW * 1.4), 6, false);
        let moves: Vec<_> = sent.lock().unwrap().iter().filter_map(|m| if let EditorMsg::ToApp { msg: HubToApp::Move { request, node, parent, index }, .. } = m { Some((*request, *node, *parent, *index)) } else { None }).collect();
        assert_eq!(moves.iter().map(|m| (m.1, m.2, m.3)).collect::<Vec<_>>(), [(1117, 1116, 2)], "one move, the new ids");
        // The app moves it; then a rebuild, and the hub moves it again under a new id: the
        // selection follows it.
        let request = moves[0].0;
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Moved { request, node: Some(1117), parent: Some(1116), index: Some(2) } });
        feed(&mut host, &to_ui, HubToEditor::Replayed { app: 1, moved: vec![(1117, 2117)], replaying: vec![], lost: vec![] });
        let mut tree = host.ui.state.tree.clone();
        for n in tree.nodes.iter_mut() {
            if n.id == 1117 {
                n.id = 2117;
            }
        }
        tree.revision += 1;
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Tree(tree) });
        assert_eq!(host.ui.state.selected_node().map(|n| n.id), Some(2117), "the moved control is selected");
    }

    /// Esc during a drag: nothing moves, the gap slides shut, the release is no tap. A row cannot
    /// go into itself; onto a layout row it goes in, last, the row lit.
    #[test]
    fn a_drag_cancelled_or_into_a_layout() {
        let (mut host, sent, _to_ui) = label_selected();
        let from = row_at(row_of(&host, 117));
        let below = row_at(row_of(&host, 119));
        drag_steps(&mut host, from, (from.0, below.1 + view::ROW * 0.4), 6, true);
        for _ in 0..20 {
            host.frame_after(16.0);
        }
        assert!(host.ui.state.gap.open > 0.9);
        host.key_down("Escape", Default::default());
        let mut opens = vec![];
        for _ in 0..20 {
            host.frame_after(16.0);
            opens.push(host.ui.state.gap.open);
        }
        assert!(opens[0] > 0.0 && opens[0] < 0.9, "it slides, it does not jump: {opens:?}");
        assert_eq!(*opens.last().unwrap(), 0.0, "shut");
        let t = host.time_ms();
        host.ui.pointer(drawnui::PointerKind::Up, from.0, below.1, t);
        host.frame_after(16.0);
        assert!(last_move(&sent).is_none(), "nothing moved");
        assert_eq!(host.ui.state.selected_node().map(|n| n.id), Some(117), "the release selected nothing else");
        // A container row: 116 is a stack. 117 dragged onto the middle of the stack's own parent row
        // goes into it; onto itself it goes nowhere.
        let parent = host.ui.state.tree.nodes.iter().find(|n| n.id == 116).and_then(|n| n.parent).unwrap();
        let onto = row_at(row_of(&host, parent));
        drag_steps(&mut host, from, (onto.0, onto.1), 6, true);
        let drop = host.ui.state.drop;
        if let Some(d) = drop {
            assert!(d.into && d.parent == parent, "{d:?}");
        }
        host.key_down("Escape", Default::default());
        host.frame_after(16.0);
        let t = host.time_ms();
        host.ui.pointer(drawnui::PointerKind::Up, onto.0, onto.1, t);
        host.frame_after(16.0);
        // The stack 116 dragged onto its own child 117: no drop.
        let stack = row_at(row_of(&host, 116));
        let over = row_at(row_of(&host, 118));
        drag_steps(&mut host, stack, (stack.0, over.1), 6, true);
        assert!(host.ui.state.drop.is_none_or(|d| d.parent != 116 && host.ui.state.can_move_into(116, d.parent)), "never into itself: {:?}", host.ui.state.drop);
    }

    /// Held near the bottom of Layers, a drag moves the list along; the drop follows.
    #[test]
    fn a_drag_held_at_the_edge_moves_the_list() {
        let (mut host, _sent, _to_ui) = label_selected();
        let layers = host.rect(host.ui.state.layers_scroll);
        let from = row_at(row_of(&host, 117));
        drag_steps(&mut host, from, (from.0, layers.bottom - 8.0), 8, true);
        let before = host.ui.state.drop;
        for _ in 0..30 {
            host.frame_after(16.0);
        }
        assert!(host.ui.state.layers_offset < -50.0, "the list moved down: {}", host.ui.state.layers_offset);
        assert_ne!(host.ui.state.drop, before, "the drop follows the rows under the pointer");
        host.key_down("Escape", Default::default());
        for _ in 0..20 {
            host.frame_after(16.0);
        }
        assert!(!host.ui.state.gap.ticking, "no work once the drag is over");
    }

    /// Dragging in Layers by eye: target/designer-shots/33-drag-*.png (a row carried with the gap
    /// open, a drop into a layout, a moved row with its marks; dark and light).
    #[test]
    #[ignore]
    fn drag_shot() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/designer-shots");
        std::fs::create_dir_all(&dir).unwrap();
        for theme in ["dark", "light"] {
            view::set_theme(view::Theme::by_id(theme));
            let mut state = label_selected_state();
            state.settings.theme = view::Theme::by_id(theme);
            let (mut host, sent, to_ui) = wired(state);
            let save = |host: &mut Headless<App>, name: &str| host.save_png(dir.join(format!("33-drag-{name}-{theme}.png")).to_str().unwrap());
            let from = row_at(row_of(&host, 117));
            let below = row_at(row_of(&host, 119));
            drag_steps(&mut host, from, (from.0 + 30.0, below.1 + view::ROW * 1.4), 6, true);
            for _ in 0..20 {
                host.frame_after(16.0);
            }
            save(&mut host, "gap");
            host.key_down("Escape", Default::default());
            for _ in 0..20 {
                host.frame_after(16.0);
            }
            let t = host.time_ms();
            host.ui.pointer(drawnui::PointerKind::Up, from.0, from.1, t);
            host.frame_after(16.0);
            // Onto the shape row after the labels: into it, last.
            let shape = host.ui.state.rows.iter().position(|r| host.ui.state.tree.nodes[r.node].type_name.ends_with("SkiaShape")).unwrap();
            let onto = row_at(shape);
            // The gap that opens on the way pushes the shape's row down a row.
            drag_steps(&mut host, from, (onto.0 + 30.0, onto.1 + view::ROW), 6, true);
            for _ in 0..20 {
                host.frame_after(16.0);
            }
            save(&mut host, "into");
            host.key_down("Escape", Default::default());
            for _ in 0..20 {
                host.frame_after(16.0);
            }
            let t = host.time_ms();
            host.ui.pointer(drawnui::PointerKind::Up, from.0, from.1, t);
            host.frame_after(16.0);
            // Moved: the row's dot, Undo and Write.
            host.key_down("ArrowDown", drawnui::Modifiers { alt: true, ..Default::default() });
            host.settle();
            let (request, node, parent, index) = last_move(&sent).unwrap();
            app_moves(&mut host, &to_ui, request, node, parent, index);
            host.ui.state.selected = Some(row_of(&host, 117));
            for _ in 0..3 {
                host.frame_after(16.0);
            }
            save(&mut host, "moved");
        }
        view::set_theme(view::Theme::by_id("dark"));
    }

    /// Dragged across rows and layouts and held at the bottom edge while the list moves, frame by
    /// frame: one gap at most, at the indentation the control lands at; the bar on a layout only
    /// while it is the target, on that layout's row; nothing left from an earlier target.
    #[test]
    fn one_drop_mark_at_the_right_indentation_every_frame() {
        let (mut host, _sent, _to_ui) = label_selected();
        let layers = host.rect(host.ui.state.layers_scroll);
        let from = row_at(row_of(&host, 117));
        let check = |host: &Headless<App>, frame: usize| {
            let st = &host.ui.state;
            let visible = |h: Handle<SkiaShape>| host.ui.tree.base(h).is_some_and(|b| b.p.is_visible);
            let gap = st.gap;
            assert_eq!(visible(st.gap_box), gap.open > 0.05, "frame {frame}: the gap shows while it is open");
            if visible(st.gap_box) {
                let r = host.rect(st.gap_box);
                let want = layers.left + view::gap_left(gap.depth) + st.layers_offset_x;
                assert!((r.left - want).abs() < 1.5, "frame {frame}: the gap at depth {}: {} vs {want}", gap.depth, r.left);
                let depth = st.drop.map(|d| if d.into { d.depth + 1 } else { d.depth });
                if let Some(depth) = depth {
                    assert_eq!(gap.depth, depth, "frame {frame}: the depth of the drop now");
                }
            }
            let into = st.drop.filter(|d| d.into && gap.open > 0.05);
            assert_eq!(visible(st.into_bar), into.is_some(), "frame {frame}: the bar only for a drop into a layout");
            if let Some(d) = into {
                let r = host.rect(st.into_bar);
                let row_top = layers.top + view::PROPS_PAD + d.row as f32 * view::ROW + st.layers_offset + st.row_shift(d.row);
                assert!(r.top >= row_top - 0.5 && r.bottom <= row_top + view::ROW + 0.5, "frame {frame}: the line on the layout's row: {r:?} row at {row_top}");
                assert!((r.left - (layers.left + view::row_text_left(d.depth) + st.layers_offset_x)).abs() < 1.5, "frame {frame}: the line at the layout's indentation");
            }
        };
        let t = host.time_ms();
        host.ui.pointer(drawnui::PointerKind::Down, from.0, from.1, t);
        host.frame_after(16.0);
        // Down the list a few points per frame (over labels, a shape, a stack, their children), then
        // held in the bottom edge zone while the list moves along.
        let mut y = from.1;
        let mut frame = 0;
        let mut intos = 0;
        while y < layers.bottom - 6.0 {
            y += 5.0;
            let t = host.time_ms();
            host.ui.pointer(drawnui::PointerKind::Move, from.0 + 20.0, y, t);
            host.frame_after(16.0);
            check(&host, frame);
            intos += usize::from(host.ui.state.drop.is_some_and(|d| d.into));
            frame += 1;
        }
        for _ in 0..40 {
            host.frame_after(16.0);
            check(&host, frame);
            frame += 1;
        }
        assert!(intos > 0, "the path went over a layout");
        assert!(host.ui.state.layers_offset < 0.0, "the list moved along");
        host.key_down("Escape", Default::default());
        for _ in 0..25 {
            host.frame_after(16.0);
            check(&host, frame);
            frame += 1;
        }
    }

    /// A drop into a layout by eye: target/designer-shots/34-drop-into-*.png.
    #[test]
    #[ignore]
    fn drop_into_shot() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/designer-shots");
        std::fs::create_dir_all(&dir).unwrap();
        for theme in ["dark", "light"] {
            view::set_theme(view::Theme::by_id(theme));
            let mut state = label_selected_state();
            state.settings.theme = view::Theme::by_id(theme);
            let (mut host, _sent, _to_ui) = wired(state);
            let from = row_at(row_of(&host, 117));
            let shape = host.ui.state.rows.iter().position(|r| host.ui.state.tree.nodes[r.node].type_name.ends_with("SkiaShape")).unwrap();
            let onto = row_at(shape);
            drag_steps(&mut host, from, (onto.0 + 30.0, onto.1 + view::ROW), 6, true);
            for _ in 0..20 {
                host.frame_after(16.0);
            }
            assert!(host.ui.state.drop.is_some_and(|d| d.into));
            host.save_png(dir.join(format!("34-drop-into-{theme}.png")).to_str().unwrap());
        }
        view::set_theme(view::Theme::by_id("dark"));
    }

    /// Alt+Up and Alt+Down move the selected control among its siblings.
    #[test]
    fn alt_arrows_move_the_selected_control() {
        let (mut host, sent, _to_ui) = label_selected();
        host.key_down("ArrowDown", drawnui::Modifiers { alt: true, ..Default::default() });
        host.settle();
        assert_eq!(last_move(&sent).map(|m| (m.1, m.2, m.3)), Some((117, 116, 1)));
        host.key_down("ArrowUp", drawnui::Modifiers { alt: true, ..Default::default() });
        host.settle();
        assert_eq!(sent.lock().unwrap().iter().filter(|m| matches!(m, EditorMsg::ToApp { msg: HubToApp::Move { .. }, .. })).count(), 1, "first already: Up does nothing");
    }

    /// A part the library made for itself (a button's caption), shown with "Show library parts": its
    /// properties stay editable, its place does not. No Delete, Paste, Duplicate, Alt+arrows or drag
    /// on it, nothing dropped beside or into it, and the menus read those lines unavailable.
    #[test]
    fn a_library_part_keeps_its_place() {
        let mut state = label_selected_state();
        state.settings.show_internal = true;
        let mut tree = state.tree.clone();
        tree.nodes.iter_mut().filter(|n| n.id == 117).for_each(|n| n.internal = true);
        state.set_tree(tree);
        state.clip = Some(Copied { type_name: "DrawnUi.Draw.SkiaLabel".into(), props: vec![], children: vec![] });
        let (mut host, sent, _to_ui) = wired(state);
        assert_eq!(host.ui.state.selected_node().map(|n| n.id), Some(117));
        // Copy and Copy style only (nothing done yet, no style copied), in the right-click menu and Edit.
        let only_copy = [false, false, true, false, false, true, false, false];
        let (x, y) = row_at(row_of(&host, 117));
        host.right_click(x, y);
        assert_eq!(menu_lines(&mut host).iter().map(|l| l.1).collect::<Vec<_>>(), only_copy);
        host.press_key("Escape");
        host.settle();
        open_bar(&mut host, 1);
        assert_eq!(menu_lines(&mut host).iter().map(|l| l.1).collect::<Vec<_>>(), only_copy);
        host.press_key("Escape");
        host.settle();
        // Its keys do nothing.
        sent.lock().unwrap().clear();
        for (key, m) in [("Delete", Default::default()), ("ArrowDown", drawnui::Modifiers { alt: true, ..Default::default() }), ("KeyV", ctrl(false)), ("KeyD", ctrl(false))] {
            host.key_down(key, m);
            host.settle();
        }
        // Its row does not lift.
        let below = row_at(row_of(&host, 119));
        drag_steps(&mut host, (x, y), (x, below.1 + view::ROW * 0.4), 6, false);
        assert_eq!(host.ui.state.dragged_node(), None);
        let changed = |m: &EditorMsg| matches!(m, EditorMsg::ToApp { msg: HubToApp::Remove { .. } | HubToApp::Move { .. } | HubToApp::Insert { .. } | HubToApp::Copy { .. }, .. });
        assert!(!sent.lock().unwrap().iter().any(changed), "{:?}", sent.lock().unwrap());
        // Nothing lands right before it, nor into a part that holds children.
        let r = row_of(&host, 117) as f32;
        assert_eq!(host.ui.state.drop_at((r + 0.1) * view::ROW), None);
        assert!(host.ui.state.accepts_children(116));
        host.ui.state.tree.nodes.iter_mut().filter(|n| n.id == 116).for_each(|n| n.internal = true);
        assert!(!host.ui.state.accepts_children(116));
        let r = row_of(&host, 116) as f32;
        assert!(host.ui.state.drop_at((r + 0.5) * view::ROW).is_none_or(|d| d.parent != 116), "not into it");
    }

    /// A hot reload builds the screen again from the code: what was changed here and not written is
    /// gone from the app, so its marks go too; what was written stays marked.
    #[test]
    fn a_rebuild_forgets_what_was_not_written() {
        let (mut host, _sent, to_ui) = label_selected();
        // Added 9001 (not written), 9002 (written); 118 taken out (not written), 119 taken out (written).
        let mut tree = host.ui.state.tree.clone();
        let at = tree.nodes.iter().position(|n| n.id == 117).unwrap();
        tree.nodes.insert(at + 1, Node { id: 9001, parent: Some(116), type_name: "DrawnUi.Draw.SkiaLabel".into(), visible: true, ..Default::default() });
        tree.nodes.insert(at + 2, Node { id: 9002, parent: Some(116), type_name: "DrawnUi.Draw.SkiaLabel".into(), visible: true, ..Default::default() });
        for (request, id) in [(1u32, 9001u64), (2, 9002)] {
            host.ui.state.insert_requests.insert(request, "DrawnUi.Draw.SkiaLabel".into());
            feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Inserted { request, node: Some(id), parent: Some(116), index: Some(1) } });
            feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Tree(tree.clone()) });
        }
        // Write on 9002's row: the second Write (9001's row comes first).
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let writes: Vec<drawnui::skia::Rect> = host.ui.accessibility_nodes().iter().filter(|n| n.label == "Write SkiaLabel to code").map(|n| n.rect).collect();
        host.pan((writes[1].center_x(), writes[1].center_y()), (writes[1].center_x(), writes[1].center_y()), 16.0, 1);
        host.frame_after(16.0);
        let request = *host.ui.state.insert_code.iter().find(|(_, id)| **id == 9002).map(|(r, _)| r).expect("9002 written");
        feed(&mut host, &to_ui, HubToEditor::CodeEdited { request, node: 116, uri: "Page.cs".into(), range: Default::default() });
        // 119 out and written (its Write: the second one, after 9001's), then 118 out.
        let (x, y) = row_at(row_of(&host, 119));
        host.tap(x, y);
        host.key_down("Delete", Default::default());
        host.settle();
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let writes: Vec<drawnui::skia::Rect> = host.ui.accessibility_nodes().iter().filter(|n| n.label == "Write SkiaLabel to code").map(|n| n.rect).collect();
        host.pan((writes[1].center_x(), writes[1].center_y()), (writes[1].center_x(), writes[1].center_y()), 16.0, 1);
        host.frame_after(16.0);
        let request = *host.ui.state.remove_code.iter().find(|(_, id)| **id == 119).map(|(r, _)| r).expect("119 written");
        feed(&mut host, &to_ui, HubToEditor::CodeEdited { request, node: 119, uri: "Page.cs".into(), range: Default::default() });
        let (x, y) = row_at(row_of(&host, 118));
        host.tap(x, y);
        host.key_down("Delete", Default::default());
        host.settle();
        let marks = |h: &Headless<App>| [9001u64, 9002, 118, 119].map(|id| h.ui.state.row_change(id));
        assert_eq!(marks(&host), [Some((false, false)), Some((false, true)), Some((true, false)), Some((true, true))]);
        // A tree from the app that is only newer (same ids): every mark stays.
        let mut same = host.ui.state.tree.clone();
        same.nodes.retain(|n| !host.ui.state.is_removed(n.id));
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Tree(same.clone()) });
        assert_eq!(marks(&host), [Some((false, false)), Some((false, true)), Some((true, false)), Some((true, true))]);
        // Rebuilt from the code: new ids. The code has 9002 (written) and 118 (its removal not
        // written), not 9001 nor 119.
        let mut rebuilt = same.clone();
        rebuilt.nodes.retain(|n| n.id != 9001);
        let at = rebuilt.nodes.iter().position(|n| n.id == 9002).unwrap();
        rebuilt.nodes.insert(at + 1, Node { id: 118, parent: Some(116), type_name: "DrawnUi.Draw.SkiaLabel".into(), visible: true, ..Default::default() });
        // Every control under the shell's own layers gets a new id.
        let kept = [1u64, 2, 3];
        for n in rebuilt.nodes.iter_mut().filter(|n| !kept.contains(&n.id)) {
            n.id += 50_000;
            n.parent = n.parent.map(|p| if kept.contains(&p) { p } else { p + 50_000 });
        }
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Tree(rebuilt) });
        let state = &host.ui.state;
        assert!(state.row_change(9001 + 50_000).is_none() && state.row_change(9001).is_none(), "the unwritten add is gone");
        assert_eq!(state.row_change(9002 + 50_000), Some((false, true)), "the written add is still marked, by its place");
        assert!(!state.removed.iter().any(|r| r.node.id == 118), "the unwritten removal is gone: the code brought it back");
        assert_eq!(state.row_change(119), Some((true, true)), "the written removal keeps its row, with Undo");
        assert_eq!(state.tree.nodes.iter().find(|n| n.id == 119).and_then(|n| n.parent), Some(116 + 50_000), "under the rebuilt stack");
    }

    /// The hub put the changes not written back onto the rebuilt controls: their dots, Undo and
    /// Write move to the new ids; an add on its way back keeps its mark; what is lost is said.
    #[test]
    fn marks_follow_the_changes_the_hub_puts_back() {
        let (mut host, _sent, to_ui) = label_selected();
        host.ui.state.pending.insert((118, "TextColor".into()), "#FFFF0000".into());
        host.ui.state.live_original.insert((118, "TextColor".into()), "#FFFFFFFF".into());
        host.ui.state.inserted.insert("#9001".into(), Inserted::default());
        // The rebuilt tree: every control of the page with a new id (+50000), 9001 not back yet.
        let mut tree = host.ui.state.tree.clone();
        let kept = [1u64, 2, 3];
        for n in tree.nodes.iter_mut().filter(|n| !kept.contains(&n.id)) {
            n.id += 50_000;
            n.parent = n.parent.map(|p| if kept.contains(&p) { p } else { p + 50_000 });
        }
        feed(&mut host, &to_ui, HubToEditor::Replayed { app: 1, moved: vec![(118, 50_118)], replaying: vec![9001], lost: vec![] });
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Tree(tree.clone()) });
        let state = &host.ui.state;
        assert_eq!(state.pending.get(&(50_118, "TextColor".into())).map(String::as_str), Some("#FFFF0000"), "the dot moved");
        assert!(state.live_original.contains_key(&(50_118, "TextColor".into())), "Undo moved");
        assert!(state.inserted.contains_key("#9001"), "the add on its way back keeps its mark");
        // It arrives.
        feed(&mut host, &to_ui, HubToEditor::Replayed { app: 1, moved: vec![(9001, 9101)], replaying: vec![], lost: vec![] });
        assert!(host.ui.state.inserted.contains_key("#9101") && !host.ui.state.replaying.contains(&9001));
        // Lost: said in the status line.
        let lost = designer_protocol::Refusal::new("hub-lost-prop", &[("type", &"SkiaLabel"), ("prop", &"FontSize")]);
        feed(&mut host, &to_ui, HubToEditor::Replayed { app: 1, moved: vec![], replaying: vec![], lost: vec![lost] });
        assert_eq!(host.ui.state.status, "1 change could not be kept: SkiaLabel FontSize: its control is not in the code any more");
        assert!(host.ui.state.status_error);
    }

    /// The Layers menu by eye: target/designer-shots/29-layers-menu*.png.
    #[test]
    #[ignore]
    fn layers_menu_shot() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/designer-shots");
        std::fs::create_dir_all(&dir).unwrap();
        for theme in ["dark", "light"] {
            view::set_theme(view::Theme::by_id(theme));
            let mut state = with_controls(with_props());
            state.settings.theme = view::Theme::by_id(theme);
            state.clip = Some(Copied { type_name: "DrawnUi.Draw.SkiaLabel".into(), ..Default::default() });
            let mut host = host(state);
            let (x, y) = row_at(row_of(&host, 117));
            host.right_click(x, y);
            let paste = line_at(&mut host, "Paste");
            host.hover(paste.0, paste.1);
            for _ in 0..3 {
                host.frame_after(16.0);
            }
            host.save_png(dir.join(format!("29-layers-menu-{theme}.png")).to_str().unwrap());
        }
        view::set_theme(view::Theme::by_id("dark"));
        // A deleted control's row, and Write all after Paste style.
        let (mut host, _sent, _to_ui) = label_selected();
        host.key_down("KeyC", ctrl(true));
        host.key_down("Delete", Default::default());
        host.settle();
        host.save_png(dir.join("29-layers-deleted.png").to_str().unwrap());
        // Paste style on a label: two live edits, Write all in the Properties header.
        let (mut host, _sent, to_ui) = label_selected();
        host.key_down("KeyC", ctrl(true));
        let (x, y) = row_at(row_of(&host, 118));
        host.tap(x, y);
        let props = host.ui.state.tree.nodes.iter().find(|n| n.id == 118).map(|_| with_props().props).unwrap();
        feed(&mut host, &to_ui, HubToEditor::FromApp { app: 1, msg: AppMsg::Props { node: 118, props } });
        host.key_down("KeyV", ctrl(true));
        host.settle();
        host.save_png(dir.join("29-write-all.png").to_str().unwrap());
    }

    /// Undo and Write in a narrow Layers panel by eye: target/designer-shots/31-layers-actions.png
    /// (an added control selected, a deleted one under the mouse, the list scrolled sideways).
    #[test]
    #[ignore]
    fn layers_actions_shot() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/designer-shots");
        std::fs::create_dir_all(&dir).unwrap();
        let mut state = with_controls(connected());
        (state.screens_w, state.properties_w) = (160.0, 1280.0 - 160.0 - 265.0 - 12.0);
        let (mut host, _sent, _to_ui) = added_control(state);
        let (x, y) = row_at(row_of(&host, 118));
        host.tap(x, y);
        host.key_down("Delete", Default::default());
        host.settle();
        let (x, y) = row_at(row_of(&host, 9999));
        host.tap(x, y);
        let layers = host.rect(host.ui.state.layers_scroll);
        host.wheel_horizontal(layers.center_x(), layers.center_y(), -1.0);
        host.settle();
        let (_, y) = row_at(row_of(&host, 118));
        host.hover(layers.left + 60.0, y);
        for _ in 0..3 {
            host.frame_after(16.0);
        }
        host.save_png(dir.join("31-layers-actions.png").to_str().unwrap());
    }

    /// The menu bar by eye: target/designer-shots/30-menubar-*.png (Edit open, a line lit; View
    /// with its order submenu; File with a recent app; the light theme).
    #[test]
    #[ignore]
    fn menubar_shot() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/designer-shots");
        std::fs::create_dir_all(&dir).unwrap();
        for theme in ["dark", "light"] {
            view::set_theme(view::Theme::by_id(theme));
            let mut state = with_controls(with_props());
            state.settings.theme = view::Theme::by_id(theme);
            state.apps[0].info.project = Some("C:/apps/HelloWpf/HelloWpf.csproj".into());
            state.recent = vec![designer_protocol::RecentApp { name: "HelloOld".into(), engine: "dotnet-wpf".into(), project: "C:/apps/HelloOld/HelloOld.csproj".into(), agent: None, last_seen: 0, platform: None }];
            state.clip = Some(Copied { type_name: "DrawnUi.Draw.SkiaLabel".into(), ..Default::default() });
            let mut host = host(state);
            let save = |host: &mut Headless<App>, name: &str| {
                for _ in 0..3 {
                    host.frame_after(16.0);
                }
                host.save_png(dir.join(format!("30-menubar-{name}-{theme}.png")).to_str().unwrap());
            };
            open_bar(&mut host, 1);
            let paste = line_at(&mut host, "Paste");
            host.hover(paste.0, paste.1);
            save(&mut host, "edit");
            if theme == "light" {
                continue;
            }
            host.press_key("Escape");
            host.settle();
            open_bar(&mut host, 2);
            let order = line_at(&mut host, "Properties order");
            host.hover(order.0, order.1);
            host.settle();
            save(&mut host, "view");
            host.tap(640.0, 600.0);
            host.settle();
            open_bar(&mut host, 0);
            let run = line_at(&mut host, "Run app");
            host.hover(run.0, run.1);
            host.settle();
            save(&mut host, "file");
        }
        view::set_theme(view::Theme::by_id("dark"));
    }

    /// The Controls panel by eye: target/designer-shots/28-controls*.png (a row hovered, place mode,
    /// a drag over Layers, an added control, the light theme).
    #[test]
    #[ignore]
    fn controls_shot() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/designer-shots");
        std::fs::create_dir_all(&dir).unwrap();
        let save = |host: &mut Headless<App>, name: &str| {
            for _ in 0..3 {
                host.frame_after(16.0);
            }
            host.save_png(dir.join(format!("{name}.png")).to_str().unwrap());
        };
        for theme in ["dark", "light"] {
            view::set_theme(view::Theme::by_id(theme));
            let mut state = with_controls(with_props());
            state.settings.theme = view::Theme::by_id(theme);
            let mut host = host(state);
            let at = at_label(&mut host, "SkiaStack, Layout");
            host.hover(at.0, at.1);
            save(&mut host, &format!("28-controls-{theme}"));
        }
        view::set_theme(view::Theme::by_id("dark"));
        // Place mode, nothing selected: Properties says how to finish.
        let (mut host, _sent, _to_ui) = wired(with_controls(connected()));
        let at = at_label(&mut host, "SkiaLabel, Text");
        host.tap(at.0, at.1);
        save(&mut host, "28-controls-placing");
        // A drag over Layers, between two rows and onto a container.
        let list = host.rect(host.ui.state.layers_scroll);
        let row = host.ui.state.rows.iter().position(|r| host.ui.state.tree.nodes[r.node].id == 118).unwrap();
        let (x, y) = (list.left + 150.0, list.top + view::PROPS_PAD + row as f32 * view::ROW);
        host.ui.pointer_button(drawnui::PointerKind::Down, MouseButton::Left, at.0, at.1, host.time_ms());
        host.frame_after(16.0);
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            host.ui.pointer_button(drawnui::PointerKind::Move, MouseButton::Left, at.0 + (x - at.0) * t, at.1 + (y - at.1) * t, host.time_ms());
            host.frame_after(16.0);
        }
        save(&mut host, "28-controls-drag-between");
        let into = y - 2.0 * view::ROW + view::ROW / 2.0;
        host.ui.pointer_button(drawnui::PointerKind::Move, MouseButton::Left, x, into, host.time_ms());
        save(&mut host, "28-controls-drag-into");
        // An added control, not in code yet: dot, Undo, Write.
        let (mut host, _sent, _to_ui) = added_control(with_controls(connected()));
        save(&mut host, "28-controls-added");
        shot_at(with_controls(with_props()), "28-controls-125", 1.25);
    }

    /// A connected app showing the recorded tree `fixture` (under fixtures/).
    fn connected_to(fixture: &str, name: &str) -> App {
        let tree: AppMsg = serde_json::from_str(&std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(fixture)).unwrap()).unwrap();
        let AppMsg::Tree(tree) = tree else { panic!("a tree fixture") };
        let info = AppInfo { protocol: "0.1.0".into(), name: name.into(), engine: "dotnet-wpf".into(), pid: 4242, project: None, agent: None, platform: None };
        let mut app = App { connected: true, apps: vec![ConnectedApp { id: 1, info }], app: Some(1), ..Default::default() };
        app.screens = ["Recycled cells", "Images", "Shapes", "Text", "Layouts", "Common Controls", "Editor", "Transforms"]
            .iter()
            .map(|t| Screen { route: t.to_lowercase(), title: Some((*t).to_owned()) })
            .collect();
        app.set_tree(tree);
        app
    }

    fn host(state: App) -> Headless<App> {
        let font = |name: &str| assets(&format!("assets/{name}")).expect("font file");
        let ui = Ui::new(state, view::build)
            .font_bytes("Default", &font("OpenSans-Regular.ttf"))
            .font_bytes("Semibold", &font("OpenSans-Semibold.ttf"))
            .background(view::window_background());
        let mut host = Headless::new(keys(ui), 1280, 820, 1.0);
        host.ui.state.waker = Some(host.ui.waker());
        host.settle();
        host
    }

    #[test]
    fn dividers_resize_the_side_panels() {
        let mut host = host(connected());
        assert_eq!(host.ui.state.screens_width(), 160.0);
        // The grab area shows the resize cursor. Not checked over the Layers rows (x 161-164):
        // there the engine takes the cursor from the deepest control (the row's hand), while the
        // drag goes to the divider on top; reported to drawnui-rust.
        for x in [157.0, 159.0, 160.0] {
            host.hover(x, 400.0);
            host.settle();
            assert_eq!(host.ui.cursor(), Cursor::ResizeLeftRight, "resize cursor at x {x}");
        }
        // A drag from either edge of the grab area moves the line, not a row under it.
        host.pan((164.0, 400.0), (184.0, 400.0), 200.0, 10);
        host.settle();
        let w = host.ui.state.screens_width();
        assert!((w - 180.0).abs() < 2.0, "screens panel {w} after a drag from the right edge");
        host.pan((177.0, 400.0), (157.0, 400.0), 200.0, 10);
        host.settle();
        let w = host.ui.state.screens_width();
        assert!((w - 160.0).abs() < 2.0, "screens panel {w} after a drag from the left edge");
        host.pan((160.0, 400.0), (260.0, 400.0), 200.0, 10);
        host.settle();
        let w = host.ui.state.screens_width();
        assert!((w - 260.0).abs() < 2.0, "screens panel {w} after a 100 pt drag");
        let right = 1280.0 - host.ui.state.properties_width();
        host.pan((right, 400.0), (right - 80.0, 400.0), 200.0, 10);
        host.settle();
        let w = host.ui.state.properties_width();
        assert!((w - 420.0).abs() < 2.0, "properties panel {w} after an 80 pt drag");
    }

    #[test]
    fn the_fold_box_folds_without_selecting() {
        let mut host = host(connected());
        let before = host.ui.state.rows.len();
        let first = host.ui.state.tree.nodes[host.ui.state.rows[0].node].id;
        // Row 0 of Layers: title bar 40 + header 34 + padding 4, half a row down; its +/- box sits
        // on the first tree line, half a level into the panel (which starts after Screens, 160).
        host.tap(160.0 + 6.0 + 10.0, 40.0 + 34.0 + 4.0 + view::ROW / 2.0);
        host.settle();
        assert!(host.ui.state.collapsed.contains(&first), "row 0 folded");
        assert_eq!(host.ui.state.rows.len(), 1, "only the folded root stays ({before} before)");
        assert_eq!(host.ui.state.selected, None, "the box does not select");
    }

    #[test]
    fn right_click_and_a_second_tap_keep_the_selection() {
        let mut host = host(with_props());
        let row = host.ui.state.selected.unwrap();
        let other = if row == 0 { 1 } else { 0 };
        let at = |r: usize| (160.0 + 140.0, 40.0 + 34.0 + 4.0 + r as f32 * view::ROW + view::ROW / 2.0);
        // A right click on the selected row opens the menu and keeps its properties.
        let (x, y) = at(row);
        host.right_click(x, y);
        assert_eq!(host.ui.state.selected, Some(row));
        assert!(host.ui.state.popup.is_some(), "the menu opens");
        host.press_key("Escape");
        host.settle();
        // A left tap on the row already selected keeps its properties: no reload.
        let (x, y) = at(row);
        host.tap(x, y);
        assert_eq!(host.ui.state.props.len(), 5, "the shown properties stay");
        // A left tap on another row selects it.
        let (x, y) = at(other);
        host.tap(x, y);
        assert_eq!(host.ui.state.selected, Some(other), "a left tap selects");
    }

    /// The editing state a design shot and the tests use: a label selected, some properties.
    fn with_props() -> App {
        let mut app = connected();
        let row = app.rows.iter().position(|r| app.tree.nodes[r.node].type_name.ends_with("SkiaLabel")).unwrap();
        app.selected = Some(row);
        let prop = |name: &str, type_name: &str, value: &str, is_set: bool, group: &str| Prop {
            name: name.into(),
            type_name: type_name.into(),
            value: value.into(),
            is_set,
            group: Some(group.into()),
            choices: (name == "HorizontalOptions").then(|| ["Start", "Center", "End", "Fill"].map(String::from).to_vec()),
            flags: false,
            is_default: None,
        };
        app.props = vec![
            prop("FontSize", "System.Double", "28", true, "Text"),
            prop("HorizontalOptions", "DrawnUi.Views.LayoutOptions", "Fill", true, "Layout"),
            prop("Margin", "DrawnUi.Views.Thickness", "0,8,0,4", true, "Layout"),
            prop("Text", "System.String", "SkiaLayout types", true, "Text"),
            prop("TextColor", "DrawnUi.Color", "#FFFFFFFF", true, "Text"),
        ];
        app.rebuild_prop_rows();
        app
    }

    #[test]
    fn changed_lists_what_the_code_sets() {
        let (to_ui, rx) = std::sync::mpsc::channel();
        let mut state = with_props();
        state.props_order = PropsOrder::Set;
        // The library set IsEnabled to its default: not a change of anyone's.
        state.props.push(Prop { name: "IsEnabled".into(), type_name: "System.Boolean".into(), value: "true".into(), is_set: true, is_default: Some(true), ..Default::default() });
        state.rebuild_prop_rows();
        state.inbox_rx = Some(rx);
        let mut host = host(state);
        let changed = |app: &App| -> Vec<String> { (0..app.props.len()).filter(|&i| app.prop_changed(i)).map(|i| app.props[i].name.clone()).collect() };
        let mut before = changed(&host.ui.state);
        before.sort();
        assert_eq!(before, ["FontSize", "HorizontalOptions", "Margin", "Text", "TextColor"], "set and not default");
        // The hub read the code: it sets Text, FontSize and IsEnabled (at its default). What the code
        // sets is Set even at its default; a value not at its default stays Set without code too.
        let (app, node) = (host.ui.state.app.unwrap(), host.ui.state.selected_node().unwrap().id);
        to_ui.send(Incoming::Hub(HubToEditor::CodeSet { app, node, names: Some(vec!["Text".into(), "FontSize".into(), "IsEnabled".into()]) })).unwrap();
        host.ui.waker().invalidate(host.ui.state.inbox, Dirty::APPLY);
        host.settle();
        let mut after = changed(&host.ui.state);
        after.sort();
        assert_eq!(after, ["FontSize", "HorizontalOptions", "IsEnabled", "Margin", "Text", "TextColor"]);
        // No code for it: back to set and not default.
        to_ui.send(Incoming::Hub(HubToEditor::CodeSet { app, node, names: None })).unwrap();
        host.ui.waker().invalidate(host.ui.state.inbox, Dirty::APPLY);
        host.settle();
        assert_eq!(changed(&host.ui.state).len(), 5);
    }

    #[test]
    fn editing_a_property_does_not_move_the_list() {
        let mut host = host(with_props());
        let list = host.ui.state.props_list;
        let before = host.rect(list);
        // The FontSize row: first row of the Set group, under its header row (40 + 34) and the selection
        // header; tap its value cell.
        let top = before.top / 1.0 + 4.0 + view::ROW + view::ROW / 2.0;
        // A tap that does not wait for the tree to settle: a focused field blinks its caret.
        let at = (1280.0 - 340.0 + 200.0, top);
        host.pan(at, at, 16.0, 1);
        for _ in 0..10 {
            host.frame_after(16.0);
        }
        assert_eq!(host.ui.state.edit_prop.as_deref(), Some("FontSize"), "the tapped row is edited");
        assert_eq!(host.rect(list), before, "the list keeps its place and size");
        let binds = view::PROP_BINDS.with(|b| b.get());
        for _ in 0..60 {
            host.frame_after(16.0);
        }
        let more = view::PROP_BINDS.with(|b| b.get()) - binds;
        assert!(more == 0, "{more} rows bound again in 60 idle frames while editing");
    }

    #[test]
    fn a_text_edit_shows_in_layers() {
        let (to_ui, rx) = std::sync::mpsc::channel();
        let mut state = with_props();
        state.inbox_rx = Some(rx);
        let mut host = host(state);
        let row = host.ui.state.selected.unwrap();
        let node = host.ui.state.tree.nodes[host.ui.state.rows[row].node].id;
        let mut props = host.ui.state.props.clone();
        props.iter_mut().find(|p| p.name == "Text").unwrap().value = "Renamed".into();
        let app = host.ui.state.app.unwrap();
        to_ui.send(Incoming::Hub(HubToEditor::FromApp { app, msg: AppMsg::Props { node, props } })).unwrap();
        host.ui.waker().invalidate(host.ui.state.inbox, Dirty::APPLY);
        host.settle();
        let n = host.ui.state.rows[row].node;
        assert_eq!(host.ui.state.tree.nodes[n].text.as_deref(), Some("Renamed"));
    }

    /// The panels' scroll bars: shown while the pointer is over a panel and while it scrolls, in the
    /// theme's colors. Looked at by eye: target/designer-shots/25-scrollbar*.png.
    #[test]
    #[ignore]
    fn scrollbar_shot() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/designer-shots");
        std::fs::create_dir_all(&dir).expect("shots folder");
        for theme in ["dark", "light"] {
            let mut state = connected();
            state.settings.theme = view::Theme::all().find(|t| t.id() == theme).unwrap();
            let mut host = Headless::new(keys(Ui::new(state, view::build).font_bytes("Default", &assets("assets/OpenSans-Regular.ttf").unwrap()).font_bytes("Semibold", &assets("assets/OpenSans-Semibold.ttf").unwrap()).background(view::window_background())), 1280, 500, 1.0);
            host.settle();
            let layers = host.rect(host.ui.state.layers_scroll);
            let (x, y) = (layers.center_x(), layers.center_y());
            host.hover(x, y);
            host.pan((x, y), (x, y - 120.0), 16.0, 8);
            for _ in 0..4 {
                host.frame_after(16.0);
            }
            host.save_png(dir.join(format!("25-scrollbar-{theme}.png")).to_str().unwrap());
        }
        // A narrow window: Layers scrolls sideways too, its bar at the bottom.
        let state = connected();
        let mut host = Headless::new(keys(Ui::new(state, view::build).font_bytes("Default", &assets("assets/OpenSans-Regular.ttf").unwrap()).font_bytes("Semibold", &assets("assets/OpenSans-Semibold.ttf").unwrap()).background(view::window_background())), 720, 500, 1.0);
        host.settle();
        let layers = host.rect(host.ui.state.layers_scroll);
        let (x, y) = (layers.center_x(), layers.center_y());
        host.hover(x, y);
        host.pan((x, y), (x - 100.0, y), 16.0, 8);
        for _ in 0..4 {
            host.frame_after(16.0);
        }
        eprintln!("layers_width {} viewport {}", host.ui.state.layers_width, layers.width());
        host.save_png(dir.join("25-scrollbar-narrow.png").to_str().unwrap());
    }

    #[test]
    fn the_apps_menu_groups_by_platform() {
        let (to_ui, rx) = std::sync::mpsc::channel();
        let mut state = connected();
        state.inbox_rx = Some(rx);
        let mut host = host(state);
        let app = |id: u32, name: &str, platform: &str| ConnectedApp { id, info: AppInfo { protocol: "0.1.0".into(), name: name.into(), engine: "dotnet-maui".into(), pid: id, project: None, agent: None, platform: Some(platform.into()) } };
        to_ui.send(Incoming::Hub(HubToEditor::Apps { apps: vec![app(2, "Tutorials", "windows"), app(3, "Tutorials", "android"), app(4, "Shop", "android")], recent: vec![] })).unwrap();
        host.ui.waker().invalidate(host.ui.state.inbox, Dirty::APPLY);
        host.settle();
        assert_eq!(host.ui.state.platforms(), ["windows", "android"]);
        assert_eq!(view::app_menu_groups(&host.ui.state), [("windows".to_owned(), vec![0]), ("android".to_owned(), vec![1, 2])]);
        assert_eq!(host.ui.state.app, Some(4), "the newest app is shown");
        // The status line says where apps can come from.
        to_ui.send(Incoming::Hub(HubToEditor::Cables { android: 1, ios: 0 })).unwrap();
        host.ui.waker().invalidate(host.ui.state.inbox, Dirty::APPLY);
        host.settle();
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        host.settle();
        let labels: Vec<String> = host.ui.accessibility_nodes().iter().map(|n| n.label.clone()).collect();
        assert!(labels.iter().any(|l| l.contains("3 apps") && l.contains("Wi-Fi off") && l.contains("USB: 1 Android")), "{labels:?}");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/designer-shots");
        std::fs::create_dir_all(&dir).expect("shots folder");
        host.save_png(dir.join("24-platform-chip.png").to_str().unwrap());
    }

    #[test]
    fn the_title_bar_shows_only_the_last_app() {
        let (to_ui, rx) = std::sync::mpsc::channel();
        let mut state = connected();
        state.inbox_rx = Some(rx);
        let mut host = host(state);
        let info = |name: &str| AppInfo { protocol: "0.1.0".into(), name: name.into(), engine: "dotnet-maui".into(), pid: 7, project: None, agent: None, platform: Some("android".into()) };
        let recent = |name: &str| designer_protocol::RecentApp { name: name.into(), engine: "dotnet-wpf".into(), project: format!("{name}.csproj"), agent: None, last_seen: 0, platform: None };
        let send = |host: &mut Headless<App>, apps: Vec<ConnectedApp>, recent: Vec<designer_protocol::RecentApp>| {
            to_ui.send(Incoming::Hub(HubToEditor::Apps { apps, recent })).unwrap();
            host.ui.waker().invalidate(host.ui.state.inbox, Dirty::APPLY);
            host.settle();
        };
        let wpf = host.ui.state.apps[0].clone();
        // A second app connects: it is shown, one pill.
        send(&mut host, vec![wpf.clone(), ConnectedApp { id: 2, info: info("Tutorials") }], vec![recent("A"), recent("B")]);
        assert_eq!((host.ui.state.app, host.ui.state.pill_count(), host.ui.state.pill_slot()), (Some(2), 1, 1));
        // The pill says where the app runs.
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        host.settle();
        assert!(host.ui.accessibility_nodes().iter().any(|n| n.label.contains("Tutorials") && n.label.contains("Android")), "{:?}", host.ui.accessibility_nodes().iter().map(|n| n.label.clone()).collect::<Vec<_>>());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/designer-shots");
        std::fs::create_dir_all(&dir).expect("shots folder");
        host.save_png(dir.join("23-platform-chip.png").to_str().unwrap());
        // It goes away: the other one is shown again.
        send(&mut host, vec![wpf], vec![recent("Tutorials"), recent("A")]);
        assert_eq!((host.ui.state.app, host.ui.state.pill_count()), (Some(1), 1));
        // Nothing connected: the pill is the most recent app.
        send(&mut host, vec![], vec![recent("HelloWpf"), recent("Tutorials")]);
        assert_eq!((host.ui.state.app, host.ui.state.pill_count(), host.ui.state.pill_slot()), (None, 1, 0));
    }

    #[test]
    fn undo_takes_back_a_live_edit_then_a_write() {
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::<EditorMsg>::new()));
        let (to_ui, rx) = std::sync::mpsc::channel();
        let mut state = with_props();
        state.inbox_rx = Some(rx);
        let log = sent.clone();
        state.to_hub = Some(Box::new(move |m| log.lock().unwrap().push(m)));
        let mut host = host(state);
        let list = host.rect(host.ui.state.props_list);
        let top = list.top + 4.0 + view::ROW + view::ROW / 2.0;
        let undo_at = (list.right - view::BAR_LANE - 13.0 - 28.0, top);
        // FontSize 28, +1 live, then the edit ends (Enter): 29 in the app, not in code.
        let at = (1280.0 - 340.0 + 200.0, top);
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        let field = host.rect(host.ui.state.edit_overlay);
        let plus = (field.right - 6.0, field.center_y());
        host.pan(plus, plus, 16.0, 1);
        host.frame_after(16.0);
        host.press_key("Enter");
        host.frame_after(16.0);
        let font_size = host.ui.state.props.iter().position(|p| p.name == "FontSize").unwrap();
        assert!(host.ui.state.can_undo(font_size), "a live edit can be undone");
        sent.lock().unwrap().clear();
        host.pan(undo_at, undo_at, 16.0, 1);
        host.frame_after(16.0);
        let set_back = sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::ToApp { msg: HubToApp::SetProperty { name, value, .. }, .. } if name == "FontSize" && value == "28"));
        assert!(set_back, "Undo sets 28 back in the app: {:?}", sent.lock().unwrap());
        assert_eq!(host.ui.state.props[font_size].value, "28", "the row shows the value it went back to");
        assert!(host.ui.state.pending.is_empty() && !host.ui.state.can_undo(font_size));
        // A write to code (answered by the hub), then Undo asks the hub to take it back.
        // Write shows on the hovered row.
        let write_at = (list.right - view::BAR_LANE - 13.0, top);
        host.hover(write_at.0, write_at.1);
        host.frame_after(16.0);
        host.pan(write_at, write_at, 16.0, 1);
        host.frame_after(16.0);
        let request = host.ui.state.code_requests.keys().copied().next().unwrap();
        let range = designer_protocol::source::EditorRange::default();
        to_ui.send(Incoming::Hub(HubToEditor::CodeEdited { request, node: 0, uri: "Page.cs".into(), range })).unwrap();
        host.ui.waker().invalidate(host.ui.state.inbox, Dirty::APPLY);
        host.settle();
        assert!(host.ui.state.can_undo(font_size), "a write can be undone");
        sent.lock().unwrap().clear();
        host.pan(undo_at, undo_at, 16.0, 1);
        host.frame_after(16.0);
        assert!(sent.lock().unwrap().contains(&EditorMsg::UndoCode { request }), "{:?}", sent.lock().unwrap());
        to_ui.send(Incoming::Hub(HubToEditor::CodeUndone { request, uri: "Page.cs".into() })).unwrap();
        host.ui.waker().invalidate(host.ui.state.inbox, Dirty::APPLY);
        host.settle();
        assert!(!host.ui.state.can_undo(font_size), "nothing left to undo");
    }

    /// An app with no controls of its own (a page's own wrapper and nothing in it) shows an empty
    /// state in Layers. A snippet's lone root (a new React or Rust snippet: one empty layer) is a row,
    /// ready for drops.
    #[test]
    fn an_empty_app_says_so_in_layers() {
        let lone = |page_root: bool| {
            let mut state = connected();
            let mut root = state.tree.nodes[0].clone();
            root.page_root = page_root;
            let mut tree = state.tree.clone();
            tree.nodes = vec![root];
            tree.revision = 2;
            state.set_tree(tree);
            let mut host = host(state);
            host.settle();
            host
        };
        let shown = |h: &Headless<App>| h.ui.tree.base(h.ui.state.empty_tree).is_some_and(|b| b.p.is_visible);
        let host = lone(true);
        assert!(shown(&host) && host.ui.state.rows.is_empty(), "only the page's wrapper: the empty state, no row");
        let host = lone(false);
        assert!(!shown(&host) && host.ui.state.rows.len() == 1, "a lone root of the app's own: its row");
        let mut host = self::host(connected());
        host.settle();
        assert!(!shown(&host), "not with controls");
    }

    /// A recycled cell's row says which item it showed, and a live edit on it says it is this cell only.
    #[test]
    fn a_recycled_cell_shows_its_item() {
        let mut state = with_props();
        let node = state.selected_node().unwrap().id;
        let at = state.tree.nodes.iter().position(|n| n.id == node).unwrap();
        state.tree.nodes[at].template_index = Some(12);
        state.edit_prop = Some("FontSize".into());
        state.edit_value = "30".into();
        state.apply_edit_live(false);
        assert!(state.status.contains("item 12") && state.status.contains("every item"), "{}", state.status);
        let mut host = host(state);
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        host.settle();
        assert!(host.ui.accessibility_nodes().iter().any(|n| n.label.contains("item 12")), "the row reads its item");
    }

    /// Inside a page (DrawFiddle's C# snippet) the page's own wrapper has no row: the snippet's root is
    /// the top row, as for a React or Rust snippet. Drops and pastes count in the controls under it.
    #[test]
    fn a_page_wrapper_has_no_row() {
        let mut state = with_controls(connected());
        let n = |id: u64, parent: Option<u64>, t: &str| Node { id, parent, type_name: format!("DrawnUi.Draw.{t}"), visible: true, rect: [0.0, 0.0, 100.0, 20.0], ..Default::default() };
        // The wrapper 1, the snippet's layer 2 with a label 3 and a stack 4 holding a label 5.
        let mut nodes = vec![n(1, None, "SkiaLayout"), n(2, Some(1), "SkiaLayer"), n(3, Some(2), "SkiaLabel"), n(4, Some(2), "SkiaStack"), n(5, Some(4), "SkiaLabel")];
        nodes[0].page_root = true;
        state.set_tree(Tree { revision: 7, width: 400.0, height: 300.0, nodes });
        state.collapsed.clear();
        state.rebuild_rows();
        let rows: Vec<(u64, usize)> = state.rows.iter().map(|r| (state.tree.nodes[r.node].id, r.depth)).collect();
        assert_eq!(rows, [(2, 0), (3, 1), (4, 1), (5, 2)], "the snippet's root first, no row for the wrapper");
        // Into the layer (middle of its row): last, after its two controls; into the stack: after its label.
        assert_eq!(state.drop_at(0.5 * view::ROW).map(|d| (d.parent, d.index, d.into)), Some((2, 2, true)));
        assert_eq!(state.drop_at(2.5 * view::ROW).map(|d| (d.parent, d.index, d.into)), Some((4, 1, true)));
        // Between the label and the stack: in the layer at 1. Above the layer: nothing (that is the wrapper).
        assert_eq!(state.drop_at(1.9 * view::ROW).map(|d| (d.parent, d.index, d.into)), Some((2, 1, false)));
        assert_eq!(state.drop_at(0.1 * view::ROW), None);
        // The layer selected: a paste goes into it, last.
        state.clip = Some(Copied { type_name: "DrawnUi.Draw.SkiaLabel".into(), props: vec![], children: vec![] });
        state.selected = Some(0);
        assert_eq!(state.selected_node().map(|n| n.id), Some(2));
        assert_eq!(state.paste_target(false), Some((2, 2)));
    }

    /// Inside a page the root is the page's wrapper: no drop or paste goes into it.
    #[test]
    fn a_page_root_takes_no_controls() {
        let mut state = connected();
        let root = state.tree.nodes[0].id;
        let root_type = state.tree.nodes[0].type_name.clone();
        state.controls.insert(state.app.unwrap(), vec![ControlKind { type_name: root_type, group: "Layout".into(), container: true, ..Default::default() }]);
        state.embedded = true;
        assert!(state.accepts_children(root), "an app's own root (a React snippet's) takes controls, also in a page");
        state.tree.nodes[0].page_root = true;
        assert!(!state.accepts_children(root), "the page's wrapper does not");
    }

    /// A layout that makes its children from items takes no control, also with no items yet (no
    /// cell to tell it by): no drop into it, no paste.
    #[test]
    fn an_empty_items_layout_takes_no_controls() {
        let mut state = with_controls(connected());
        let wrap = state.tree.nodes.iter().find(|n| n.type_name.ends_with("SkiaWrap")).map(|n| n.id).unwrap();
        let mut tree = state.tree.clone();
        let kids: Vec<u64> = tree.nodes.iter().filter(|n| n.parent == Some(wrap)).map(|n| n.id).collect();
        tree.nodes.retain(|n| !kids.contains(&n.id) && !n.parent.is_some_and(|p| kids.contains(&p)));
        state.set_tree(tree.clone());
        state.collapsed.clear();
        state.rebuild_rows();
        assert!(state.accepts_children(wrap), "an empty plain layout takes controls");
        tree.nodes.iter_mut().filter(|n| n.id == wrap).for_each(|n| n.templated = true);
        state.set_tree(tree);
        state.collapsed.clear();
        state.rebuild_rows();
        assert!(!state.accepts_children(wrap));
        let row = state.rows.iter().position(|r| state.tree.nodes[r.node].id == wrap).unwrap();
        for frac in [0.3, 0.5, 0.7] {
            let drop = state.drop_at((row as f32 + frac) * view::ROW);
            assert_eq!(drop, None, "at {frac} of its row: no drop into it, nor into its parent instead");
        }
        state.selected = Some(row);
        assert!(state.paste_target(false).is_none_or(|(p, _)| p != wrap), "no paste into it");
    }

    /// Every cell of a templated layout is the same code: nothing goes into, beside or out of any
    /// control inside a cell, at any depth. Its properties stay editable.
    #[test]
    fn a_cell_of_a_template_keeps_its_structure() {
        let mut state = with_controls(connected());
        // The wrap 10 makes its cells 11, 16, 21 from items; 12 is a stack inside cell 11, with labels 13, 14.
        let mut tree = state.tree.clone();
        for n in tree.nodes.iter_mut() {
            if n.id == 10 {
                n.templated = true;
            }
            if n.parent == Some(10) {
                n.template_index = Some(n.id);
            }
        }
        state.set_tree(tree);
        state.collapsed.clear();
        state.rebuild_rows();
        state.clip = Some(Copied { type_name: "DrawnUi.Draw.SkiaLabel".into(), props: vec![], children: vec![] });
        let row = |state: &App, id: u64| state.rows.iter().position(|r| state.tree.nodes[r.node].id == id).unwrap();
        assert!(!state.accepts_children(12), "a stack inside a cell takes nothing");
        assert_eq!(state.drop_at((row(&state, 12) as f32 + 0.5) * view::ROW), None, "no drop over it");
        assert_eq!(state.drop_at((row(&state, 13) as f32 + 0.1) * view::ROW), None, "nor beside a control inside the cell");
        state.selected = Some(row(&state, 13));
        for a in [MenuAction::Delete, MenuAction::Paste, MenuAction::Duplicate] {
            assert!(!state.action_enabled(a), "{a:?}");
        }
        assert!(state.action_enabled(MenuAction::Copy) && state.action_enabled(MenuAction::CopyStyle));
        assert!(!state.move_selected_by(1), "Alt+Down does nothing");
        // Its row does not lift.
        let (mut host, sent, _to_ui) = wired(state);
        host.ui.state.selected = None;
        let from = row_at(row_of(&host, 13));
        drag_steps(&mut host, from, (from.0, from.1 + view::ROW * 3.4), 6, false);
        assert_eq!(host.ui.state.dragged_node(), None);
        assert!(!sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::ToApp { msg: HubToApp::Move { .. }, .. })), "{:?}", sent.lock().unwrap());
    }

    /// Page mode: a control dragged from Controls out of the editor's frame goes on in the page, told
    /// in the page's pixels; let go there, the page places it. Back in, it is a drag over Layers again.
    /// The desktop window never tells anyone.
    #[test]
    fn a_controls_drag_out_of_the_frame_goes_on_in_the_page() {
        let run = |embedded: bool| {
            let posted = std::sync::Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
            let mut state = with_controls(connected());
            state.embedded = embedded;
            state.frame_offset = (100.0, 50.0);
            let log = posted.clone();
            state.to_page = Some(Box::new(move |m| log.lock().unwrap().push(m)));
            let (mut host, sent, _to_ui) = wired(state);
            let from = at_label(&mut host, "SkiaLabel, Text");
            (host, sent, posted, from)
        };
        let phases = |posted: &std::sync::Mutex<Vec<serde_json::Value>>| posted.lock().unwrap().iter().map(|m| m["phase"].as_str().unwrap_or("").to_owned()).collect::<Vec<_>>();
        let inserts = |sent: &std::sync::Mutex<Vec<EditorMsg>>| sent.lock().unwrap().iter().filter(|m| matches!(m, EditorMsg::ToApp { msg: HubToApp::Insert { .. }, .. })).count();

        // Out to the left of the frame, let go there: moves, then the drop, at the page's point.
        let (mut host, sent, posted, from) = run(true);
        drag_steps(&mut host, from, (-40.0, from.1), 6, false);
        let got = phases(&posted);
        assert!(got.len() >= 2 && got[..got.len() - 1].iter().all(|p| p == "move") && got.last().map(String::as_str) == Some("drop"), "{got:?}");
        let last = posted.lock().unwrap().last().cloned().unwrap();
        assert_eq!((last["type"].as_str(), last["typeName"].as_str()), (Some("drag.out"), Some("DrawnUi.Draw.SkiaLabel")));
        assert!((last["x"].as_f64().unwrap() - 60.0).abs() < 0.5 && (last["y"].as_f64().unwrap() - (50.0 + from.1 as f64)).abs() < 0.5, "{last}");
        assert!(host.ui.state.drag.is_none() && inserts(&sent) == 0, "the editor inserts nothing itself");

        // Out, then back in over Layers: the page hears "cancel", the drop goes into Layers as before.
        posted.lock().unwrap().clear();
        let list = host.rect(host.ui.state.layers_scroll);
        let onto = (list.left + 120.0, list.top + view::PROPS_PAD + view::ROW * 0.5);
        drag_steps(&mut host, from, (-40.0, from.1), 6, true);
        for k in 1..=6 {
            let f = k as f32 / 6.0;
            let t = host.time_ms();
            host.ui.pointer(drawnui::PointerKind::Move, -40.0 + (onto.0 + 40.0) * f, from.1 + (onto.1 - from.1) * f, t);
            host.frame_after(16.0);
        }
        let t = host.time_ms();
        host.ui.pointer(drawnui::PointerKind::Up, onto.0, onto.1, t);
        host.frame_after(16.0);
        let got = phases(&posted);
        assert_eq!(got.last().map(String::as_str), Some("cancel"), "{got:?}");
        assert!(!got.iter().any(|p| p == "drop"), "{got:?}");
        assert_eq!(inserts(&sent), 1, "dropped on Layers: inserted there");

        // The desktop window: the same drag tells no page.
        let (mut host, sent, posted, from) = run(false);
        drag_steps(&mut host, from, (-40.0, from.1), 6, false);
        assert!(posted.lock().unwrap().is_empty() && inserts(&sent) == 0);
    }

    /// Page mode: the page's drop on the app's canvas goes to the app as a place there, and its answer
    /// is recorded as a placed control's.
    #[test]
    fn the_pages_drop_on_the_canvas_places_the_control() {
        let (mut host, sent, to_ui) = wired(with_controls(connected()));
        to_ui.send(Incoming::Place { type_name: "DrawnUi.Draw.SkiaLabel".into(), x: 40.0, y: 30.0 }).unwrap();
        host.ui.waker().invalidate(host.ui.state.inbox, Dirty::APPLY);
        host.settle();
        assert!(sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::ToApp { msg: HubToApp::PlaceAt { type_name, x, y }, .. } if type_name == "DrawnUi.Draw.SkiaLabel" && *x == 40.0 && *y == 30.0)), "{:?}", sent.lock().unwrap());
        assert_eq!(host.ui.state.placing.as_deref(), Some("DrawnUi.Draw.SkiaLabel"), "its inserted answer names the type");
    }

    // ---- Writing changes to code (Settings, Code: Manual, Auto, Always) ----

    /// Two values changed live and not written: FontSize on the label 117, Text on the label 118.
    fn with_changes(mode: WriteMode) -> App {
        let mut state = with_props();
        state.settings.write_mode = mode;
        state.apps[0].info.project = Some("C:/apps/HelloWpf/HelloWpf.csproj".into());
        for (node, prop, value) in [(117u64, "FontSize", "30"), (118, "Text", "Changed")] {
            state.live_original.insert((node, prop.into()), "x".into());
            state.pending.insert((node, prop.into()), value.into());
        }
        state
    }

    fn set_codes(sent: &std::sync::Mutex<Vec<EditorMsg>>) -> Vec<(u32, u64, String)> {
        sent.lock().unwrap().iter().filter_map(|m| if let EditorMsg::SetCode { request, node, prop, .. } = m { Some((*request, *node, prop.clone())) } else { None }).collect()
    }

    fn edited(host: &mut Headless<App>, to_ui: &std::sync::mpsc::Sender<Incoming>, request: u32, node: u64) {
        let range = designer_protocol::source::EditorRange::default();
        feed(host, to_ui, HubToEditor::CodeEdited { request, node, uri: "Page.cs".into(), range });
    }

    fn frames(host: &mut Headless<App>, n: usize) {
        for _ in 0..n {
            host.frame_after(16.0);
        }
    }

    fn restarts(sent: &std::sync::Mutex<Vec<EditorMsg>>) -> usize {
        sent.lock().unwrap().iter().filter(|m| matches!(m, EditorMsg::RestartApp { .. })).count()
    }

    /// The desktop window has Pick in its title bar, named with its key; Layers has it only in a
    /// page, which has no title bar.
    #[test]
    fn pick_is_in_the_title_bar_on_the_desktop_and_in_layers_in_a_page() {
        let names = |embedded: bool| {
            let mut state = connected();
            state.embedded = embedded;
            let mut host = host(state);
            host.ui.set_accessibility_enabled(true);
            host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
            let labels: Vec<String> = host.ui.accessibility_nodes().iter().map(|n| n.label.clone()).collect();
            (labels.iter().any(|l| l == "Pick a control in the app (Ctrl+P)"), labels.iter().any(|l| l == "Pick a control in the app"))
        };
        assert_eq!(names(false), (true, false), "desktop: the title bar's, not Layers'");
        assert_eq!(names(true), (false, true), "in a page: Layers', no title bar");
        // The title bar's button picks, and reads pressed while it does.
        let mut host = host(connected());
        let at = at_label(&mut host, "Pick a control in the app (Ctrl+P)");
        host.tap(at.0, at.1);
        host.settle();
        assert!(host.ui.state.pick_mode);
    }

    /// The changes not in code are counted where they can be written at once: the title bar on the
    /// desktop, the Layers header in a page.
    #[test]
    fn the_changes_not_in_code_are_counted_with_write_all() {
        let shown = |embedded: bool, mode: WriteMode| {
            let mut state = with_changes(mode);
            state.embedded = embedded;
            let mut host = host(state);
            host.ui.set_accessibility_enabled(true);
            host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
            host.ui.accessibility_nodes().iter().any(|n| n.label == "Write all 2 changes to code")
        };
        assert!(shown(false, WriteMode::Manual) && shown(true, WriteMode::Manual), "both windows, every mode");
        let mut state = with_changes(WriteMode::Manual);
        state.pending.clear();
        let mut host = host(state);
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        assert!(!host.ui.accessibility_nodes().iter().any(|n| n.label.starts_with("Write all") && n.label.ends_with("changes to code") && n.label.contains("Write all 0")), "none: not shown");
    }

    /// Write all changes: every change in one go, one step of the history; Ctrl+Z takes all of
    /// them back together.
    #[test]
    fn a_flush_is_one_step_of_the_history() {
        let (mut host, sent, to_ui) = wired(with_changes(WriteMode::Manual));
        let at = at_label(&mut host, "Write all 2 changes to code");
        host.tap(at.0, at.1);
        host.settle();
        let writes = set_codes(&sent);
        assert_eq!(writes.iter().map(|w| (w.1, w.2.as_str())).collect::<Vec<_>>(), [(117, "FontSize"), (118, "Text")]);
        for (request, node, _) in &writes {
            edited(&mut host, &to_ui, *request, *node);
        }
        let undo = host.ui.state.histories.get(&1).map(|h| h.undo.iter().map(|s| s.label()).collect::<Vec<_>>()).unwrap_or_default();
        assert_eq!(undo, ["Write all changes"], "one step");
        sent.lock().unwrap().clear();
        host.key_down("KeyZ", ctrl(false));
        host.settle();
        let undone: Vec<u32> = sent.lock().unwrap().iter().filter_map(|m| if let EditorMsg::UndoCode { request } = m { Some(*request) } else { None }).collect();
        assert_eq!(undone.len(), 2, "both writes taken back: {undone:?}");
    }

    /// Restart: Manual asks first; Auto writes the changes, then restarts once they are written.
    #[test]
    fn restart_writes_the_changes_first() {
        // Auto: written at once, the restart once both writes are answered.
        let (mut host, sent, to_ui) = wired(with_changes(WriteMode::Auto));
        host.key_down("KeyR", ctrl(false));
        host.settle();
        let writes = set_codes(&sent);
        assert_eq!((writes.len(), restarts(&sent)), (2, 0), "written first, no restart yet");
        edited(&mut host, &to_ui, writes[0].0, writes[0].1);
        assert_eq!(restarts(&sent), 0, "one write still on its way");
        edited(&mut host, &to_ui, writes[1].0, writes[1].1);
        assert_eq!(restarts(&sent), 1, "then the restart");
        // Manual: the question, nothing written or restarted until it is answered.
        let (mut host, sent, to_ui) = wired(with_changes(WriteMode::Manual));
        host.key_down("KeyR", ctrl(false));
        host.settle();
        assert!(host.ui.state.confirm.is_some() && set_codes(&sent).is_empty() && restarts(&sent) == 0);
        let write = at_label(&mut host, "Write them, then restart");
        host.tap(write.0, write.1);
        host.settle();
        let writes = set_codes(&sent);
        assert_eq!(writes.len(), 2);
        for (request, node, _) in &writes {
            edited(&mut host, &to_ui, *request, *node);
        }
        assert_eq!(restarts(&sent), 1);
        // Manual, "Restart without writing": the changes stay live.
        let (mut host, sent, _to_ui) = wired(with_changes(WriteMode::Manual));
        host.key_down("KeyR", ctrl(false));
        host.settle();
        let skip = at_label(&mut host, "Restart without writing");
        host.tap(skip.0, skip.1);
        host.settle();
        assert_eq!((set_codes(&sent).len(), restarts(&sent), host.ui.state.pending.len()), (0, 1, 2));
    }

    /// A page's code.flush: Auto writes and answers once the writes are done; Manual answers at
    /// once, with what is left.
    #[test]
    fn a_page_flush_writes_and_answers() {
        let run = |mode: WriteMode| {
            let posted = std::sync::Arc::new(std::sync::Mutex::new(Vec::<serde_json::Value>::new()));
            let mut state = with_changes(mode);
            state.embedded = true;
            let log = posted.clone();
            state.to_page = Some(Box::new(move |m| log.lock().unwrap().push(m)));
            let (mut host, sent, to_ui) = wired(state);
            to_ui.send(Incoming::Flush(serde_json::json!(7))).unwrap();
            host.ui.waker().invalidate(host.ui.state.inbox, Dirty::APPLY);
            host.settle();
            (host, sent, to_ui, posted)
        };
        let (mut host, sent, to_ui, posted) = run(WriteMode::Auto);
        let writes = set_codes(&sent);
        assert!(writes.len() == 2 && posted.lock().unwrap().is_empty(), "not answered before the writes are");
        for (request, node, _) in &writes {
            edited(&mut host, &to_ui, *request, *node);
        }
        assert_eq!(posted.lock().unwrap().as_slice(), [serde_json::json!({ "type": "code.flushed", "request": 7, "written": 2, "left": 0 })]);
        let (_host, sent, _to_ui, posted) = run(WriteMode::Manual);
        assert!(set_codes(&sent).is_empty());
        assert_eq!(posted.lock().unwrap().as_slice(), [serde_json::json!({ "type": "code.flushed", "request": 7, "written": 0, "left": 2 })]);
    }

    /// Always: a finished edit is written at once (Enter), never while it is typed.
    #[test]
    fn always_writes_a_finished_edit_not_each_keystroke() {
        let mut state = with_props();
        state.settings.write_mode = WriteMode::Always;
        let (mut host, sent, _to_ui) = wired(state);
        let top = host.rect(host.ui.state.props_list).top + 4.0 + view::ROW + view::ROW / 2.0;
        let at = (1280.0 - 340.0 + 300.0, top);
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        host.frame_after(16.0);
        host.type_text("5");
        frames(&mut host, 10);
        assert!(set_codes(&sent).is_empty(), "typing writes nothing");
        host.press_key("Enter");
        frames(&mut host, 10);
        assert!(sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::SetCode { prop, value, .. } if prop == "FontSize" && value == "285")), "{:?}", sent.lock().unwrap());
    }

    /// Always: a row dragged over Layers is written once it is dropped and moved, never per frame
    /// of the drag.
    #[test]
    fn always_writes_a_move_once_it_is_dropped() {
        let mut state = label_selected_state();
        state.settings.write_mode = WriteMode::Always;
        let (mut host, sent, to_ui) = wired(state);
        let moves = |sent: &std::sync::Mutex<Vec<EditorMsg>>| sent.lock().unwrap().iter().filter(|m| matches!(m, EditorMsg::MoveCode { .. })).count();
        let from = row_at(row_of(&host, 117));
        let below = row_at(row_of(&host, 119));
        let to = (from.0, below.1 + view::ROW * 1.4);
        drag_steps(&mut host, from, to, 6, true);
        frames(&mut host, 10);
        assert_eq!(moves(&sent), 0, "nothing written while it is carried");
        let t = host.time_ms();
        host.ui.pointer(drawnui::PointerKind::Up, to.0, to.1, t);
        host.frame_after(16.0);
        let (request, node, parent, index) = last_move(&sent).expect("a Move");
        app_moves(&mut host, &to_ui, request, node, parent, index);
        frames(&mut host, 10);
        assert_eq!(moves(&sent), 1, "written once it moved");
    }

    /// Auto and Manual never write by themselves.
    #[test]
    fn auto_and_manual_leave_a_finished_edit_live() {
        for mode in [WriteMode::Auto, WriteMode::Manual] {
            let mut state = with_props();
            state.settings.write_mode = mode;
            let (mut host, sent, _to_ui) = wired(state);
            let top = host.rect(host.ui.state.props_list).top + 4.0 + view::ROW + view::ROW / 2.0;
            let at = (1280.0 - 340.0 + 300.0, top);
            host.pan(at, at, 16.0, 1);
            host.frame_after(16.0);
            host.frame_after(16.0);
            host.type_text("5");
            host.press_key("Enter");
            frames(&mut host, 10);
            assert!(set_codes(&sent).is_empty() && !host.ui.state.pending.is_empty(), "{mode:?}");
        }
    }

    /// The hosting page (DrawFiddle) sets the look: the designer takes that theme at once, with no
    /// opening circle (that is only for a theme picked in Settings).
    #[test]
    fn the_page_sets_the_theme() {
        let (to_ui, rx) = std::sync::mpsc::channel();
        let mut state = connected();
        state.inbox_rx = Some(rx);
        let mut host = host(state);
        to_ui.send(Incoming::Theme("light".into())).unwrap();
        host.ui.waker().invalidate(host.ui.state.inbox, Dirty::APPLY);
        for _ in 0..4 {
            host.frame_after(16.0);
            assert!(host.ui.state.wipe.is_none(), "no circle");
        }
        assert_eq!(host.ui.state.settings.theme.id(), "light", "applied within a few frames");
        view::set_theme(view::Theme::by_id("dark"));
    }

    /// Settings, "Animate theme and language changes" off: a theme picked applies at once.
    #[test]
    fn a_theme_change_can_skip_the_circle() {
        let mut state = connected();
        state.settings.instant_changes = true;
        let mut host = host(state);
        host.key_down("Comma", ctrl(false));
        host.settle();
        let light = at_label(&mut host, "Light");
        host.pan(light, light, 16.0, 1);
        host.frame_after(16.0);
        host.frame_after(16.0);
        assert!(host.ui.state.wipe.is_none(), "no circle");
        assert_eq!(host.ui.state.settings.theme.id(), "light");
        view::set_theme(view::Theme::by_id("dark"));
    }

    /// The selection box in the app takes the Selection color from Settings.
    #[test]
    fn the_selection_box_takes_the_settings_color() {
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::<EditorMsg>::new()));
        let mut state = connected();
        state.settings.selection_color = Some("#FFA3E635".into());
        let log = sent.clone();
        state.to_hub = Some(Box::new(move |m| log.lock().unwrap().push(m)));
        let mut host = host(state);
        let at = (160.0 + 120.0, 40.0 + 34.0 + 4.0 + view::ROW * 1.5);
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        let colored = sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::ToApp { msg: HubToApp::Highlight { node: Some(_), color: Some(c) }, .. } if c == "#FFA3E635"));
        assert!(colored, "{:?}", sent.lock().unwrap());
    }

    /// The app builds its controls again (new ids): the selection follows the control at the same
    /// place and asks for its properties.
    #[test]
    fn the_selection_follows_a_rebuild() {
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::<EditorMsg>::new()));
        let (to_ui, rx) = std::sync::mpsc::channel();
        let mut state = connected();
        state.inbox_rx = Some(rx);
        let log = sent.clone();
        state.to_hub = Some(Box::new(move |m| log.lock().unwrap().push(m)));
        let mut host = host(state);
        let at = (160.0 + 120.0, 40.0 + 34.0 + 4.0 + view::ROW * 1.5);
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        let picked = host.ui.state.selected_node().map(|n| (n.id, n.type_name.clone())).expect("a row selected");
        let mut tree = host.ui.state.tree.clone();
        for n in &mut tree.nodes {
            n.id += 10_000;
            n.parent = n.parent.map(|p| p + 10_000);
        }
        sent.lock().unwrap().clear();
        to_ui.send(Incoming::Hub(HubToEditor::FromApp { app: 1, msg: AppMsg::Tree(tree) })).unwrap();
        host.ui.waker().invalidate(host.ui.state.inbox, Dirty::APPLY);
        host.settle();
        let now = host.ui.state.selected_node().map(|n| (n.id, n.type_name.clone()));
        assert_eq!(now, Some((picked.0 + 10_000, picked.1)), "the same control, built again");
        let asked = sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::ToApp { msg: HubToApp::GetProps { node }, .. } if *node == picked.0 + 10_000));
        assert!(asked, "its properties asked for: {:?}", sent.lock().unwrap());
    }

    #[test]
    fn undo_during_an_edit_ends_it_and_shows_the_value_taken_back() {
        let mut host = host(with_props());
        let list = host.rect(host.ui.state.props_list);
        let top = list.top + 4.0 + view::ROW + view::ROW / 2.0;
        let undo_at = (list.right - view::BAR_LANE - 13.0 - 28.0, top);
        // FontSize 28, +1 live while the field stays open, then Undo.
        let at = (1280.0 - 340.0 + 200.0, top);
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        let field = host.rect(host.ui.state.edit_overlay);
        let plus = (field.right - 6.0, field.center_y());
        host.pan(plus, plus, 16.0, 1);
        host.frame_after(16.0);
        assert_eq!(host.ui.state.edit_prop.as_deref(), Some("FontSize"));
        host.pan(undo_at, undo_at, 16.0, 1);
        host.frame_after(16.0);
        let font_size = host.ui.state.props.iter().position(|p| p.name == "FontSize").unwrap();
        assert_eq!(host.ui.state.edit_prop, None, "Undo ends the edit");
        assert_eq!(host.ui.state.props[font_size].value, "28");
    }

    /// Write sits at the right end of a row, by the list's scroll bar. A press within the bar's
    /// grab lane is the bar's (the content gets no tap; under the thumb the list jumps there),
    /// so on a list long enough to scroll a tap on Write must still land on Write.
    #[test]
    fn write_works_on_a_list_that_scrolls() {
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::<EditorMsg>::new()));
        let mut state = with_props();
        state.props_order = PropsOrder::Set;
        // As the .NET agent's SkiaLabel: 116 properties, 111 of them at their default.
        for i in 0..111 {
            state.props.push(Prop { name: format!("Other{i:03}"), type_name: "System.Double".into(), value: "0".into(), is_default: Some(true), ..Default::default() });
        }
        state.rebuild_prop_rows();
        let node = state.selected_node().unwrap().id;
        state.pending.insert((node, "Text".into()), "Hello Designer".into());
        state.live_original.insert((node, "Text".into()), "SkiaLayout types".into());
        let log = sent.clone();
        state.to_hub = Some(Box::new(move |m| log.lock().unwrap().push(m)));
        let mut host = host(state);
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        host.settle();
        let write = host.ui.accessibility_nodes().iter().find(|n| n.label == "Write Text to code").expect("Write on the Text row").rect;
        let at = (write.center_x(), write.center_y());
        host.hover(at.0, at.1);
        host.frame_after(16.0);
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        let written = sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::SetCode { prop, value, .. } if prop == "Text" && value == "Hello Designer"));
        assert!(written, "Write sends Text to the hub: {:?}", sent.lock().unwrap());
        let offset = host.ui.tree.find::<SkiaScroll>(host.ui.state.props_scroll).map(|s| s.viewport_offset_y());
        assert_eq!(offset, Some(0.0), "the list stays where it was");
    }

    #[test]
    fn a_double_tap_folds_a_layer() {
        let mut host = host(connected());
        let first = host.ui.state.tree.nodes[host.ui.state.rows[0].node].id;
        let at = (160.0 + 120.0, 40.0 + 34.0 + 4.0 + view::ROW / 2.0);
        host.pan(at, at, 16.0, 1);
        host.frame_after(100.0);
        assert_eq!(host.ui.state.selected, Some(0), "the first tap selects");
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        assert!(host.ui.state.collapsed.contains(&first), "the second tap folds");
        host.frame_after(800.0);
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        assert!(host.ui.state.collapsed.contains(&first), "a slow tap does not unfold");
    }

    #[test]
    fn settings_switch_the_theme_and_come_back() {
        let mut host = host(connected());
        let gear = host.rect(host.ui.state.settings_button);
        host.tap(gear.center_x(), gear.center_y());
        host.settle();
        assert!(host.ui.state.on_settings, "the gear opens Settings");
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        host.settle();
        {
            let nodes = host.ui.accessibility_nodes();
            let has = |role: &str, label: &str, pressed: Option<bool>| nodes.iter().any(|n| n.role == role && n.label == label && (pressed.is_none() || n.is_pressed == pressed));
            assert!(has(Aria::BUTTON, "Settings, back to the designer", None));
            assert!(has(Aria::RADIO, "Appearance", Some(true)) && has(Aria::RADIO, "About", Some(false)), "sections as radios");
            assert!(has(Aria::RADIO, "Dark", Some(true)) && has(Aria::RADIO_GROUP, "Theme", None));
            assert!(nodes.iter().filter(|n| n.can_interact && n.label.trim().is_empty()).count() == 0, "every control that takes input has a name");
        }
        let light = host.rect(host.ui.state.light_segment);
        let card_picture = |host: &Headless<App>| {
            let wrapper: ControlId = host.ui.state.light_segment.into();
            let card = host.ui.tree.children(wrapper)[0];
            (host.rect(card), host.rect(host.ui.tree.children(host.ui.tree.children(card)[0])[0]))
        };
        let unselected = card_picture(&host);
        let at = (light.center_x(), light.center_y());
        host.pan(at, at, 16.0, 1);
        // The circle starts at the click: on its first frame after the new window is built (its clock
        // starts there), 90 pt away is still old.
        // A 200 ms stall right after the rebuild (a debug build, the shader's first use) moves it one frame only.
        host.frame_after(16.0);
        host.frame_after(200.0);
        let early = host.pixel(at.0 as i32, at.1 as i32 + 90);
        assert!(early.r() < 80, "the circle starts small, 90 pt below the click is still dark: {early:?}");
        // Halfway: the old (dark) window still covers the far corner, the new one shows at the click.
        for _ in 0..12 {
            host.frame_after(16.0);
        }
        assert!(host.ui.state.wipe.is_some(), "the wipe runs");
        assert!(view::wipe_shader_compiles(), "theme-wipe.sksl compiles");
        let corners = [(4, 4), (1275, 4), (4, 815), (1275, 815)];
        let (fx, fy) = corners.into_iter().max_by_key(|(x, y)| (x - at.0 as i32).pow(2) + (y - at.1 as i32).pow(2)).unwrap();
        let far = host.pixel(fx, fy);
        assert!(far.r() < 80, "the far corner still shows the old theme, got {far:?}");
        let near = host.pixel(at.0 as i32 + 6, at.1 as i32 + 46);
        assert!(near.r() > 200, "around the click the new theme shows, got {near:?}");
        // The uncovered window takes the mouse while the picture still opens.
        host.hover(at.0, at.1);
        host.frame_after(16.0);
        let light_card = host.ui.tree.children(host.ui.state.light_segment)[0];
        assert!(host.ui.hovered().contains(&light_card), "hover reaches the new window during the change");
        // Another theme picked while this one still opens: the first one ends at once, the new one starts.
        let first = host.ui.state.wipe;
        let paper = view::Theme::by_id("paper");
        host.ui.state.section = Section::Appearance;
        let dark_card = host.rect(host.ui.state.light_segment);
        let at2 = (dark_card.center_x() + 156.0, dark_card.center_y());
        host.pan(at2, at2, 16.0, 1);
        for _ in 0..4 {
            host.frame_after(16.0);
        }
        assert_ne!(host.ui.state.wipe, first, "a new change runs");
        assert!(first.is_some_and(|f| host.ui.tree.base(f).is_none()), "the first picture is gone");
        // The new picture is the clean light window: no dark leftover of the first circle far away.
        let leftover = host.pixel(fx, fy);
        assert!(leftover.r() > 200, "no half-open first change in the new picture, got {leftover:?}");
        assert_eq!(host.ui.state.settings.theme, paper, "the second pick applies");
        host.settle();
        assert!(host.ui.state.wipe.is_none(), "the wipe is gone at the end");
        assert_eq!(host.ui.state.settings.theme, paper);
        // Back to light for the rest of this test.
        let light = host.rect(host.ui.state.light_segment);
        host.tap(light.center_x(), light.center_y());
        host.settle();
        assert_eq!(host.ui.state.settings.theme, view::Theme::by_id("light"));
        assert!(host.ui.state.on_settings, "the rebuilt window shows Settings again");
        assert_eq!(card_picture(&host), unselected, "selecting a card moves neither it nor its picture");
        // The window is drawn light: the canvas near the bottom right corner (Settings' page).
        let px = host.pixel(1270, 810);
        assert!(px.r() > 200 && px.g() > 200 && px.b() > 200, "light background, got {px:?}");
        // The back button is the arrow and the word "Settings": a click on the word goes back too.
        let back = host.rect(host.ui.state.back_button);
        assert!(back.width() > 70.0, "arrow and title in one button, {back:?}");
        host.tap(back.right - 12.0, back.center_y());
        host.settle();
        assert!(!host.ui.state.on_settings, "a click on \"Settings\" goes back");
        let gear = host.rect(host.ui.state.settings_button);
        host.tap(gear.center_x(), gear.center_y());
        host.settle();
        host.press_key("Escape");
        host.settle();
        assert!(!host.ui.state.on_settings, "Esc goes back to the designer");
        assert!(!host.ui.state.rows.is_empty(), "the tree still shows");
        view::set_theme(view::Theme::by_id("dark"));
    }

    #[test]
    fn the_title_bar_buttons_show_hover_and_press() {
        for theme in [view::Theme::by_id("dark"), view::Theme::by_id("light")] {
            view::set_theme(theme);
            let mut state = connected();
            state.settings.theme = theme;
            let mut host = host(state);
            let gear = host.rect(host.ui.state.settings_button);
            let (x, y) = (gear.left as i32 + 3, gear.top as i32 + 3);
            let before = host.pixel(x, y);
            host.hover(gear.center_x(), gear.center_y());
            host.frame_after(16.0);
            let hovered = host.pixel(x, y);
            let delta = |a: Color, b: Color| (a.r() as i32 - b.r() as i32).abs() + (a.g() as i32 - b.g() as i32).abs() + (a.b() as i32 - b.b() as i32).abs();
            assert!(delta(before, hovered) >= 18, "{theme:?}: hover shows on the gear ({before:?} -> {hovered:?})");
            host.ui.pointer_button(drawnui::PointerKind::Down, MouseButton::Left, gear.center_x(), gear.center_y(), host.time_ms());
            host.frame_after(16.0);
            let pressed = host.pixel(x, y);
            assert!(delta(before, pressed) > delta(before, hovered), "{theme:?}: pressed is stronger than hover");
        }
        view::set_theme(view::Theme::by_id("dark"));
    }

    #[test]
    fn screen_readers_get_names_roles_and_states() {
        let mut state = with_props();
        let node = state.selected_node().unwrap().id;
        state.pending.insert((node, "TextColor".into()), "#FFFF0000".into());
        state.live_original.insert((node, "TextColor".into()), "#FFFFFFFF".into());
        let mut host = host(state);
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        host.settle();
        let nodes = host.ui.accessibility_nodes().to_vec();
        let find = |role: &str, label: &str| nodes.iter().find(|n| n.role == role && n.label.starts_with(label)).unwrap_or_else(|| panic!("no {role} \"{label}\" in {:#?}", nodes.iter().map(|n| (n.role.to_string(), n.label.clone())).collect::<Vec<_>>()));
        assert!(find(Aria::BUTTON, "Settings").can_interact);
        assert!(find(Aria::BUTTON, "Pick a control in the app").is_pressed == Some(false));
        assert_eq!(find(Aria::RADIO, "Set").is_pressed, Some(true), "the current order is checked");
        assert_eq!(find(Aria::RADIO, "Type").is_pressed, Some(false));
        find(Aria::RADIO_GROUP, "Properties order");
        find(Aria::LIST, "Layers");
        find(Aria::LIST, "Properties");
        find(Aria::HEADING, "Layers");
        let row = find(Aria::LIST_ITEM, "SkiaLabel");
        assert!(row.label.contains("level") && row.label.contains("selected"), "{}", row.label);
        let prop = find(Aria::LIST_ITEM, "TextColor");
        assert!(prop.label.contains("changed"), "{}", prop.label);
        find(Aria::BUTTON, "Undo TextColor");
        find(Aria::BUTTON, "Write TextColor to code");
        find(Aria::STATUS, "Hub connected");
        assert!(nodes.iter().filter(|n| n.can_interact && n.label.trim().is_empty()).count() == 0, "every control that takes input has a name");
    }

    #[test]
    fn settings_come_back_as_saved() {
        let saved = Settings {
            theme: view::Theme::by_id("graphite"),
            selection_color: Some("#FFA3E635".into()),
            show_internal: true,
            screens_width: 210.0,
            properties_width: 400.0,
            screens_share: 0.3,
            folded_control_groups: vec!["Media".into()],
            toggled_groups: vec!["Behavior".into()],
            language: Some("de".into()),
            allow_devices: true,
            section: Section::About,
            instant_changes: true,
            write_mode: WriteMode::Always,
        };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert_eq!(back.theme, saved.theme);
        assert!(back.show_internal && back.instant_changes);
        assert_eq!((back.screens_width, back.properties_width), (210.0, 400.0));
        assert_eq!(back.toggled_groups, vec!["Behavior".to_string()]);
        assert_eq!(back.section, Section::About);
        assert_eq!(back.selection_color.as_deref(), Some("#FFA3E635"));
        assert_eq!(back.language.as_deref(), Some("de"));
        assert_eq!(back.write_mode, WriteMode::Always);
        // The file written before panels and sections were saved still loads, with defaults for the rest.
        let old: Settings = serde_json::from_str(r#"{"theme": "graphite", "props_order": "AZ", "show_internal": false, "section": "Properties"}"#).unwrap();
        assert_eq!(old.section, Section::Appearance, "a section that no longer exists opens Appearance");
        assert_eq!(old.theme, view::Theme::by_id("graphite"));
        assert_eq!(old.screens_width, 0.0);
        assert_eq!(old.write_mode, WriteMode::Auto, "written to code before the code is needed, by default");
    }

    #[test]
    fn a_selected_theme_card_does_not_move_at_125() {
        let probe = |id: &str| {
            let mut st = connected();
            st.settings.theme = view::Theme::by_id(id);
            view::set_theme(st.settings.theme);
            let font = |name: &str| assets(&format!("assets/{name}")).expect("font file");
            let ui = Ui::new(st, view::build).font_bytes("Default", &font("OpenSans-Regular.ttf")).font_bytes("Semibold", &font("OpenSans-Semibold.ttf"));
            let mut h = Headless::new(keys(ui), 1600, 1025, 1.25);
            h.settle();
            let gear = h.rect(h.ui.state.settings_button);
            h.tap(gear.center_x(), gear.center_y());
            h.settle();
            let card = h.ui.tree.children(h.ui.state.light_segment)[0];
            let picture = h.ui.tree.children(h.ui.tree.children(card)[0])[0];
            (h.rect(card), h.rect(picture))
        };
        let (unselected, selected) = (probe("dark"), probe("light"));
        view::set_theme(view::Theme::by_id("dark"));
        assert_eq!(unselected, selected, "the Light card and its picture stay put when it is selected, at 125 %");
    }

    #[test]
    fn help_opens_and_goes_back() {
        let mut host = host(connected());
        let help = host.rect(host.ui.state.help_button);
        let gear = host.rect(host.ui.state.settings_button);
        assert!(gear.right <= help.left, "Help is the last one, after Settings");
        host.tap(help.center_x(), help.center_y());
        host.settle();
        assert!(host.ui.state.on_help, "the Help button opens Help");
        let back = host.rect(host.ui.state.help_back_button);
        host.tap(back.right - 10.0, back.center_y());
        host.settle();
        assert!(!host.ui.state.on_help, "its back button goes back");
        host.tap(help.center_x(), help.center_y());
        host.settle();
        host.press_key("Escape");
        host.settle();
        assert!(!host.ui.state.on_help && !host.ui.state.on_settings, "Esc goes back to the designer");
    }

    #[test]
    fn the_language_menu_switches_the_window() {
        let mut host = host(connected());
        let gear = host.rect(host.ui.state.settings_button);
        host.tap(gear.center_x(), gear.center_y());
        host.settle();
        let button = host.rect(host.ui.state.language_button);
        host.tap(button.center_x(), button.center_y());
        host.settle();
        assert!(host.ui.state.popup.is_some(), "the Language button opens its menu");
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        host.settle();
        let deutsch = host.ui.accessibility_nodes().iter().find(|n| n.label == "Deutsch").map(|n| n.id).expect("Deutsch in the menu");
        host.ui.accessibility_activate(deutsch);
        host.settle();
        assert_eq!(host.ui.state.settings.language.as_deref(), Some("de"));
        assert_eq!(i18n::language(), "de");
        assert!(host.ui.state.on_settings, "still on Settings after the rebuild");
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        host.settle();
        let names: Vec<String> = host.ui.accessibility_nodes().iter().map(|n| n.label.clone()).collect();
        assert!(names.iter().any(|n| n.starts_with("Einstellungen")), "the window speaks German: {names:?}");
        i18n::set_language("en");
    }

    #[test]
    fn menus_work_from_the_keyboard() {
        let mut host = host(connected());
        host.ui.set_keyboard_navigation(true);
        host.ui.set_accessibility_enabled(true);
        let gear = host.rect(host.ui.state.settings_button);
        host.tap(gear.center_x(), gear.center_y());
        host.settle();
        let button = host.rect(host.ui.state.language_button);
        host.tap(button.center_x(), button.center_y());
        host.settle();
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let focused = |h: &Headless<App>| h.ui.accessibility_focused().and_then(|f| h.ui.accessibility_nodes().iter().find(|n| n.control == f).map(|n| n.label.clone()));
        assert_eq!(focused(&host).as_deref(), Some("System (English)"), "the open menu focuses its checked item");
        host.press_key("ArrowDown");
        host.press_key("ArrowDown");
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        assert_eq!(focused(&host).as_deref(), Some("Deutsch"), "the arrows move through the items");
        host.press_key("ArrowUp");
        host.press_key("Enter");
        host.settle();
        assert_eq!(host.ui.state.settings.language.as_deref(), Some("en"), "Enter picks");
        i18n::set_language("en");
    }

    /// The lines of the open menus (Layers' and the menu bar's): name, can act, key.
    fn menu_lines(host: &mut Headless<App>) -> Vec<(String, bool, String)> {
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let nodes = host.ui.accessibility_nodes().to_vec();
        let menus: Vec<drawnui::skia::Rect> = nodes.iter().filter(|n| n.role == Aria::MENU).map(|n| n.rect).collect();
        let inside = |r: &drawnui::skia::Rect| menus.iter().any(|m| m.left <= r.center_x() && r.center_x() <= m.right && m.top <= r.center_y() && r.center_y() <= m.bottom);
        nodes
            .iter()
            .filter(|n| [Aria::MENU_ITEM, Aria::MENU_ITEM_CHECKBOX, Aria::MENU_ITEM_RADIO].contains(&n.role.as_ref()) && inside(&n.rect))
            .map(|n| (n.label.clone(), n.can_interact, n.hint.clone()))
            .collect()
    }

    /// Where the line `label` of an open menu is.
    fn line_at(host: &mut Headless<App>, label: &str) -> (f32, f32) {
        host.ui.set_accessibility_enabled(true);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let nodes = host.ui.accessibility_nodes().to_vec();
        let menus: Vec<drawnui::skia::Rect> = nodes.iter().filter(|n| n.role == Aria::MENU).map(|n| n.rect).collect();
        let r = nodes
            .iter()
            .filter(|n| n.label == label && n.role != Aria::MENU && menus.iter().any(|m| m.left <= n.rect.center_x() && n.rect.center_x() <= m.right && m.top <= n.rect.center_y() && n.rect.center_y() <= m.bottom))
            .map(|n| n.rect)
            .next()
            .unwrap_or_else(|| panic!("no line \"{label}\""));
        (r.center_x(), r.center_y())
    }

    /// Taps the line `label` of an open menu.
    fn tap_line(host: &mut Headless<App>, label: &str) {
        let (x, y) = line_at(host, label);
        host.tap(x, y);
        host.settle();
    }

    /// Opens menu `menu` of the bar with a click on its title.
    fn open_bar(host: &mut Headless<App>, menu: usize) {
        let title = host.rect(host.ui.state.bar_titles[menu]);
        host.tap(title.center_x(), title.center_y());
        host.settle();
        assert_eq!(host.ui.state.bar_open, Some(menu));
    }

    fn alt() -> drawnui::Modifiers {
        drawnui::Modifiers { alt: true, ..Default::default() }
    }

    /// The menu bar from the keyboard alone, its menus in windows of their own as on the desktop:
    /// Alt puts the keyboard on the bar, Left and Right walk the titles, Down opens one on its first
    /// line that can act, Up and Down walk the lines (dimmed ones skipped, round at the ends),
    /// Right and Left open and close a submenu or go to the next menu, Enter runs, Esc closes one
    /// level, the last Esc gives the keyboard back.
    /// The keyboard is at one place on the menu bar: its focus (the ring, what a screen reader
    /// reads) is on the highlighted title or line at every step, the menus in windows of their own
    /// as on the desktop. Tab onto a title works as Alt does; Esc gives the focus back.
    #[test]
    fn the_menu_bar_focus_is_where_its_highlight_is() {
        let mut state = with_controls(with_props());
        state.apps[0].info.project = Some("C:/apps/HelloWpf/HelloWpf.csproj".into());
        state.recent = vec![designer_protocol::RecentApp { name: "HelloOld".into(), engine: "dotnet-wpf".into(), project: "C:/apps/HelloOld/HelloOld.csproj".into(), agent: None, last_seen: 0, platform: None }];
        let (mut host, _sent, _to_ui) = wired(state);
        host.popup_windows(true);
        host.ui.set_keyboard_navigation(true);
        host.ui.set_accessibility_enabled(true);
        // The host shows each menu's window under its anchor; then a few frames.
        let settle = |host: &mut Headless<App>| {
            for _ in 0..3 {
                for r in host.popup_window_requests() {
                    host.place_popup_window(r.id, r.size.width, r.size.height, 1.0, drawnui::skia::Point::new(r.anchor.left, r.anchor.bottom));
                }
                host.frame_after(16.0);
            }
            host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        };
        let press = |host: &mut Headless<App>, key: &str| {
            host.key_down(key, Default::default());
            host.key_up(key, Default::default());
            settle(host);
        };
        // Where the highlight is: the lit line of the submenu or the menu, else the title.
        let lit = |host: &Headless<App>| -> Option<drawnui::ControlId> {
            let st = &host.ui.state;
            match (st.bar_sub, st.sub_line, st.bar_open, st.bar_line, st.bar_title) {
                (Some(_), Some(l), ..) => st.sub_line_handles.get(l).map(|h| (*h).into()),
                (_, _, Some(_), Some(l), _) => st.bar_line_handles.get(l).map(|h| (*h).into()),
                (_, _, None, _, Some(t)) => Some(st.bar_titles[t].into()),
                _ => None,
            }
        };
        let same = |host: &Headless<App>, what: &str| {
            assert!(lit(host).is_some(), "{what}: something is lit");
            assert_eq!(host.ui.accessibility_focused(), lit(host), "{what}: the focus is on what is lit");
        };
        // Tab onto File, then Down: File opens on its first line, the focus there.
        let file: drawnui::ControlId = host.ui.state.bar_titles[0].into();
        for _ in 0..40 {
            if host.ui.accessibility_focused() == Some(file) {
                break;
            }
            press(&mut host, "Tab");
        }
        assert_eq!(host.ui.accessibility_focused(), Some(file), "Tab reaches File");
        press(&mut host, "ArrowDown");
        assert_eq!((host.ui.state.bar_open, host.ui.state.bar_line), (Some(0), Some(0)), "Down opens File on its first line");
        same(&host, "File open");
        for (key, what) in [("ArrowDown", "Down"), ("ArrowDown", "Down"), ("ArrowUp", "Up"), ("ArrowUp", "Up, round"), ("Home", "Home")] {
            press(&mut host, key);
            same(&host, what);
        }
        // Into the Run app submenu and back; then the menus to the right and left.
        press(&mut host, "ArrowRight");
        assert!(host.ui.state.bar_sub.is_some());
        same(&host, "the submenu");
        press(&mut host, "ArrowLeft");
        same(&host, "back from the submenu");
        for (key, menu) in [("ArrowLeft", 1), ("ArrowLeft", 0)] {
            press(&mut host, key);
            assert_eq!(host.ui.state.bar_open, Some(menu));
            same(&host, &format!("menu {menu}"));
        }
        // File's Run app opens its submenu: Right goes in, Right again goes on to Edit, then round to File.
        press(&mut host, "ArrowRight");
        if host.ui.state.bar_sub.is_some() {
            same(&host, "Run app");
            press(&mut host, "ArrowRight");
        }
        assert_eq!(host.ui.state.bar_open, Some(1));
        same(&host, "menu 1");
        press(&mut host, "ArrowRight");
        assert_eq!(host.ui.state.bar_open, Some(0));
        same(&host, "menu 0");
        // Esc closes the menu: the focus on its title; Esc again leaves the bar, the focus where
        // Tab put it.
        press(&mut host, "Escape");
        assert_eq!((host.ui.state.bar_open, host.ui.state.bar_title), (None, Some(0)));
        same(&host, "the title after Esc");
        press(&mut host, "Escape");
        assert_eq!(host.ui.state.bar_title, None);
        assert_eq!(host.ui.accessibility_focused(), Some(file), "the focus back where it was");
        // Tab on a title, then Enter, Left and Right: the same.
        press(&mut host, "ArrowRight");
        assert_eq!(host.ui.state.bar_title, Some(1), "Right moves along the titles");
        same(&host, "Edit");
        press(&mut host, "Enter");
        assert_eq!(host.ui.state.bar_open, Some(1));
        same(&host, "Edit open");
        press(&mut host, "Escape");
        press(&mut host, "Escape");
        // Alt from the Controls search field: the focus goes to File, and back to the field after.
        let search: drawnui::ControlId = host.ui.state.controls_search_field.into();
        for _ in 0..60 {
            if host.ui.accessibility_focused() == Some(search) {
                break;
            }
            press(&mut host, "Tab");
        }
        assert_eq!(host.ui.accessibility_focused(), Some(search), "Tab reaches the search field");
        host.key_down("AltLeft", drawnui::Modifiers { alt: true, ..Default::default() });
        host.key_up("AltLeft", Default::default());
        settle(&mut host);
        same(&host, "Alt");
        press(&mut host, "ArrowRight");
        same(&host, "Alt, Right");
        press(&mut host, "ArrowDown");
        same(&host, "Alt, Down");
        press(&mut host, "Escape");
        press(&mut host, "Escape");
        assert_eq!(host.ui.accessibility_focused(), Some(search), "back on the search field");
    }

    #[test]
    fn the_menu_bar_works_from_the_keyboard_alone() {
        let mut state = with_controls(with_props());
        state.apps[0].info.project = Some("C:/apps/HelloWpf/HelloWpf.csproj".into());
        state.recent = vec![designer_protocol::RecentApp { name: "HelloOld".into(), engine: "dotnet-wpf".into(), project: "C:/apps/HelloOld/HelloOld.csproj".into(), agent: None, last_seen: 0, platform: None }];
        let (mut host, _sent, _to_ui) = wired(state);
        host.popup_windows(true);
        host.ui.set_keyboard_navigation(true);
        host.ui.set_accessibility_enabled(true);
        let search: drawnui::ControlId = host.ui.state.controls_search_field.into();
        host.ui.focus(Some(search));
        // A text field with the caret never lets `settle` end: a few frames instead.
        let frames = |host: &mut Headless<App>| {
            for _ in 0..4 {
                host.frame_after(16.0);
            }
        };
        let press = |host: &mut Headless<App>, key: &str| {
            host.key_down(key, Default::default());
            host.key_up(key, Default::default());
            frames(host);
        };
        let alt_alone = |host: &mut Headless<App>| {
            host.key_down("AltLeft", alt());
            host.key_up("AltLeft", Default::default());
            frames(host);
        };
        let at = |host: &Headless<App>| (host.ui.state.bar_title, host.ui.state.bar_open, host.ui.state.bar_line, host.ui.state.sub_line);
        // Alt: the keyboard on the bar, on File, no menu open; the search field lets go of it.
        alt_alone(&mut host);
        assert_eq!(at(&host), (Some(0), None, None, None));
        assert_ne!(host.ui.focused(), Some(search), "the bar has the keys now");
        // Left and Right walk the titles, round at the ends.
        press(&mut host, "ArrowRight");
        assert_eq!(host.ui.state.bar_title, Some(1));
        press(&mut host, "ArrowLeft");
        press(&mut host, "ArrowLeft");
        assert_eq!(host.ui.state.bar_title, Some(1));
        press(&mut host, "ArrowRight");
        // Down opens File on its first line.
        press(&mut host, "ArrowDown");
        assert_eq!(at(&host), (None, Some(0), Some(0), None));
        // Down walks it and comes round: Run app, Restart app, Settings, About, Exit, Run app; Up goes back round.
        for want in [1, 2, 3, 4, 0] {
            press(&mut host, "ArrowDown");
            assert_eq!(host.ui.state.bar_line, Some(want));
        }
        press(&mut host, "ArrowUp");
        assert_eq!(host.ui.state.bar_line, Some(4));
        press(&mut host, "Home");
        // Right on Run app opens its submenu on its first line; Left closes it; Esc too.
        press(&mut host, "ArrowRight");
        assert!(host.ui.state.bar_sub.is_some());
        assert_eq!(host.ui.state.sub_line, Some(0));
        press(&mut host, "ArrowLeft");
        assert!(host.ui.state.bar_sub.is_none());
        assert_eq!(at(&host), (None, Some(0), Some(0), None), "back on Run app");
        press(&mut host, "ArrowRight");
        press(&mut host, "Escape");
        assert!(host.ui.state.bar_sub.is_none() && host.ui.state.bar_open == Some(0), "Esc closes one level");
        // Right on a line without a submenu: the next menu. Edit has nothing to undo and redo: its
        // first line that can act is Copy.
        press(&mut host, "ArrowDown");
        press(&mut host, "ArrowRight");
        assert_eq!(at(&host), (None, Some(1), Some(2), None));
        press(&mut host, "ArrowUp");
        assert_eq!(host.ui.state.bar_line, Some(7), "Up from Copy goes round past the dimmed Undo and Redo to Delete");
        // Esc closes the menu, the keyboard stays on its title; Esc again gives it back.
        press(&mut host, "Escape");
        assert_eq!(at(&host), (Some(1), None, None, None));
        press(&mut host, "Escape");
        assert_eq!(at(&host), (None, None, None, None));
        assert_eq!(host.ui.focused(), Some(search), "the keyboard is back where it was");
        // A menu the mouse opened walks with the keys too; Alt leaves the bar.
        let file = host.rect(host.ui.state.bar_titles[0]);
        host.pan((file.center_x(), file.center_y()), (file.center_x(), file.center_y()), 16.0, 1);
        frames(&mut host);
        assert_eq!(host.ui.state.bar_open, Some(0));
        press(&mut host, "ArrowDown");
        assert_eq!(host.ui.state.bar_line, Some(0));
        alt_alone(&mut host);
        assert_eq!(at(&host), (None, None, None, None));
        // Screen readers: the bar, its titles and the open menu's lines with their names and states.
        alt_alone(&mut host);
        press(&mut host, "ArrowRight");
        press(&mut host, "ArrowRight");
        press(&mut host, "Enter");
        // The host puts the menu's window under its title.
        for r in host.popup_window_requests() {
            host.place_popup_window(r.id, r.size.width, r.size.height, 1.0, drawnui::skia::Point::new(r.anchor.left, r.anchor.bottom));
        }
        frames(&mut host);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let nodes = host.ui.accessibility_nodes().to_vec();
        assert!(nodes.iter().any(|n| n.role == Aria::MENU_BAR && n.label == "Menu bar"));
        assert!(nodes.iter().any(|n| n.role == Aria::MENU_ITEM && n.label == "File" && n.is_pressed == Some(true)), "File reads open");
        assert!(nodes.iter().any(|n| n.role == Aria::MENU_ITEM && n.label == "About"));
    }

    /// Left and Right go round both menus, on the bar and with a menu open; what the keyboard
    /// is on reads selected (the menus' selected color), the mouse over a title only lightly; a
    /// mouse resting on a title or under a menu that opens moves nothing.
    #[test]
    fn the_keyboard_goes_round_all_menus_and_its_place_reads_selected() {
        let (mut host, _sent, _to_ui) = label_selected();
        host.popup_windows(true);
        let press = |host: &mut Headless<App>, key: &str| {
            host.key_down(key, Default::default());
            host.key_up(key, Default::default());
            host.settle();
        };
        // The mouse rests on Edit.
        let edit = host.rect(host.ui.state.bar_titles[1]);
        host.hover(edit.center_x(), edit.center_y());
        host.settle();
        host.key_down("AltLeft", drawnui::Modifiers { alt: true, ..Default::default() });
        host.key_up("AltLeft", Default::default());
        host.settle();
        let mut seen = vec![host.ui.state.bar_title];
        for _ in 0..2 {
            press(&mut host, "ArrowRight");
            seen.push(host.ui.state.bar_title);
        }
        assert_eq!(seen, [Some(0), Some(1), Some(0)], "Right goes round both");
        let mut seen = vec![];
        for _ in 0..2 {
            press(&mut host, "ArrowLeft");
            seen.push(host.ui.state.bar_title);
        }
        assert_eq!(seen, [Some(1), Some(0)], "Left too");
        // The title the keyboard is on reads selected; Edit under the mouse does not light.
        let fill = |host: &mut Headless<App>, menu: usize| {
            let r = host.rect(host.ui.state.bar_titles[menu]);
            host.pixel((r.left + 4.0) as i32, r.center_y() as i32)
        };
        let selected = view::c(view::MENU_SELECTED);
        let near = |a: drawnui::skia::Color, b: Color| (a.r() as i32 - b.r() as i32).abs() + (a.g() as i32 - b.g() as i32).abs() + (a.b() as i32 - b.b() as i32).abs() < 12;
        assert!(near(fill(&mut host, 0), selected), "File reads selected: {:?}", fill(&mut host, 0));
        assert!(!near(fill(&mut host, 1), selected), "Edit under the resting mouse is not lit as selected");
        // With a menu open (opened from the keyboard), Right goes round both, the mouse resting on Edit or not.
        press(&mut host, "ArrowDown");
        let mut seen = vec![host.ui.state.bar_open];
        for _ in 0..2 {
            press(&mut host, "ArrowRight");
            // A submenu line (Run app) opens its submenu first: Right again goes on.
            if host.ui.state.bar_sub.is_some() {
                press(&mut host, "ArrowRight");
            }
            seen.push(host.ui.state.bar_open);
        }
        assert_eq!(seen, [Some(0), Some(1), Some(0)], "with a menu open too");
        for want in [1, 0] {
            press(&mut host, "ArrowLeft");
            assert_eq!(host.ui.state.bar_open, Some(want), "Left with a menu open");
        }
        // The open menu's title and its keyboard line read selected.
        assert!(near(fill(&mut host, 0), selected), "the open menu's title");
        let line = host.ui.state.bar_line.expect("a lit line");
        assert_eq!(line, 2, "Settings, the first line that can act");
        // The mouse moving onto a title takes over again.
        host.hover(edit.right + 60.0, edit.center_y());
        host.settle();
        host.hover(edit.center_x(), edit.center_y());
        host.settle();
        assert_eq!(host.ui.state.bar_open, Some(1), "a real move over Edit opens it");
        assert!(!host.ui.state.bar_keys);
    }

    /// The keyboard's highlight on the bar and in a menu by eye, dark and light:
    /// target/designer-shots/32-menubar-keys-*.png.
    #[test]
    #[ignore]
    fn menubar_keys_shot() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/designer-shots");
        std::fs::create_dir_all(&dir).unwrap();
        for theme in ["dark", "light", "midnight", "graphite", "paper", "high-contrast"] {
            view::set_theme(view::Theme::by_id(theme));
            let mut state = with_controls(with_props());
            state.settings.theme = view::Theme::by_id(theme);
            state.clip = Some(Copied { type_name: "DrawnUi.Draw.SkiaLabel".into(), ..Default::default() });
            let mut host = host(state);
            let press = |host: &mut Headless<App>, key: &str| {
                host.key_down(key, Default::default());
                host.key_up(key, Default::default());
                host.settle();
            };
            host.key_down("AltLeft", drawnui::Modifiers { alt: true, ..Default::default() });
            host.key_up("AltLeft", Default::default());
            host.settle();
            press(&mut host, "ArrowRight");
            for _ in 0..3 {
                host.frame_after(16.0);
            }
            host.save_png(dir.join(format!("32-menubar-keys-bar-{theme}.png")).to_str().unwrap());
            press(&mut host, "ArrowDown");
            press(&mut host, "ArrowDown");
            for _ in 0..3 {
                host.frame_after(16.0);
            }
            host.save_png(dir.join(format!("32-menubar-keys-menu-{theme}.png")).to_str().unwrap());
        }
        view::set_theme(view::Theme::by_id("dark"));
    }

    #[test]
    fn the_menu_bar_opens_walks_and_closes() {
        let (mut host, _sent, _to_ui) = label_selected();
        let names = |lines: &[(String, bool, String)]| lines.iter().map(|l| l.0.clone()).collect::<Vec<_>>();
        // A click on Edit opens it under its title, with each line's key; what cannot act now
        // (nothing to take back, nothing copied) reads unavailable.
        open_bar(&mut host, 1);
        let lines = menu_lines(&mut host);
        assert_eq!(names(&lines), ["Undo", "Redo", "Copy", "Paste", "Duplicate", "Copy style", "Paste style", "Delete"]);
        assert_eq!(lines.iter().map(|l| l.1).collect::<Vec<_>>(), [false, false, true, false, true, true, false, true]);
        assert_eq!(lines.iter().map(|l| l.2.as_str()).collect::<Vec<_>>(), ["Ctrl+Z", "Ctrl+Y", "Ctrl+C", "Ctrl+V", "Ctrl+D", "Ctrl+Shift+C", "Ctrl+Shift+V", "Del"]);
        let title = host.rect(host.ui.state.bar_titles[1]);
        let menu = host.ui.accessibility_nodes().iter().find(|n| n.role == Aria::MENU).map(|n| n.rect).unwrap();
        assert!((menu.left - title.left).abs() < 2.0 && menu.top >= title.bottom - 1.0, "under its title: {menu:?} {title:?}");
        let bar = host.ui.accessibility_nodes().iter().find(|n| n.role == Aria::MENU_BAR).map(|n| n.label.clone());
        assert_eq!(bar.as_deref(), Some("Menu bar"));
        // While a menu is open the mouse over another title opens that one.
        let file = host.rect(host.ui.state.bar_titles[0]);
        host.hover(file.center_x(), file.center_y());
        host.settle();
        assert_eq!(host.ui.state.bar_open, Some(0));
        assert_eq!(names(&menu_lines(&mut host)), ["Run app", "Restart app", "Settings", "About", "Exit"]);
        // A press outside closes it, and the title is no longer lit.
        host.tap(640.0, 500.0);
        host.settle();
        assert_eq!(host.ui.state.bar_open, None);
        // Alt+F opens File from the keyboard with its first line lit; Right and Left go to the next
        // menus, Down walks the menu, Esc closes it.
        host.ui.set_keyboard_navigation(true);
        host.key_down("KeyF", alt());
        host.key_up("KeyF", Default::default());
        host.settle();
        assert_eq!((host.ui.state.bar_open, host.ui.state.bar_line), (Some(0), Some(2)), "its first line that can act: Settings");
        host.press_key("ArrowRight");
        host.settle();
        assert_eq!(host.ui.state.bar_open, Some(1));
        host.press_key("ArrowLeft");
        host.press_key("ArrowLeft");
        host.settle();
        assert_eq!(host.ui.state.bar_open, Some(1), "Left from File goes round to Edit");
        host.press_key("ArrowDown");
        host.settle();
        assert_eq!(host.ui.state.bar_line, Some(4), "Down lights the next line that can act: Copy, then Duplicate");
        host.press_key("Escape");
        host.settle();
        assert_eq!((host.ui.state.bar_open, host.ui.state.bar_title), (None, Some(1)), "Esc closes it, the keyboard stays on Edit");
        host.press_key("Escape");
        host.settle();
        assert_eq!(host.ui.state.bar_title, None, "Esc again leaves the bar");
        // Alt alone puts the keyboard on the bar (File, no menu); again, it leaves.
        host.key_down("AltLeft", alt());
        host.key_up("AltLeft", Default::default());
        host.settle();
        assert_eq!((host.ui.state.bar_open, host.ui.state.bar_title), (None, Some(0)));
        host.key_down("AltLeft", alt());
        host.key_up("AltLeft", Default::default());
        host.settle();
        assert_eq!((host.ui.state.bar_open, host.ui.state.bar_title), (None, None));
        // Alt with something else is no Alt alone.
        host.key_down("AltLeft", alt());
        host.key_down("Tab", alt());
        host.key_up("AltLeft", Default::default());
        host.settle();
        assert_eq!(host.ui.state.bar_open, None);
    }

    /// Each line runs what its button or key runs.
    #[test]
    fn menu_bar_lines_run_what_their_buttons_and_keys_run() {
        let mut state = with_controls(with_props());
        state.apps[0].info.project = Some("C:/apps/HelloWpf/HelloWpf.csproj".into());
        state.recent = vec![designer_protocol::RecentApp { name: "HelloOld".into(), engine: "dotnet-wpf".into(), project: "C:/apps/HelloOld/HelloOld.csproj".into(), agent: None, last_seen: 0, platform: None }];
        let (mut host, sent, _to_ui) = wired(state);
        let sent_any = |f: &dyn Fn(&EditorMsg) -> bool| sent.lock().unwrap().iter().any(f);
        // Ctrl+P: the Pick button, on and off.
        host.key_down("KeyP", ctrl(false));
        host.settle();
        assert!(host.ui.state.pick_mode && sent_any(&|m| matches!(m, EditorMsg::ToApp { msg: HubToApp::PickMode { on: true }, .. })));
        host.key_down("KeyP", ctrl(false));
        host.settle();
        assert!(!host.ui.state.pick_mode, "Ctrl+P turns it off again");
        // Edit, Copy: Ctrl+C, the app reads the selected control.
        open_bar(&mut host, 1);
        tap_line(&mut host, "Copy");
        assert!(sent_any(&|m| matches!(m, EditorMsg::ToApp { msg: HubToApp::Copy { node: 117, .. }, .. })));
        // File, Restart app, and Ctrl+R: the pill's restart.
        open_bar(&mut host, 0);
        tap_line(&mut host, "Restart app");
        assert!(sent_any(&|m| matches!(m, EditorMsg::RestartApp { app: 1 })));
        sent.lock().unwrap().clear();
        host.ui.state.runs.clear();
        host.key_down("KeyR", ctrl(false));
        host.settle();
        assert!(sent_any(&|m| matches!(m, EditorMsg::RestartApp { app: 1 })), "Ctrl+R");
        // File, Run app, a recent app: the apps menu's run.
        open_bar(&mut host, 0);
        tap_line(&mut host, "Run app");
        tap_line(&mut host, "HelloOld");
        assert!(sent_any(&|m| matches!(m, EditorMsg::RunApp { project } if project.ends_with("HelloOld.csproj"))));
        // F1: the Help page; Ctrl+,: Settings; File, About: its section.
        host.key_down("F1", Default::default());
        host.settle();
        assert!(host.ui.state.on_help, "F1");
        host.press_key("Escape");
        host.settle();
        host.key_down("Comma", ctrl(false));
        host.settle();
        assert!(host.ui.state.on_settings, "Ctrl+,");
        host.press_key("Escape");
        host.settle();
        open_bar(&mut host, 0);
        tap_line(&mut host, "About");
        assert!(host.ui.state.on_settings && host.ui.state.section == Section::About);
    }

    /// Every language underlines a different letter in each title, on a key of its usual layout.
    #[test]
    fn each_language_has_one_letter_per_menu() {
        for (code, _) in i18n::LANGUAGES {
            i18n::set_language(code);
            let keys: Vec<Option<&str>> = (0..view::menubar::MENU_COUNT).map(view::menubar::title_key).collect();
            assert!(keys.iter().all(Option::is_some), "{code}: every title has a letter {keys:?}");
            let mut unique = keys.clone();
            unique.sort();
            unique.dedup();
            assert_eq!(unique.len(), keys.len(), "{code}: no two titles share a letter {keys:?}");
        }
        i18n::set_language("en");
    }

    #[test]
    fn the_color_picker_sets_live_and_esc_puts_it_back() {
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::<EditorMsg>::new()));
        let mut state = with_props();
        let log = sent.clone();
        state.to_hub = Some(Box::new(move |m| log.lock().unwrap().push(m)));
        let mut host = host(state);
        host.ui.set_accessibility_enabled(true);
        // TextColor (#FFFFFFFF), the fifth row of A-Z: its value opens the picker.
        let list = host.rect(host.ui.state.props_list);
        let at = (1280.0 - 340.0 + 200.0, list.top + 4.0 + 5.0 * view::ROW + view::ROW / 2.0);
        host.pan(at, at, 16.0, 1);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        assert!(host.ui.state.picker.is_some() && host.ui.state.popup.is_some(), "a color opens the picker");
        let plane = host.ui.accessibility_nodes().iter().find(|n| n.label == "Saturation and brightness").map(|n| n.rect).expect("the square");
        // Drag to the top right corner: full saturation and brightness of hue 0 = red.
        host.pan((plane.left + 10.0, plane.bottom - 10.0), (plane.right + 30.0, plane.top - 30.0), 120.0, 6);
        host.frame_after(16.0);
        let value = host.ui.state.edit_value.clone();
        assert_eq!(value, "#FFFF0000", "the dragged color");
        let live = sent.lock().unwrap().iter().any(|m| matches!(m, EditorMsg::ToApp { msg: HubToApp::SetProperty { name, value, .. }, .. } if name == "TextColor" && value == "#FFFF0000"));
        assert!(live, "set live in the app");
        host.press_key("Escape");
        host.frame_after(16.0);
        assert!(host.ui.state.popup.is_none() && host.ui.state.picker.is_none(), "Esc closes the picker");
        let back = sent.lock().unwrap().iter().rev().find_map(|m| match m {
            EditorMsg::ToApp { msg: HubToApp::SetProperty { name, value, .. }, .. } if name == "TextColor" => Some(value.clone()),
            _ => None,
        });
        assert_eq!(back.as_deref(), Some("#FFFFFFFF"), "Esc puts the old color back");
        assert!(host.ui.state.pending.is_empty());
    }

    /// A click on Layers' empty space below the rows lets go of the selection; a click on a row selects.
    #[test]
    fn a_click_below_the_rows_deselects() {
        let mut host = host(connected_to("designersample/home.json", "DesignerSample"));
        let rows = host.ui.state.rows.len();
        let row = row_at(1);
        host.pan(row, row, 16.0, 1);
        host.settle();
        assert!(host.ui.state.selected.is_some(), "a row click selects");
        let below = (row.0, row_at(rows).1 + view::ROW * 3.0);
        host.pan(below, below, 16.0, 1);
        host.settle();
        assert_eq!(host.ui.state.selected, None, "the empty space deselects");
        assert!(host.ui.state.props.is_empty());
    }

    /// Enter in the color picker keeps the color and closes it: after a drag on the square, and
    /// after typing a hex color in its field.
    #[test]
    fn enter_in_the_color_picker_keeps_the_color_and_closes_it() {
        let mut host = host(with_props());
        host.ui.set_accessibility_enabled(true);
        let list = host.rect(host.ui.state.props_list);
        let at = (1280.0 - 340.0 + 200.0, list.top + 4.0 + 5.0 * view::ROW + view::ROW / 2.0);
        let key = |host: &Headless<App>| host.ui.state.pending.iter().find(|((_, n), _)| n == "TextColor").map(|(_, v)| v.clone());
        // A drag to red, then Enter.
        host.pan(at, at, 16.0, 1);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let plane = host.ui.accessibility_nodes().iter().find(|n| n.label == "Saturation and brightness").map(|n| n.rect).expect("the square");
        host.pan((plane.left + 10.0, plane.bottom - 10.0), (plane.right + 30.0, plane.top - 30.0), 120.0, 6);
        host.frame_after(16.0);
        host.press_key("Enter");
        host.frame_after(16.0);
        assert!(host.ui.state.popup.is_none() && host.ui.state.picker.is_none() && host.ui.state.edit_prop.is_none(), "Enter closes the picker");
        assert_eq!(key(&host).as_deref(), Some("#FFFF0000"), "the color is kept");
        // A hex typed in the field, then Enter.
        host.pan(at, at, 16.0, 1);
        host.frame_after(drawnui::ACCESSIBILITY_INTERVAL_MS);
        let hex = host.ui.accessibility_nodes().iter().find(|n| n.label == "Hex color").map(|n| n.rect).expect("the hex field");
        host.pan((hex.center_x(), hex.center_y()), (hex.center_x(), hex.center_y()), 16.0, 1);
        host.frame_after(16.0);
        host.press_key("End");
        for _ in 0..12 {
            host.press_key("Backspace");
        }
        host.type_text("00FF00");
        host.frame_after(16.0);
        host.press_key("Enter");
        host.frame_after(16.0);
        assert!(host.ui.state.popup.is_none() && host.ui.state.picker.is_none(), "Enter in the hex field closes the picker");
        assert_eq!(key(&host).as_deref(), Some("#FF00FF00"), "the typed color is kept");
        assert!(host.ui.focused().is_none(), "nothing keeps the keyboard");
    }

    #[test]
    fn the_themes_file_is_complete() {
        assert!(view::Theme::all().count() >= 6);
        for t in view::Theme::all() {
            assert_eq!(view::Theme::by_id(t.id()), t, "ids are unique");
        }
        assert_eq!(view::Theme::by_id("Dark"), view::Theme::by_id("dark"), "an older settings.json still finds its theme");
    }

    /// Emoji in app texts (a label's text in Layers and Properties) are drawn, not boxes. Looked at
    /// by eye: target/designer-shots/22-emoji.png.
    #[test]
    #[ignore]
    fn emoji_shot() {
        let mut state = with_props();
        for p in state.props.iter_mut().filter(|p| p.name == "Text") {
            p.value = "Hello 😀 👍 🔊 ⚙ ❤".into();
        }
        let row = state.selected.unwrap();
        let n = state.rows[row].node;
        state.tree.nodes[n].text = Some("Hello 😀 👍 🔊".into());
        let mut host = host(state);
        for _ in 0..3 {
            host.frame_after(16.0);
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/designer-shots");
        std::fs::create_dir_all(&dir).expect("shots folder");
        host.save_png(dir.join("22-emoji.png").to_str().unwrap());
        // The Text value opened for editing: the edit field shows the same emoji.
        let text_row = host.ui.state.prop_rows.iter().position(|r| matches!(r, PropRow::Prop(i) if host.ui.state.props[*i].name == "Text")).unwrap();
        let top = host.rect(host.ui.state.props_list).top + 4.0 + view::ROW + view::ROW * text_row as f32 + view::ROW / 2.0;
        let at = (1280.0 - 340.0 + 300.0, top);
        host.pan(at, at, 16.0, 1);
        for _ in 0..4 {
            host.frame_after(16.0);
        }
        host.save_png(dir.join("22-emoji-edit.png").to_str().unwrap());
    }

    #[test]
    fn the_first_edit_starts_with_the_caret_at_the_end() {
        let mut host = host(with_props());
        let top = host.rect(host.ui.state.props_list).top + 4.0 + view::ROW + view::ROW / 2.0;
        // FontSize (28), right of its value: the caret after it.
        let at = (1280.0 - 340.0 + 300.0, top);
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        host.frame_after(16.0);
        host.type_text("5");
        host.press_key("Enter");
        host.frame_after(16.0);
        assert!(host.ui.state.pending.values().any(|v| v == "285"), "typing appends: {:?}", host.ui.state.pending);
    }

    #[test]
    fn a_tap_in_the_value_puts_the_caret_there() {
        let mut host = host(with_props());
        let top = host.rect(host.ui.state.props_list).top + 4.0 + view::ROW + view::ROW / 2.0;
        // FontSize (28): the field starts 148 pt into the row, after the - stepper and the
        // field's padding; a tap just after the "2" puts the caret between 2 and 8.
        let probe = (1280.0 - 340.0 + 200.0, top);
        host.pan(probe, probe, 16.0, 1);
        host.frame_after(16.0);
        host.frame_after(16.0);
        host.press_key("Escape");
        host.frame_after(16.0);
        let field = host.rect(host.ui.state.editor);
        let between = (field.left + 6.0 + 7.5, top);
        host.pan(between, between, 16.0, 1);
        host.frame_after(16.0);
        host.frame_after(16.0);
        host.type_text("5");
        host.press_key("Enter");
        host.frame_after(16.0);
        assert!(host.ui.state.pending.values().any(|v| v == "258"), "the caret sat where the tap was: {:?}", host.ui.state.pending);
    }

    #[test]
    fn an_edit_starts_with_the_caret_at_the_end() {
        let mut host = host(with_props());
        let top = host.rect(host.ui.state.props_list).top + 4.0 + view::ROW + view::ROW / 2.0;
        let at = (1280.0 - 340.0 + 200.0, top);
        // FontSize (28), twice: the second edit reuses the field.
        for _ in 0..2 {
            host.pan(at, at, 16.0, 1);
            host.frame_after(16.0);
            host.press_key("Escape");
            host.frame_after(16.0);
        }
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        host.type_text("5");
        host.press_key("Enter");
        host.frame_after(16.0);
        assert!(host.ui.state.pending.values().any(|v| v == "285"), "typing appends: {:?}", host.ui.state.pending);
        assert!(host.ui.focused().is_none(), "Enter lets go of the keyboard");
    }

    /// Editing a value, then selecting another control in Layers: the edit ends and the field
    /// lets go of the keyboard (it stayed focused over the new control's properties).
    #[test]
    fn selecting_another_control_ends_the_edit() {
        let mut host = host(with_props());
        let top = host.rect(host.ui.state.props_list).top + 4.0 + view::ROW + view::ROW / 2.0;
        let at = (1280.0 - 340.0 + 200.0, top);
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        let editor = host.ui.state.editor.into();
        assert!(host.ui.state.edit_prop.is_some() && host.ui.focused() == Some(editor), "editing FontSize");
        let other = row_at(row_of(&host, 118));
        host.pan(other, other, 16.0, 1);
        host.frame_after(16.0);
        assert_eq!(host.ui.state.selected_node().map(|n| n.id), Some(118));
        assert_eq!(host.ui.state.edit_prop, None, "the edit ended");
        assert_ne!(host.ui.focused(), Some(editor), "the field lets go of the keyboard");
        host.type_text("7");
        host.frame_after(16.0);
        assert!(host.ui.state.pending.is_empty(), "typing edits nothing: {:?}", host.ui.state.pending);
    }

    #[test]
    fn escape_cancels_an_edit() {
        let mut host = host(with_props());
        let top = host.rect(host.ui.state.props_list).top + 4.0 + view::ROW + view::ROW / 2.0;
        let at = (1280.0 - 340.0 + 200.0, top);
        // FontSize (28): typed text, then Esc.
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        host.type_text("99");
        host.frame_after(16.0);
        host.press_key("Escape");
        host.frame_after(16.0);
        assert_eq!(host.ui.state.edit_prop, None, "Esc ends the edit");
        assert!(host.ui.focused().is_none(), "the field lets go of the keyboard");
        assert!(host.ui.state.pending.is_empty(), "nothing was applied");
        // +1 applies live; Esc takes it back.
        host.pan(at, at, 16.0, 1);
        host.frame_after(16.0);
        // The + stepper: the last thing in the edit field's row.
        let field = host.rect(host.ui.state.edit_overlay);
        let plus = (field.right - 6.0, field.center_y());
        host.pan(plus, plus, 16.0, 1);
        host.frame_after(16.0);
        assert_eq!(host.ui.state.pending.len(), 1, "a step applies live");
        host.press_key("Escape");
        host.frame_after(16.0);
        assert!(host.ui.state.pending.is_empty(), "Esc takes the step back");
        assert_eq!(host.ui.state.edit_prop, None);
    }

    #[test]
    #[ignore]
    fn shots() {
        let recent = |name: &str, project: &str| designer_protocol::RecentApp { name: name.into(), engine: "dotnet-wpf".into(), project: project.into(), agent: None, last_seen: 0, platform: None };
        let mut none = App { connected: true, ..Default::default() };
        none.recent = vec![recent("DesignerSample", "C:/x/DesignerSample"), recent("HelloWpf", "C:/x/HelloWpf")];
        shot(none, "1-no-app");
        // Editing in place: FontSize being typed, TextColor changed in the app but not in code.
        let mut state = with_props();
        let node = state.selected_node().map(|n| n.id).unwrap();
        state.pending.insert((node, "TextColor".into()), "#FFFF0000".into());
        state.live_original.insert((node, "TextColor".into()), "#FFFFFFFF".into());
        let mut host = host(state);
        let top = host.rect(host.ui.state.props_list).top + 4.0 + view::ROW + view::ROW / 2.0;
        let at = (1280.0 - 340.0 + 200.0, top);
        host.pan(at, at, 16.0, 1);
        for _ in 0..5 {
            host.frame_after(16.0);
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/designer-shots");
        std::fs::create_dir_all(&dir).expect("shots folder");
        host.save_png(dir.join("7-inline-edit.png").to_str().unwrap());
        // The choices dropdown of HorizontalOptions (second A-Z row), the mouse over "Center".
        let (x, y) = (1280.0 - 340.0 + 200.0, top + view::ROW);
        host.pan((x, y), (x, y), 16.0, 1);
        for _ in 0..3 {
            host.frame_after(16.0);
        }
        // "Center", the second item: under the row, past the panel's 4 pt padding.
        host.hover(x + 20.0, y + view::ROW / 2.0 + 4.0 + view::ROW + 2.0 + view::ROW / 2.0);
        for _ in 0..3 {
            host.frame_after(16.0);
        }
        host.save_png(dir.join("8-menu.png").to_str().unwrap());
        // The color picker of TextColor (the fifth A-Z row).
        host.press_key("Escape");
        host.settle();
        let (x, y) = (1280.0 - 340.0 + 200.0, top + 4.0 * view::ROW);
        host.pan((x, y), (x, y), 16.0, 1);
        for _ in 0..3 {
            host.frame_after(16.0);
        }
        host.save_png(dir.join("21-color-picker.png").to_str().unwrap());
        shot(connected(), "2-connected");
        // The gear and the Pick button under the mouse.
        {
            let mut h = super::shots::host(connected());
            let gear = h.rect(h.ui.state.settings_button);
            h.hover(gear.center_x(), gear.center_y());
            h.frame_after(16.0);
            h.save_png(dir.join("15-hover-gear.png").to_str().unwrap());
        }
        // Settings and About, dark and light; the designer in light.
        let settings_shot = |section: Section, theme: view::Theme, name: &str| {
            view::set_theme(theme);
            let mut state = connected();
            state.section = section;
            state.settings.theme = theme;
            let mut h = super::shots::host(state);
            let gear = h.rect(h.ui.state.settings_button);
            h.tap(gear.center_x(), gear.center_y());
            h.settle();
            h.save_png(dir.join(format!("{name}.png")).to_str().unwrap());
        };
        settings_shot(Section::Appearance, view::Theme::by_id("dark"), "10-settings");
        // The theme change halfway.
        {
            view::set_theme(view::Theme::by_id("dark"));
            let mut h = super::shots::host(connected());
            let gear = h.rect(h.ui.state.settings_button);
            h.tap(gear.center_x(), gear.center_y());
            h.settle();
            let light = h.rect(h.ui.state.light_segment);
            let at = (light.center_x(), light.center_y());
            h.pan(at, at, 16.0, 1);
            for (i, frames) in [4, 6, 10].iter().enumerate() {
                for _ in 0..*frames {
                    h.frame_after(16.0);
                }
                h.save_png(dir.join(format!("17-theme-change-{}.png", i + 1)).to_str().unwrap());
            }
            h.settle();
            view::set_theme(view::Theme::by_id("dark"));
        }
        settings_shot(Section::Layers, view::Theme::by_id("dark"), "16-layers-settings");
        settings_shot(Section::About, view::Theme::by_id("dark"), "11-about");
        for lang in ["de", "ru", "sr"] {
            i18n::set_language(lang);
            settings_shot(Section::Appearance, view::Theme::by_id("dark"), &format!("20-settings-{lang}"));
            shot(with_props(), &format!("20-designer-{lang}"));
            view::set_theme(view::Theme::by_id("dark"));
            let mut h = super::shots::host(connected());
            let help = h.rect(h.ui.state.help_button);
            h.tap(help.center_x(), help.center_y());
            h.settle();
            h.save_png(dir.join(format!("20-help-{lang}.png")).to_str().unwrap());
        }
        i18n::set_language("en");
        {
            view::set_theme(view::Theme::by_id("dark"));
            let mut h = super::shots::host(connected());
            let help = h.rect(h.ui.state.help_button);
            h.tap(help.center_x(), help.center_y());
            h.settle();
            h.save_png(dir.join("19-help.png").to_str().unwrap());
        }
        settings_shot(Section::About, view::Theme::by_id("light"), "12-about-light");
        settings_shot(Section::Connection, view::Theme::by_id("light"), "13-connection-light");
        for t in view::Theme::all() {
            view::set_theme(t);
            let mut st = with_props();
            st.settings.theme = t;
            shot(st, &format!("18-theme-{}", t.id()));
        }
        view::set_theme(view::Theme::by_id("light"));
        let mut light = with_props();
        light.settings.theme = view::Theme::by_id("light");
        shot(light, "14-designer-light");
        view::set_theme(view::Theme::by_id("dark"));
        let mut selected = connected();
        let row = selected.rows.iter().position(|r| selected.tree.nodes[r.node].field.is_some()).or_else(|| selected.rows.iter().position(|r| selected.tree.nodes[r.node].type_name.ends_with("SkiaLabel"))).unwrap();
        selected.selected = Some(row);
        let prop = |name: &str, type_name: &str, value: &str, is_set: bool, group: &str| Prop {
            name: name.into(),
            type_name: type_name.into(),
            value: value.into(),
            is_set,
            group: Some(group.into()),
            choices: (name == "HorizontalOptions").then(|| ["Start", "Center", "End", "Fill"].map(String::from).to_vec()),
            flags: false,
            is_default: None,
        };
        selected.props = vec![
            prop("FontSize", "System.Double", "28", true, "Text"),
            prop("HorizontalOptions", "DrawnUi.Views.LayoutOptions", "Fill", true, "Layout"),
            prop("Margin", "DrawnUi.Views.Thickness", "0,8,0,4", true, "Layout"),
            prop("Text", "System.String", "SkiaLayout types", true, "Text"),
            prop("TextColor", "DrawnUi.Color", "#FFFFFFFF", true, "Text"),
            prop("BackgroundColor", "DrawnUi.Color", "#00000000", false, "Look"),
            prop("HeightRequest", "System.Double", "-1", false, "Layout"),
            prop("MaxLines", "System.Int32", "-1", false, "Text"),
            prop("Opacity", "System.Double", "1", false, "Look"),
            prop("Padding", "DrawnUi.Views.Thickness", "0,0,0,0", false, "Layout"),
            prop("Rotation", "System.Double", "0", false, "Transform"),
            prop("UseCache", "DrawnUi.Draw.SkiaCacheType", "Operations", false, "Behavior"),
            prop("WidthRequest", "System.Double", "-1", false, "Layout"),
        ];
        selected.props.sort_by(|a, b| (!a.is_set, &a.name).cmp(&(!b.is_set, &b.name)));
        selected.rebuild_prop_rows();
        let mut choices = selected.clone_for_shot();
        let mut set_order = selected.clone_for_shot();
        set_order.props_order = PropsOrder::Set;
        set_order.rebuild_prop_rows();
        shot(set_order, "9-set-order");
        selected.edit_prop = Some("FontSize".into());
        selected.edit_value = "34".into();
        selected.edit_step = Some(1.0);
        selected.status = "Written to LayoutsPage.cs, line 41".into();
        choices.edit_prop = Some("HorizontalOptions".into());
        choices.edit_value = "Fill".into();
        choices.edit_choices = ["Start", "Center", "End", "Fill"].map(String::from).to_vec();
        choices.pick_mode = true;
        choices.props_order = PropsOrder::Groups;
        choices.rebuild_prop_rows();
        shot(choices, "6-choices-pick");
        shot(selected, "3-editing");
        let mut sample = connected_to("designersample/home.json", "DesignerSample");
        sample.recent = vec![recent("HelloWpf", "C:/x/HelloWpf")];
        sample.runs.insert("C:/x/HelloWpf".into(), (designer_protocol::RunState::Building, String::new()));
        shot(sample, "4-sample");
        shot_at(connected_to("designersample/home.json", "DesignerSample"), "5-sample-125", 1.25);
    }
}
