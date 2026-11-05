//! The desktop menu bar, at the far left of the title bar: File and Edit. A click opens a
//! menu, Alt with a title's underlined letter (or Alt alone) opens one from the keyboard, Left and
//! Right go to the next menu, Up and Down through it; Escape or a click outside closes it. Each line
//! runs the same code as its button or its key, shows that key, and reads unavailable when it
//! cannot act now.

use super::*;
use crate::ui::MenuAction;

/// One command of the menu bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Cmd {
    /// File: Run app opens the recent apps that are not running; a pick runs one.
    Run,
    Recent(usize),
    Restart,
    Settings,
    Exit,
    /// Edit: the Layers menu's actions (Undo and Redo first).
    Act(MenuAction),
    /// Keys only (Ctrl+P, F1): Pick is the Layers header's button, Help the title bar's.
    Pick,
    Help,
    About,
}

/// The menus: their title (its ftl key, "&" before the underlined letter) and lines; None is a
/// separator. Run app opens the submenu 4.
const MENUS: [(&str, &[Option<Cmd>]); 2] = [
    ("menubar-file", &[Some(Cmd::Run), Some(Cmd::Restart), None, Some(Cmd::Settings), Some(Cmd::About), None, Some(Cmd::Exit)]),
    (
        "menubar-edit",
        &[
            Some(Cmd::Act(MenuAction::Undo)),
            Some(Cmd::Act(MenuAction::Redo)),
            None,
            Some(Cmd::Act(MenuAction::Copy)),
            Some(Cmd::Act(MenuAction::Paste)),
            Some(Cmd::Act(MenuAction::Duplicate)),
            None,
            Some(Cmd::Act(MenuAction::CopyStyle)),
            Some(Cmd::Act(MenuAction::PasteStyle)),
            None,
            Some(Cmd::Act(MenuAction::Delete)),
        ],
    ),
];

pub(crate) const MENU_COUNT: usize = MENUS.len();

/// Menu `menu`'s entries now (4: the recent apps). Exit only where the designer
/// has a window of its own.
fn entries(menu: usize, app: &App) -> Vec<Option<Cmd>> {
    match menu {
        4 => (0..app.recent.len()).map(|i| Some(Cmd::Recent(i))).collect(),
        m => {
            let mut list: Vec<Option<Cmd>> = MENUS[m].1.to_vec();
            if cfg!(target_os = "emscripten") && m == 0 {
                list.truncate(list.len() - 2);
            }
            list
        }
    }
}

/// A title as shown, and the place of its underlined letter.
pub(crate) fn title_text(menu: usize) -> (String, Option<usize>) {
    let raw = tr(MENUS[menu].0);
    match raw.find('&') {
        Some(at) => {
            let mut text = raw.clone();
            text.remove(at);
            (text, Some(raw[..at].chars().count()))
        }
        None => (raw, None),
    }
}

/// The key (W3C code) under a letter on the usual layout of the language: Latin letters where
/// QWERTY has them, Cyrillic where the Russian layout has them.
fn letter_key(letter: char) -> Option<&'static str> {
    const LATIN: [&str; 26] = [
        "KeyA", "KeyB", "KeyC", "KeyD", "KeyE", "KeyF", "KeyG", "KeyH", "KeyI", "KeyJ", "KeyK", "KeyL", "KeyM", "KeyN", "KeyO", "KeyP", "KeyQ", "KeyR", "KeyS", "KeyT", "KeyU", "KeyV", "KeyW", "KeyX", "KeyY", "KeyZ",
    ];
    const CYRILLIC: [(char, &str); 33] = [
        ('Й', "KeyQ"), ('Ц', "KeyW"), ('У', "KeyE"), ('К', "KeyR"), ('Е', "KeyT"), ('Н', "KeyY"), ('Г', "KeyU"), ('Ш', "KeyI"), ('Щ', "KeyO"), ('З', "KeyP"), ('Х', "BracketLeft"),
        ('Ъ', "BracketRight"), ('Ф', "KeyA"), ('Ы', "KeyS"), ('В', "KeyD"), ('А', "KeyF"), ('П', "KeyG"), ('Р', "KeyH"), ('О', "KeyJ"), ('Л', "KeyK"), ('Д', "KeyL"), ('Ж', "Semicolon"),
        ('Э', "Quote"), ('Я', "KeyZ"), ('Ч', "KeyX"), ('С', "KeyC"), ('М', "KeyV"), ('И', "KeyB"), ('Т', "KeyN"), ('Ь', "KeyM"), ('Б', "Comma"), ('Ю', "Period"), ('Ё', "Backquote"),
    ];
    let up = letter.to_uppercase().next()?;
    if up.is_ascii_uppercase() {
        return Some(LATIN[(up as u8 - b'A') as usize]);
    }
    CYRILLIC.iter().find(|(c, _)| *c == up).map(|(_, k)| *k)
}

/// The key of menu `menu`'s underlined letter.
pub(crate) fn title_key(menu: usize) -> Option<&'static str> {
    let (text, at) = title_text(menu);
    at.and_then(|i| text.chars().nth(i)).and_then(letter_key)
}

/// The menu whose underlined letter is on `key`.
fn menu_for_key(key: &str) -> Option<usize> {
    (0..MENU_COUNT).find(|&m| title_key(m) == Some(key))
}

/// A key as the platform writes it: Ctrl+Shift+C, or ⇧⌘C on macOS.
pub(super) fn key_text((key, command, shift): (&str, bool, bool)) -> String {
    let mac = cfg!(target_os = "macos");
    let key = match key {
        "Delete" if mac => "\u{232B}",
        "Delete" => "Del",
        "Comma" => ",",
        k => k.strip_prefix("Key").unwrap_or(k),
    };
    match (mac, command, shift) {
        (true, true, true) => format!("\u{21E7}\u{2318}{key}"),
        (true, true, false) => format!("\u{2318}{key}"),
        (false, true, true) => format!("Ctrl+Shift+{key}"),
        (false, true, false) => format!("Ctrl+{key}"),
        _ => key.to_owned(),
    }
}

impl Cmd {
    fn label(self, app: &App) -> String {
        match self {
            Cmd::Run => tr("menu-run-app"),
            Cmd::Recent(i) => app.recent.get(i).map(|r| r.name.clone()).unwrap_or_default(),
            Cmd::Restart => tr("menu-restart-app"),
            Cmd::Settings => tr("menu-settings"),
            Cmd::Exit => tr("menu-exit"),
            Cmd::Act(a) => action_label(app, a),
            Cmd::Pick => tr("pick"),
            Cmd::Help => tr("help"),
            Cmd::About => tr("menu-about"),
        }
    }

    /// The key that does it with the menus closed: (W3C code, Ctrl / Command, Shift).
    pub(crate) fn shortcut(self) -> Option<(&'static str, bool, bool)> {
        match self {
            Cmd::Restart => Some(("KeyR", true, false)),
            Cmd::Settings => Some(("Comma", true, false)),
            Cmd::Act(a) => Some(a.shortcut()),
            Cmd::Pick => Some(("KeyP", true, false)),
            Cmd::Help => Some(("F1", false, false)),
            _ => None,
        }
    }

    /// The submenu it opens.
    fn submenu(self) -> Option<usize> {
        match self {
            Cmd::Run => Some(4),
            _ => None,
        }
    }

    /// It can act now (else it reads unavailable).
    pub(crate) fn enabled(self, app: &App) -> bool {
        match self {
            Cmd::Run => !app.recent.is_empty(),
            Cmd::Recent(i) => app.recent.get(i).is_some_and(|r| !app.runs.contains_key(&r.project)),
            Cmd::Restart => app.apps.get(app.pill_slot()).is_some_and(|a| a.info.project.is_some()),
            Cmd::Act(a) => app.action_enabled(a),
            Cmd::Pick => app.app.is_some(),
            _ => true,
        }
    }

    /// On or off, for a check box or radio line.
    fn checked(self, app: &App) -> Option<bool> {
        match self {
            Cmd::Pick => Some(app.pick_mode),
            _ => None,
        }
    }

    fn role(self) -> &'static str {
        match self {
            Cmd::Pick => Aria::MENU_ITEM_CHECKBOX,
            _ => Aria::MENU_ITEM,
        }
    }

    /// Runs it: the same code as its button or key.
    pub(crate) fn run(self, app: &mut App, cx: &mut Cx) {
        if !self.enabled(app) {
            return;
        }
        match self {
            Cmd::Run => {}
            Cmd::Recent(i) => {
                if let Some(project) = app.recent.get(i).map(|r| r.project.clone()) {
                    app.run_project(project, cx);
                }
            }
            Cmd::Restart => app.restart_pill(0, cx),
            Cmd::Settings => app.open_settings(cx),
            Cmd::Exit => {
                if app.closing(cx) {
                    cx.close_app();
                }
            }
            Cmd::Act(a) => app.run_action(a, cx),
            Cmd::Pick => app.toggle_pick(),
            Cmd::Help => app.open_help(cx),
            Cmd::About => {
                app.show_section(Section::About);
                app.open_settings(cx);
            }
        }
    }
}

/// Menu `menu`'s width in points (lines fill it).
fn width(menu: usize, app: &App) -> f32 {
    let longest = entries(menu, app).into_iter().flatten().map(|c| c.label(app).chars().count() + c.shortcut().map_or(2, |k| key_text(k).chars().count() + 3)).max().unwrap_or(8);
    menu_width(longest)
}

/// The menu bar: one title per menu.
pub(super) fn menu_bar(app: &mut App) -> Build<SkiaLayout> {
    SkiaRow::new()
        .spacing(0)
        .vertical_options(LayoutOptions::Center)
        .accessibility_role(Aria::MENU_BAR)
        .accessibility_label(tr("menubar"))
        .keyboard_wrap(true)
        .children((0..MENU_COUNT).map(|m| title(m, &mut app.bar_titles[m])).collect::<Vec<_>>())
}

/// A title: a click opens its menu; while a menu is open the mouse over another title opens that
/// one. Its underlined letter with Alt opens it from the keyboard.
fn title(menu: usize, handle: &mut Handle<SkiaShape>) -> Build<SkiaShape> {
    let (text_now, at) = title_text(menu);
    let chars: Vec<char> = text_now.chars().collect();
    let (before, letter, after): (String, String, String) = match at {
        Some(i) if i < chars.len() => (chars[..i].iter().collect(), chars[i].to_string(), chars[i + 1..].iter().collect()),
        _ => (text_now.clone(), String::new(), String::new()),
    };
    let hint = at.and_then(|i| chars.get(i)).map(|c| format!("Alt+{}", c.to_uppercase())).unwrap_or_default();
    SkiaShape::new()
        .corner_radius(5)
        .height_request(26)
        .padding((9, 0))
        .stroke_width(1)
        .vertical_options(LayoutOptions::Center)
        .accessibility_role(Aria::MENU_ITEM)
        .accessibility_can_interact(Some(true))
        .accessibility_label(text_now.clone())
        .accessibility_hint(hint)
        .assign(handle)
        .observe(move |me, app: &App| {
            let open = app.bar_open == Some(menu);
            // Open, or the keyboard on it: the selected look of the menus, edged in the accent.
            // The mouse over it: the light hover veil (not while the keyboard drives the bar).
            let strong = open || app.bar_title == Some(menu);
            let hover = !strong && !app.bar_keys && app.bar_hover == Some(menu);
            me.set_background_color(if strong { c(MENU_SELECTED) } else { veiled(Color::TRANSPARENT, hover, false) });
            me.set_stroke_color(if strong { c(ACCENT) } else { Color::TRANSPARENT });
            me.set_accessibility_is_pressed(Some(open));
        })
        .on_hovered(move |_me, app: &mut App, cx, on| {
            app.hover_changes = app.hover_changes.wrapping_add(1);
            if on {
                app.bar_hover = Some(menu);
                // While the keyboard drives the bar, a mouse resting on a title moves nothing (a
                // menu opening or closing near it is no move); a real move takes over.
                if app.mouse_takes_over() && app.bar_open.is_some_and(|open| open != menu) {
                    app.bar_keys = false;
                    app.open_bar_menu(menu, false, cx);
                }
            } else if app.bar_hover == Some(menu) {
                app.bar_hover = None;
            }
        })
        .on_tapped(move |_me, app: &mut App, cx| {
            if !left_click(cx) {
                return;
            }
            app.bar_keys = false;
            if app.bar_open == Some(menu) {
                app.close_bar(cx);
            } else {
                // From the keyboard (Enter, Space, a screen reader): the first line ready.
                let keyboard = cx.gesture().is_none();
                app.open_bar_menu(menu, keyboard, cx);
            }
        })
        .children(
            text(12.5, TEXT)
                .vertical_options(LayoutOptions::Center)
                .accessibility_role(Aria::PRESENTATION)
                .observe(move |me, app: &App| me.set_text_color(c(if app.bar_open == Some(menu) || app.bar_title == Some(menu) { ON_SELECTED } else { TEXT })))
                .spans((TextSpan::new(before), TextSpan::new(letter).underline(true), TextSpan::new(after))),
        )
}

/// A menu of the bar (`level` 0) or a submenu (1).
/// A menu of the bar (`level` 0) or a submenu (1), and its lines (the keyboard's focus goes onto them).
fn bar_menu(app: &App, menu: usize, level: usize) -> (Build<SkiaShape>, Vec<Handle<SkiaShape>>) {
    let list = entries(menu, app);
    let (mut rows, mut handles) = (vec![], vec![]);
    let mut line = 0;
    for entry in list {
        match entry {
            None => rows.push(menu_separator()),
            Some(cmd) => {
                let mut h = Handle::default();
                rows.push(bar_line(app, cmd, level, line).assign(&mut h));
                handles.push(h);
                line += 1;
            }
        }
    }
    let name = if menu < MENU_COUNT { title_text(menu).0 } else { tr("menu-run-app") };
    let panel = menu_panel(rows, width(menu, app)).accessibility_label(name).keyboard_wrap(true).consume_gestures(|_me, app: &mut App, _cx, g| app.note_pointer(g));
    (panel, handles)
}

/// One line of a menu: a check where it is on, its name, its key or the submenu's arrow at the
/// right; lit under the mouse and the keyboard; dimmed and unavailable when it cannot act.
fn bar_line(app: &App, cmd: Cmd, level: usize, line: usize) -> Build<SkiaShape> {
    let enabled = cmd.enabled(app);
    let checked = cmd.checked(app);
    let sub = cmd.submenu();
    let keys = cmd.shortcut().map(key_text).unwrap_or_default();
    let label = cmd.label(app);
    let lit = move |app: &App| if level == 0 { app.bar_line } else { app.sub_line } == Some(line);
    // Lit by the keyboard: the selected look edged in the accent; by the mouse: the light hover.
    let strong = move |app: &App| lit(app) && app.bar_keys;
    let mut item = SkiaShape::new()
        .corner_radius(4)
        .fill_x()
        .height_request(ROW)
        .padding((6, 0, 10, 0))
        .accessibility_role(cmd.role())
        .accessibility_label(label.clone())
        .accessibility_can_interact(Some(enabled))
        .stroke_width(1)
        .observe(move |me, app: &App| {
            me.set_background_color(if strong(app) { c(MENU_SELECTED) } else if lit(app) { c(MENU_HOVER) } else { Color::TRANSPARENT });
            me.set_stroke_color(if strong(app) { c(ACCENT) } else { Color::TRANSPARENT });
        })
        .on_hovered(move |_me, app: &mut App, cx, on| {
            if !on {
                return;
            }
            // A menu opening under a resting mouse leaves the keyboard's line lit.
            if !app.mouse_takes_over() {
                return;
            }
            let at = if enabled { Some(line) } else { None };
            app.bar_keys = false;
            if level == 0 {
                app.bar_line = at;
                // A submenu opens under the mouse; another line closes the one open.
                match sub.filter(|_| enabled) {
                    Some(_) if app.bar_sub.map(|s| s.0) != Some(cmd) => app.open_bar_sub(cmd, false, cx),
                    Some(_) => {}
                    None => app.close_bar_sub(cx),
                }
            } else {
                app.sub_line = at;
            }
        })
        .on_tapped(move |_me, app: &mut App, cx| {
            if !enabled || !left_click(cx) {
                return;
            }
            if sub.is_some() {
                let keyboard = cx.gesture().is_none();
                app.open_bar_sub(cmd, keyboard, cx);
                return;
            }
            app.leave_bar(cx);
            cmd.run(app, cx);
        });
    if !keys.is_empty() {
        item = item.accessibility_hint(keys.clone());
    }
    if let Some(on) = checked {
        item = item.accessibility_is_pressed(Some(on));
    }
    let tone = if enabled { TEXT } else { FAINT };
    item.children((
        SkiaRow::new().spacing(8).vertical_options(LayoutOptions::Center).children((
            SkiaSvg::from_string(icons::CHECK).width_request(13).height_request(13).tint_color(c(ACCENT)).vertical_options(LayoutOptions::Center).opacity(if checked == Some(true) { 1.0 } else { 0.0 }),
            text(12.0, tone).text(label).vertical_options(LayoutOptions::Center).observe(move |me, app: &App| {
                if enabled {
                    me.set_text_color(c(if strong(app) { ON_SELECTED } else { TEXT }));
                }
            }),
        )),
        match sub {
            Some(_) => drawnui::Detached::from(icon(icons::CHEVRON_RIGHT, 13.0, if enabled { MUTED } else { FAINT }).horizontal_options(LayoutOptions::End).accessibility_role(Aria::PRESENTATION)),
            None => drawnui::Detached::from(
                text(11.0, FAINT)
                    .text(keys)
                    .horizontal_options(LayoutOptions::End)
                    .vertical_options(LayoutOptions::Center)
                    .accessibility_role(Aria::PRESENTATION)
                    .observe(move |me, app: &App| me.set_text_color(c(if strong(app) { ON_SELECTED } else { FAINT }))),
            ),
        },
    ))
}

fn menu_window() -> PopupWindowOptions {
    PopupWindowOptions { cpu: true, ..Default::default() }
}

/// Menu `menu`'s lines (separators left out) and whether each can act now.
fn lines(menu: usize, app: &App) -> Vec<(Cmd, bool)> {
    entries(menu, app).into_iter().flatten().map(|c| (c, c.enabled(app))).collect()
}

/// The line the keyboard starts on in a menu: its chosen option (a radio line that is on), else its
/// first line that can act.
fn first_line(menu: usize, app: &App) -> Option<usize> {
    let list = lines(menu, app);
    list.iter().position(|(c, ok)| *ok && c.role() == Aria::MENU_ITEM_RADIO && c.checked(app) == Some(true)).or_else(|| step(&list, None, "Home"))
}

/// The line the arrow `key` moves to from `from`: the next one that can act, round past either
/// end; Home and End the first and last such.
fn step(lines: &[(Cmd, bool)], from: Option<usize>, key: &str) -> Option<usize> {
    let ok: Vec<usize> = (0..lines.len()).filter(|&i| lines[i].1).collect();
    let (first, last) = (*ok.first()?, *ok.last()?);
    Some(match (key, from) {
        ("Home", _) | ("ArrowDown", None) => first,
        ("End", _) | ("ArrowUp", None) => last,
        ("ArrowDown", Some(f)) => ok.iter().copied().find(|&i| i > f).unwrap_or(first),
        (_, Some(f)) => ok.iter().rev().copied().find(|&i| i < f).unwrap_or(last),
        _ => first,
    })
}

impl App {
    /// Opens menu `menu` under its title; `keyboard`: its first line that can act is lit.
    pub(crate) fn open_bar_menu(&mut self, menu: usize, keyboard: bool, cx: &mut Cx) {
        self.close_popup(cx);
        self.close_bar(cx);
        let (content, handles) = bar_menu(self, menu, 0);
        self.bar_line_handles = handles;
        self.bar_popup = Some(cx.open_popup_window(content, self.bar_titles[menu], menu_window()));
        (self.bar_open, self.bar_title) = (Some(menu), None);
        self.bar_line = if keyboard { first_line(menu, self) } else { None };
        if keyboard {
            self.enter_bar(cx);
            self.sync_bar_focus(cx);
        }
    }

    /// Opens `cmd`'s submenu to the right of its line.
    pub(crate) fn open_bar_sub(&mut self, cmd: Cmd, keyboard: bool, cx: &mut Cx) {
        let (Some(menu), Some(sub)) = (self.bar_open, cmd.submenu()) else { return };
        self.close_bar_sub(cx);
        // The menu opens under its title, left edges together: the line's place follows from the
        // rows above it (a line ROW high, a separator 5, 2 between, 4 of padding).
        let Some(base) = cx.base(self.bar_titles[menu]) else { return };
        let title = points_rect(base);
        let mut top = title.bottom + 4.0;
        for entry in entries(menu, self) {
            if entry == Some(cmd) {
                break;
            }
            top += if entry.is_some() { ROW } else { 5.0 } + 2.0;
        }
        let at = drawnui::skia::Point::new(title.left + width(menu, self) + 8.0, top - 4.0);
        let (content, handles) = bar_menu(self, sub, 1);
        self.sub_line_handles = handles;
        self.bar_sub = Some((cmd, cx.open_popup_window(content, at, menu_window())));
        self.sub_line = if keyboard { first_line(sub, self) } else { None };
        if keyboard {
            self.enter_bar(cx);
            self.sync_bar_focus(cx);
        }
    }

    /// The keyboard's focus (its ring, what a screen reader reads) goes where the bar's highlight is:
    /// the lit line of the submenu or the menu, else the title the keyboard is on.
    fn sync_bar_focus(&mut self, cx: &mut Cx) {
        let target = match (self.bar_sub, self.sub_line, self.bar_open, self.bar_line, self.bar_title) {
            (Some(_), Some(l), ..) => self.sub_line_handles.get(l).copied(),
            (_, _, Some(_), Some(l), _) => self.bar_line_handles.get(l).copied(),
            (_, _, Some(m), None, _) | (_, _, None, _, Some(m)) => Some(self.bar_titles[m]),
            _ => None,
        };
        if let Some(h) = target {
            cx.set_accessibility_focus(h);
        }
    }

    pub(crate) fn close_bar_sub(&mut self, cx: &mut Cx) {
        if let Some((_, popup)) = self.bar_sub.take() {
            cx.close_popup_window(popup);
        }
        self.sub_line = None;
    }

    /// Closes the open menu and its submenu.
    pub(crate) fn close_bar(&mut self, cx: &mut Cx) {
        self.close_bar_sub(cx);
        if let Some(popup) = self.bar_popup.take() {
            cx.close_popup_window(popup);
        }
        (self.bar_open, self.bar_line) = (None, None);
    }

    /// The keyboard comes to the menu bar: it takes the keys from a text field, and remembers
    /// where they were, to give them back.
    fn enter_bar(&mut self, cx: &mut Cx) {
        (self.bar_keys, self.keys_moves) = (true, self.pointer_moves);
        if self.bar_return.is_none() {
            self.bar_return = Some(cx.focused());
            self.bar_return_focus = cx.accessibility_focused();
            cx.focus(Some(self.bar_titles[0]));
        }
    }

    /// The menu bar lets go: menus closed, the keyboard back where it was.
    pub(crate) fn leave_bar(&mut self, cx: &mut Cx) {
        self.close_bar(cx);
        (self.bar_title, self.bar_keys) = (None, false);
        if let Some(back) = self.bar_return.take() {
            cx.focus(back);
            // The screen reader's focus goes back too: onto the field that had the keys, else where
            // it was (a title, when Tab brought it there).
            if let Some(f) = back.or(self.bar_return_focus.take()) {
                cx.set_accessibility_focus(f);
            }
            self.bar_return_focus = None;
        }
    }

    /// A popup closed, for any reason (a click outside, the window losing the keyboard): the bar follows.
    pub(crate) fn bar_popup_closed(&mut self, id: PopupId, cx: &mut Cx) {
        if self.bar_sub.is_some_and(|s| s.1 == id) {
            (self.bar_sub, self.sub_line) = (None, None);
        }
        if self.bar_popup == Some(id) {
            self.bar_popup = None;
            self.leave_bar(cx);
        }
    }

    /// Line `line` of menu `menu`, from the keyboard: a submenu opens, a command runs (the bar
    /// lets go first).
    fn activate(&mut self, menu: usize, line: Option<usize>, cx: &mut Cx) {
        let Some((cmd, true)) = line.and_then(|l| lines(menu, self).get(l).copied()) else { return };
        if cmd.submenu().is_some() {
            self.open_bar_sub(cmd, true, cx);
        } else {
            self.leave_bar(cx);
            cmd.run(self, cx);
        }
    }

    /// The menu bar's keys, before the window's others, as in Windows menus. Alt with a title's
    /// letter opens its menu. On the bar: Left and Right move between the titles; Down, Enter or
    /// Space open one. In a menu: Up and Down move through the lines that can act (round at the
    /// ends), Right opens a submenu or goes to the next menu, Left closes the submenu or goes to
    /// the menu before, Enter or Space runs the line, Esc closes one level. True = used.
    pub(crate) fn bar_key(&mut self, event: &KeyEvent, cx: &mut Cx) -> bool {
        let used = self.bar_key_used(event, cx);
        // The keyboard drives the bar until the mouse moves; its focus is where the highlight is.
        if used && (self.bar_open.is_some() || self.bar_title.is_some()) {
            (self.bar_keys, self.keys_moves) = (true, self.pointer_moves);
            self.sync_bar_focus(cx);
        }
        used
    }

    fn bar_key_used(&mut self, event: &KeyEvent, cx: &mut Cx) -> bool {
        if self.embedded {
            return false;
        }
        let m = event.modifiers();
        let key = event.key;
        // Escape: the canvas closed the top popup already, before the window's keys (KeyEvent::handled).
        // One level, as the bar's own Escape: a submenu, or the menu with the keyboard kept on its title.
        if event.handled
            && key == "Escape"
            && let Some(open) = self.bar_open
        {
            if self.bar_sub.take().is_some() {
                self.sub_line = None;
            } else {
                self.enter_bar(cx);
                self.close_bar(cx);
                self.bar_title = Some(open);
            }
            return true;
        }
        // Enter or Space the canvas's navigation pressed already: a title opened its menu (the keyboard
        // comes along), a line ran.
        // The press reaches the title as a tap, so the menu opened with no line lit: light its first.
        if event.handled && matches!(key, "Enter" | "NumpadEnter" | "Space") {
            let Some(open) = self.bar_open else { return false };
            self.enter_bar(cx);
            match self.bar_sub {
                Some((cmd, _)) if self.sub_line.is_none() => self.sub_line = first_line(cmd.submenu().unwrap_or(0), self),
                None if self.bar_line.is_none() => self.bar_line = first_line(open, self),
                _ => {}
            }
            return true;
        }
        // Alt alone (pressed and let go) puts the keyboard on the bar; anything with it is no "Alt alone".
        self.alt_alone = matches!(key, "AltLeft" | "AltRight") && !m.ctrl && !m.shift && !m.meta;
        if m.alt && !m.ctrl
            && let Some(menu) = menu_for_key(key)
        {
            self.enter_bar(cx);
            self.open_bar_menu(menu, true, cx);
            return true;
        }
        if m.ctrl || m.alt || m.meta {
            return false;
        }
        let n = MENU_COUNT;
        // Tab put the keyboard on a title: the bar's keys work from there as after Alt.
        if self.bar_title.is_none()
            && self.bar_open.is_none()
            && matches!(key, "ArrowLeft" | "ArrowRight" | "ArrowDown" | "Enter" | "NumpadEnter" | "Space")
            && let Some(t) = cx.accessibility_focused().and_then(|f| self.bar_titles.iter().position(|h| drawnui::ControlId::from(*h) == f))
        {
            self.enter_bar(cx);
            self.bar_title = Some(t);
            // Left or Right the canvas's navigation used already moved the focus to this title.
            if event.handled && matches!(key, "ArrowLeft" | "ArrowRight") {
                return true;
            }
        }
        let Some(open) = self.bar_open.filter(|_| self.bar_popup.is_some_and(|p| cx.is_popup_window_open(p))) else {
            let Some(t) = self.bar_title else { return false };
            match key {
                "ArrowLeft" => self.bar_title = Some((t + n - 1) % n),
                "ArrowRight" => self.bar_title = Some((t + 1) % n),
                "ArrowDown" | "Enter" | "NumpadEnter" | "Space" => self.open_bar_menu(t, true, cx),
                "Escape" => self.leave_bar(cx),
                _ => return false,
            }
            return true;
        };
        if !matches!(key, "ArrowDown" | "ArrowUp" | "Home" | "End" | "ArrowLeft" | "ArrowRight" | "Enter" | "NumpadEnter" | "Space" | "Escape") {
            return false;
        }
        // A menu the mouse opened: the keyboard comes along.
        self.enter_bar(cx);
        if let Some((cmd, _)) = self.bar_sub {
            let sub = cmd.submenu().unwrap_or(0);
            match key {
                "ArrowLeft" | "Escape" => self.close_bar_sub(cx),
                "ArrowRight" => self.open_bar_menu((open + 1) % n, true, cx),
                "Enter" | "NumpadEnter" | "Space" => self.activate(sub, self.sub_line, cx),
                _ => self.sub_line = step(&lines(sub, self), self.sub_line, key),
            }
            return true;
        }
        match key {
            "ArrowRight" => match self.bar_line.and_then(|l| lines(open, self).get(l).copied()) {
                Some((cmd, true)) if cmd.submenu().is_some() => self.open_bar_sub(cmd, true, cx),
                _ => self.open_bar_menu((open + 1) % n, true, cx),
            },
            "ArrowLeft" => self.open_bar_menu((open + n - 1) % n, true, cx),
            "Enter" | "NumpadEnter" | "Space" => self.activate(open, self.bar_line, cx),
            "Escape" => {
                self.close_bar(cx);
                self.bar_title = Some(open);
            }
            _ => self.bar_line = step(&lines(open, self), self.bar_line, key),
        }
        true
    }

    /// Where the mouse is, from every pointer move (the window's and the menus'): a hover that comes
    /// without a move (a menu opened or closed under a resting mouse) is told apart. Never uses it.
    pub(crate) fn note_pointer(&mut self, g: &Gesture) -> bool {
        if g.kind == GestureKind::Pointer && self.pointer_at != Some((g.location.x, g.location.y)) {
            self.pointer_at = Some((g.location.x, g.location.y));
            self.pointer_moves = self.pointer_moves.wrapping_add(1);
        }
        false
    }

    /// The mouse has the menus: the keyboard does not drive them, or the mouse moved since it did.
    pub(crate) fn mouse_takes_over(&self) -> bool {
        !self.bar_keys || self.pointer_moves != self.keys_moves
    }

    /// Alt let go with nothing pressed with it: the keyboard goes onto the bar (its first title),
    /// or, when it is there, back where it was.
    pub(crate) fn bar_key_up(&mut self, event: &KeyEvent, cx: &mut Cx) -> bool {
        if self.embedded || !matches!(event.key, "AltLeft" | "AltRight") || !std::mem::take(&mut self.alt_alone) {
            return false;
        }
        if self.bar_open.is_some() || self.bar_title.is_some() {
            self.leave_bar(cx);
        } else {
            self.enter_bar(cx);
            self.bar_title = Some(0);
            self.sync_bar_focus(cx);
        }
        true
    }
}
