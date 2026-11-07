//! How the window looks: a dark IDE theme with three panels (Screens, Layers, Properties), a title
//! bar and a status bar. State and messages live in `ui.rs`; this file only builds controls.

use drawnui::prelude::*;

use super::{App, Inbox, PropRow, PropsOrder, SECTIONS, Section, short_type};
use crate::i18n::{self, tr, tr_args};
use crate::icons;
use designer_protocol::HubToApp;

pub(super) mod menubar;

// Themes: palettes from assets/themes.json, one type scale (11 / 12 / 13 / 15), one spacing step (4).
// Every color is a role (`Tone`); `c` takes its value in the current theme.

/// The themes, read from `assets/themes.json` (edit that file to change or add one): in the order
/// Settings shows them.
const THEMES_JSON: &str = include_str!("../../assets/themes.json");

/// One theme as the file defines it.
#[derive(serde::Deserialize)]
struct ThemeDef {
    id: String,
    name: String,
    #[serde(default)]
    light: bool,
    colors: std::collections::HashMap<String, String>,
}

/// One theme ready to draw with: every role's color.
struct Palette {
    id: String,
    name: String,
    light: bool,
    colors: [u32; ROLES],
}

/// The themes file, checked: an unknown role, a missing role or a bad color names the theme and the role.
fn parse_themes(json: &str) -> Result<Vec<Palette>, String> {
    #[derive(serde::Deserialize)]
    struct File {
        themes: Vec<ThemeDef>,
    }
    let file: File = serde_json::from_str(json).map_err(|e| format!("themes.json: {e}"))?;
    file.themes
        .into_iter()
        .map(|t| {
            if let Some(unknown) = t.colors.keys().find(|k| !ROLE_NAMES.contains(&k.as_str())) {
                return Err(format!("theme {}: unknown role {unknown}", t.id));
            }
            let mut colors = [0u32; ROLES];
            for (i, role) in ROLE_NAMES.iter().enumerate() {
                let value = t.colors.get(*role).ok_or_else(|| format!("theme {}: no color for {role}", t.id))?;
                colors[i] = value
                    .strip_prefix('#')
                    .filter(|h| h.len() == 6)
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .ok_or_else(|| format!("theme {}: {role} = {value} is not #RRGGBB", t.id))?;
            }
            Ok(Palette { id: t.id, name: t.name, light: t.light, colors })
        })
        .collect()
}

fn palettes() -> &'static [Palette] {
    static PALETTES: std::sync::OnceLock<Vec<Palette>> = std::sync::OnceLock::new();
    PALETTES.get_or_init(|| parse_themes(THEMES_JSON).unwrap_or_else(|e| panic!("{e}")))
}

/// The designer's look: one of the themes in `assets/themes.json`. Saved by its id.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Theme(usize);

impl Theme {
    /// Every theme, in the file's order.
    pub fn all() -> impl Iterator<Item = Theme> {
        (0..palettes().len()).map(Theme)
    }

    /// The theme with this id (any case), else the first one.
    pub fn by_id(id: &str) -> Theme {
        Theme(palettes().iter().position(|p| p.id.eq_ignore_ascii_case(id)).unwrap_or(0))
    }

    pub fn id(self) -> &'static str {
        &palettes()[self.0].id
    }

    pub fn name(self) -> &'static str {
        &palettes()[self.0].name
    }

    /// Light backgrounds: dark veils, logos for light backgrounds.
    pub fn is_light(self) -> bool {
        palettes()[self.0].light
    }

    fn palette(self) -> &'static [u32; ROLES] {
        &palettes()[self.0].colors
    }

    /// A role's color in this theme (the Settings previews draw every theme at once).
    fn color(self, t: Tone) -> Color {
        hex(self.palette()[t.0])
    }
}

impl serde::Serialize for Theme {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.id())
    }
}

impl<'de> serde::Deserialize<'de> for Theme {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Theme, D::Error> {
        let id = String::deserialize(d)?;
        Ok(Theme::by_id(&id))
    }
}

thread_local! {
    static THEME: std::cell::Cell<Theme> = const { std::cell::Cell::new(Theme(0)) };
}

/// Sets the theme the next build draws with (the window is built again to show it).
pub fn set_theme(theme: Theme) {
    THEME.with(|t| t.set(theme));
}

fn theme() -> Theme {
    THEME.with(|t| t.get())
}

fn is_light() -> bool {
    theme().is_light()
}

/// A color role: its index in every palette.
#[derive(Clone, Copy)]
pub struct Tone(usize);

const ROLES: usize = 33;

const BG: Tone = Tone(0); // window, behind the panels
const PANEL: Tone = Tone(1);
pub(super) const HEADER: Tone = Tone(2);
pub(super) const LINE: Tone = Tone(3); // separators and borders
const HOVER: Tone = Tone(4);
const SELECTION: Tone = Tone(5);
pub(super) const TEXT: Tone = Tone(6);
pub(super) const MUTED: Tone = Tone(7);
pub(super) const FAINT: Tone = Tone(8);
pub(super) const ACCENT: Tone = Tone(9);
const FIELD: Tone = Tone(10); // a control's field name
const OK: Tone = Tone(11);
const WARN: Tone = Tone(12);
const ERROR: Tone = Tone(13);
pub(super) const INPUT: Tone = Tone(14);
const ON_ACCENT: Tone = Tone(15); // text and icons on an accent fill
const MENU_HOVER: Tone = Tone(16); // a menu row under the mouse
pub(super) const MENU_SELECTED: Tone = Tone(17); // the menu row of the current value
const MENU_SELECTED_HOVER: Tone = Tone(18);
const ON_SELECTED: Tone = Tone(19); // the current value's text in a menu
const SWITCH_OFF: Tone = Tone(20); // a switch's track when off
const SWITCH_THUMB_OFF: Tone = Tone(21);
const GROUP_HEADER: Tone = Tone(22); // a group's header row, darker than the panel
const TYPE_LAYOUT: Tone = Tone(23);
const TYPE_TEXT: Tone = Tone(24);
const TYPE_INPUT: Tone = Tone(25);
const TYPE_MEDIA: Tone = Tone(26);
const TYPE_OWN: Tone = Tone(27); // the app's own classes
const GUIDE: Tone = Tone(28);
const GUIDE_BOX: Tone = Tone(29);
const GUIDE_SIGN: Tone = Tone(30);
/// The Layers panel's background: a step darker than the side panels in a dark theme, a step
/// lighter in a light one, so the tree reads as the work area.
const LAYERS: Tone = Tone(31);
/// The names in Layers: a little dimmer than the text of the side panels, so the tools read first.
const LAYERS_TEXT: Tone = Tone(32);

/// The role names the themes file uses, in `Tone` order.
const ROLE_NAMES: [&str; ROLES] = ["bg", "panel", "header", "line", "hover", "selection", "text", "muted", "faint", "accent", "field", "ok", "warn", "error", "input", "on_accent", "menu_hover", "menu_selected", "menu_selected_hover", "on_selected", "switch_off", "switch_thumb_off", "group_header", "type_layout", "type_text", "type_input", "type_media", "type_own", "guide", "guide_box", "guide_sign", "layers", "layers_text"];

/// A role's color in the current theme.
pub fn c(t: Tone) -> Color {
    theme().color(t)
}

/// The same for drawing with Skia directly.
fn sk(t: Tone) -> drawnui::skia::Color {
    drawnui::skia::Color::new(0xFF00_0000 | theme().palette()[t.0])
}

/// What a clickable control lays over its background: a light veil while the mouse is on it, a
/// stronger one while pressed. Translucent, so it shows on every surface of both themes.
fn veil(pressed: bool) -> Color {
    match (is_light(), pressed) {
        (false, false) => Color::from_argb(0x1C, 0xFF, 0xFF, 0xFF),
        (false, true) => Color::from_argb(0x30, 0xFF, 0xFF, 0xFF),
        (true, false) => Color::from_argb(0x12, 0x00, 0x00, 0x00),
        (true, true) => Color::from_argb(0x22, 0x00, 0x00, 0x00),
    }
}

/// `base` with the veil over it (an opaque base stays opaque).
fn veiled(base: Color, hovered: bool, pressed: bool) -> Color {
    if !hovered && !pressed {
        return base;
    }
    let v = veil(pressed);
    if base.a() == 0 {
        return v;
    }
    let a = v.a() as f32 / 255.0;
    let mix = |b: u8, o: u8| (b as f32 * (1.0 - a) + o as f32 * a).round() as u8;
    Color::from_argb(base.a(), mix(base.r(), v.r()), mix(base.g(), v.g()), mix(base.b(), v.b()))
}

/// Hover and press looks for a clickable control: its background is `base(app, list index)` with
/// the veil over it while the mouse is on it or presses it.
pub(super) fn interactive<T: Control>(b: Build<T>, base: impl Fn(&App, Option<usize>) -> Color + 'static) -> Build<T> {
    b.on_hovered(|me, app: &mut App, _cx, over| {
        app.hover_changes = app.hover_changes.wrapping_add(1);
        if over {
            app.hovered_control = Some(me.id());
        } else if app.hovered_control == Some(me.id()) {
            app.hovered_control = None;
        }
        if !over && app.pressed == Some(me.id()) {
            app.pressed = None;
        }
    })
    .on_down(|me, app: &mut App, _cx, _at| app.pressed = Some(me.id()))
    .on_up(|_me, app: &mut App, _cx, _at| app.pressed = None)
    .observe(move |me, app: &App| {
        let bg = veiled(base(app, me.base().context_index), me.base().is_hovered(), app.pressed == Some(me.id()));
        me.set_background_color(bg);
    })
}

/// The window's background in the current theme (the canvas behind everything).
pub fn window_background() -> Color {
    c(BG)
}

pub const ROW: f32 = 26.0;
/// Text size of the names in the Controls panel and the Properties list (one size, so they never drift).
pub const LIST_TEXT: f32 = 12.0;

pub fn hex(rgb: u32) -> Color {
    Color::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub(super) fn text(size: f32, color: Tone) -> Build<SkiaLabel> {
    // App texts can hold emoji and symbols the window's fonts lack: the system font that has them
    // draws them (only those characters; the rest keeps the window's font).
    SkiaLabel::new("").font_size(size).text_color(c(color)).max_lines(1).system_font_fallback(true)
}

fn bold(size: f32, color: Tone) -> Build<SkiaLabel> {
    text(size, color).font_family("Semibold")
}

fn icon(svg: &'static str, size: f32, color: Tone) -> Build<SkiaSvg> {
    SkiaSvg::from_string(svg).width_request(size).height_request(size).tint_color(c(color)).vertical_options(LayoutOptions::Center)
}

/// The icon of a clickable control `button`: MUTED at rest, full TEXT while the mouse is over the
/// control, ACCENT while `active` says the control is on (Pick).
fn button_icon(svg: &'static str, size: f32, button: Handle<SkiaShape>, active: fn(&App) -> bool) -> Build<SkiaSvg> {
    icon(svg, size, MUTED).observe(move |me, app: &App| {
        let tone = if active(app) {
            ACCENT
        } else if app.hovered_control == Some(button.into()) {
            TEXT
        } else {
            MUTED
        };
        me.set_tint_color(c(tone));
    })
}

/// The icon and its color for a control type: layouts blue, text white, inputs green, media violet,
/// the app's own classes amber.
fn type_icon(type_name: &str, own_class: bool) -> (&'static str, Tone) {
    if own_class {
        return (icons::FILE_CODE, TYPE_OWN);
    }
    match short_type(type_name) {
        "SkiaStack" | "SkiaLayout" | "ContentLayout" => (icons::ROWS_3, TYPE_LAYOUT),
        "SkiaRow" => (icons::COLUMNS_3, TYPE_LAYOUT),
        "SkiaLayer" | "SkiaViewSwitcher" => (icons::LAYERS, TYPE_LAYOUT),
        "SkiaGrid" | "SkiaDecoratedGrid" => (icons::GRID_3X3, TYPE_LAYOUT),
        "SkiaWrap" | "SkiaMasonry" => (icons::TEXT_WRAP, TYPE_LAYOUT),
        "SkiaScroll" => (icons::SCROLL_TEXT, TYPE_LAYOUT),
        "SkiaShell" => (icons::APP_WINDOW, TYPE_LAYOUT),
        "SkiaLabel" | "SkiaRichLabel" | "SkiaMarkdownLabel" => (icons::TYPE, TYPE_TEXT),
        "SkiaButton" => (icons::MOUSE_POINTER_CLICK, TYPE_INPUT),
        "SkiaSlider" => (icons::SLIDERS_HORIZONTAL, TYPE_INPUT),
        "SkiaSwitch" | "SkiaToggle" | "SkiaCheckbox" => (icons::TOGGLE_LEFT, TYPE_INPUT),
        "SkiaEditor" | "SkiaMauiEntry" | "SkiaMauiEditor" => (icons::TEXT_CURSOR_INPUT, TYPE_INPUT),
        "SkiaImage" | "SkiaLottie" | "SkiaGif" => (icons::IMAGE, TYPE_MEDIA),
        "SkiaSvg" => (icons::SHAPES, TYPE_MEDIA),
        "SkiaShape" | "SkiaFrame" => (icons::SQUARE, TYPE_MEDIA),
        _ => (icons::BOX, MUTED),
    }
}

/// A panel: a header with an uppercase title, then the body filling the rest.
fn panel(title: &str, extra: impl IntoChildren, body: impl IntoChildren) -> Build<SkiaLayout> {
    panel_on(PANEL, title, extra, body)
}

/// A panel with its own background role (Layers).
fn panel_on(bg: Tone, title: &str, extra: impl IntoChildren, body: impl IntoChildren) -> Build<SkiaLayout> {
    SkiaGrid::new().fill().row_definitions("34, *").background_color(c(bg)).children((
        // The title, then what the panel has on the right: in a narrow panel that is cut on its left,
        // never drawn over the title.
        SkiaGrid::new().fill().background_color(c(HEADER)).padding((12, 0)).row(0).column_definitions("Auto, *").column_spacing(8).children((
            bold(11.0, MUTED).text(title.to_uppercase()).vertical_options(LayoutOptions::Center).accessibility_role(Aria::HEADING).accessibility_label(title).column(0),
            SkiaLayer::new().fill().column(1).is_clipped_to_bounds(true).children(
                SkiaRow::new().spacing(8).horizontal_options(LayoutOptions::End).vertical_options(LayoutOptions::Center).children(extra),
            ),
        )),
        SkiaLayer::new().fill().row(1).children(body),
    ))
}

/// A centered message for a panel with nothing to show.
fn empty_state(svg: &'static str, title: &str, hint: &str) -> Build<SkiaLayout> {
    SkiaStack::new().spacing(8).padding(24).center().input_transparent(true).children((
        icon(svg, 28.0, FAINT).horizontal_options(LayoutOptions::Center),
        bold(13.0, MUTED).text(title).horizontal_options(LayoutOptions::Center),
        text(12.0, FAINT).text(hint).max_lines(3).horizontal_text_alignment(TextAlignment::Center).horizontal_options(LayoutOptions::Center),
    ))
}

/// The window: the inbox (it outlives a rebuild for a new theme) and the shell.
pub fn build(app: &mut App) -> Build<SkiaLayout> {
    SkiaLayer::new().fill().background_color(c(BG)).assign(&mut app.root).consume_gestures(|_me, app: &mut App, _cx, g| app.note_pointer(g)).children((
        Build::new(Inbox).assign(&mut app.inbox),
        SkiaLayer::new().fill().assign(&mut app.content_host).children(shell(app)),
        drag_ghost(),
    ))
}

/// What a drag over Layers carries: its type's icon and color, and its name (a moved control's
/// text after it, as its row shows it).
fn dragged_look(app: &App) -> Option<(&'static str, Tone, String, String)> {
    match app.drag?.what {
        super::DragWhat::Control(k) => {
            let k = app.kinds().get(k)?;
            let (svg, tone) = type_icon(&k.type_name, k.own);
            Some((svg, tone, short_type(&k.type_name).to_owned(), String::new()))
        }
        super::DragWhat::Row(id) => {
            let n = app.tree.nodes.iter().find(|n| n.id == id)?;
            let (svg, tone) = type_icon(&n.type_name, n.assembly.as_deref().is_some_and(|a| !a.starts_with("DrawnUi")));
            let text = n.text.as_deref().filter(|t| !t.is_empty()).map(|t| format!("\u{201C}{t}\u{201D}")).unwrap_or_default();
            Some((svg, tone, short_type(&n.type_name).to_owned(), text))
        }
    }
}

/// The control being dragged, lifted by the pointer: a row of Layers raised over the window with
/// a shadow, its icon, name and text.
fn drag_ghost() -> Build<SkiaShape> {
    SkiaShape::new()
        .corner_radius(6)
        .stroke_width(1)
        .stroke_color(c(ACCENT))
        .background_color(c(HEADER))
        .height_request(ROW)
        .padding((8, 0, 12, 0))
        .shadows(SkiaShadow::new(Color::BLACK).x(0).y(5).blur(12).opacity(0.45))
        .horizontal_options(LayoutOptions::Start)
        .vertical_options(LayoutOptions::Start)
        .input_transparent(true)
        .accessibility_role(Aria::PRESENTATION)
        .is_visible(false)
        .observe(|me, app: &App| {
            let at = app.drag.filter(|d| d.active).map(|d| d.at);
            me.set_is_visible(at.is_some());
            if let Some(at) = at {
                // Beside the pointer, a little above where it would lie: lifted.
                me.set_margin(Thickness::new(at.x + 12.0, at.y - ROW / 2.0 - 3.0, 0.0, 0.0));
            }
        })
        .children(SkiaRow::new().spacing(6).vertical_options(LayoutOptions::Center).children((
            icon(icons::BOX, 14.0, MUTED).observe(|me, app: &App| {
                if let Some((svg, tone, _, _)) = dragged_look(app) {
                    me.set_svg_string(svg);
                    me.set_tint_color(c(tone));
                }
            }),
            bold(12.0, TEXT).observe(|me, app: &App| {
                if let Some((_, _, name, _)) = dragged_look(app) {
                    me.set_text(name);
                }
            }),
            text(12.0, MUTED).observe(|me, app: &App| {
                let text = dragged_look(app).map(|l| l.3).unwrap_or_default();
                me.set_is_visible(!text.is_empty());
                me.set_text(text);
            }),
        )))
}

/// The screens: the designer at the root, Settings and Help pushed over it (they slide in from the right).
/// The pages draw their own headers, so the shell's nav bar is off.
pub fn shell(app: &mut App) -> Build<SkiaShell> {
    SkiaShell::new()
        .assign(&mut app.shell)
        .nav_bar_height(0.0)
        .page_background_color(c(BG))
        .route("settings", |app: &mut App, _args| settings_page(app))
        .route("help", |app: &mut App, _args| help_page(app))
        .on_route_changed(|_me, app: &mut App, cx, route| {
            app.on_settings = route.starts_with("settings");
            app.on_help = route.starts_with("help");
            // Built again for a new theme while Settings showed: back to it once the shell is up.
            if route.is_empty() && std::mem::take(&mut app.reopen_settings) {
                cx.go_to(app.shell, "settings", false);
            }
        })
        .root(designer(app))
}

/// The designer screen: title bar, the panels, the status bar. Embedded in a page (DrawFiddle) no
/// title bar, Screens or status line: the page is the app's window and has its own status.
fn designer(app: &mut App) -> Build<SkiaLayout> {
    let full = !app.embedded;
    SkiaGrid::new().fill().row_definitions(if full { "40, *, 26" } else { "0, *, 0" }).row_spacing(0).background_color(c(BG)).children((
        title_bar(app).row(0).is_visible(full),
        // Panels with draggable dividers: Screens over Controls | Layers | Properties.
        SkiaGrid::new().fill().row(1).column_definitions("Auto, Auto, *, Auto, Auto").background_color(c(PANEL)).children((
            left_column(app).column(0).observe(|me, app: &App| me.set_width_request(app.screens_width())),
            divider(Side::Screens).column(1),
            layers_panel(app).column(2),
            divider(Side::Properties).column(3),
            properties_panel(app).column(4).observe(|me, app: &App| me.set_width_request(app.properties_width())),
        )),
        status_bar().row(2).is_visible(full),
    ))
}

/// The menu bar at the start, then the shown app as a pill (the window's own title names the
/// designer), Settings and Help at the far end.
fn title_bar(app: &mut App) -> Build<SkiaLayout> {
    // Settings and Help keep their place at the right; the menu bar, the apps and Pick are cut on
    // their right where the window is narrow, never drawn under them.
    SkiaGrid::new().fill().background_color(c(HEADER)).padding((6, 0, 14, 0)).column_definitions("*, Auto").column_spacing(10).children((
        SkiaLayer::new().fill().column(0).is_clipped_to_bounds(true).children(SkiaRow::new().spacing(10).vertical_options(LayoutOptions::Center).children((
            menubar::menu_bar(app),
            SkiaShape::new().width_request(1).height_request(18).vertical_options(LayoutOptions::Center).background_color(c(LINE)).accessibility_role(Aria::SEPARATOR),
            SkiaRow::new()
                .spacing(6)
                .vertical_options(LayoutOptions::Center)
                .accessibility_role(Aria::TOOLBAR)
                .accessibility_label(tr("apps"))
                .assign(&mut app.apps_list)
                .items(
                |app: &App| app.pill_count(),
                app_pill,
                bind_pill,
            ),
            top_pick_button(),
        ))),
        SkiaRow::new().spacing(10).column(1).vertical_options(LayoutOptions::Center).children((
            update_button(),
            changes_button(),
            // Help last, at the far end, as in most desktop tools.
            icon_button(icons::SETTINGS, &tr("settings"), |app, cx| app.open_settings(cx)).assign(&mut app.settings_button),
            icon_button(icons::CIRCLE_QUESTION_MARK, &tr("help"), |app, cx| app.open_help(cx)).assign(&mut app.help_button),
        )),
    ))
}

/// A square icon button, named for screen readers by what it does (`label`).
fn icon_button(svg: &'static str, label: &str, tap: impl Fn(&mut App, &mut Cx) + 'static) -> Build<SkiaShape> {
    let mut me = Handle::<SkiaShape>::default();
    interactive(
        SkiaShape::new()
            .corner_radius(6)
            .width_request(28)
            .height_request(28)
            .vertical_options(LayoutOptions::Center)
            .accessibility_role(Aria::BUTTON)
            .accessibility_label(label)
            .assign(&mut me)
            .on_tapped(move |_me, app: &mut App, cx| {
                if left_click(cx) {
                    tap(app, cx)
                }
            }),
        |_, _| Color::TRANSPARENT,
    )
    .children(button_icon(svg, 16.0, me, |_| false).center())
}

/// One segment of a segmented switch (the Properties order, the theme).
/// A radio button for screen readers (checked when on); put the segments in `segments`.
pub(super) fn segment(caption: &str, on: impl Fn(&App) -> bool + Clone + 'static, tap: impl Fn(&mut App, &mut Cx) + 'static) -> Build<SkiaShape> {
    let is_on = on.clone();
    interactive(
        SkiaShape::new()
            .corner_radius(5)
            .stroke_width(1)
            .padding((10, 4))
            .vertical_options(LayoutOptions::Center)
            .accessibility_role(Aria::RADIO)
            .accessibility_label(caption)
            .observe(move |me, app: &App| {
                let on = is_on(app);
                me.set_stroke_color(c(if on { ACCENT } else { LINE }));
                me.set_accessibility_is_pressed(Some(on));
            })
            .on_tapped(move |_me, app: &mut App, cx| {
                if left_click(cx) {
                    tap(app, cx)
                }
            }),
        move |app, _| c(if on(app) { SELECTION } else { PANEL }),
    )
    .children(bold(12.0, TEXT).text(caption))
}

/// Segments as one group: one Tab stop, the arrow keys move inside, named `label`.
pub(super) fn segments(label: &str, items: impl IntoChildren) -> Build<SkiaLayout> {
    SkiaRow::new().spacing(4).vertical_options(LayoutOptions::Center).accessibility_role(Aria::RADIO_GROUP).accessibility_label(label).children(items)
}

// ---------------------------------------------------------------- Theme change

/// How long the old theme takes to open away, milliseconds.
pub const THEME_WIPE_MS: f32 = 500.0;
/// The reveal's soft edge, points; the bend of the old window just ahead of it, points; the light
/// on the edge, 0..1.
const THEME_WIPE_FEATHER: f32 = 24.0;
const THEME_WIPE_BEND: f32 = 20.0;
const THEME_WIPE_LIGHT: f32 = 0.0;

/// The reveal: the old window breaks up into grains along a soft wavy front from the click (owned by
/// the `shaders` agent; uniforms `old`, `center`, `progress`, `reach`, `scale`).
const THEME_WIPE_SKSL: &str = include_str!("../../assets/shaders/theme-wipe.sksl");

thread_local! {
    /// Compiled once; None when it does not compile (the wipe then opens as a plain circle).
    static THEME_WIPE_EFFECT: Option<drawnui::skia::RuntimeEffect> = drawnui::skia::RuntimeEffect::make_for_shader(THEME_WIPE_SKSL, None)
        .map_err(|e| eprintln!("designer: theme-wipe.sksl: {e}"))
        .ok();
}

/// Whether the reveal shader compiled (tests).
#[cfg(test)]
pub fn wipe_shader_compiles() -> bool {
    THEME_WIPE_EFFECT.with(|e| e.is_some())
}

/// The reveal shader over `image` (drawn at `bounds`), or None without a compiled shader.
fn wipe_shader(image: &drawnui::skia::Image, bounds: drawnui::skia::Rect, center: drawnui::skia::Point, progress: f32, reach: f32, scale: f32) -> Option<drawnui::skia::Shader> {
    use drawnui::skia::{Data, Matrix, Rect, SamplingOptions, TileMode, runtime_effect::ChildPtr};
    THEME_WIPE_EFFECT.with(|effect| {
        let effect = effect.as_ref()?;
        let mut uniforms = vec![0u8; effect.uniform_size()];
        for u in effect.uniforms() {
            let values: &[f32] = match u.name() {
                "center" => &[center.x, center.y],
                "progress" => &[progress],
                "reach" => &[reach],
                "scale" => &[scale],
                "feather" => &[THEME_WIPE_FEATHER],
                "bend" => &[THEME_WIPE_BEND],
                "light" => &[THEME_WIPE_LIGHT],
                _ => continue,
            };
            for (i, v) in values.iter().enumerate() {
                uniforms[u.offset() + i * 4..u.offset() + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        let to_bounds = Matrix::rect_2_rect(Rect::from_wh(image.width() as f32, image.height() as f32), bounds, None)?;
        let old = image.to_shader(Some((TileMode::Clamp, TileMode::Clamp)), SamplingOptions::default(), &to_bounds)?;
        effect.make_shader(Data::new_copy(&uniforms), &[ChildPtr::Shader(old)], None)
    })
}

/// The theme change over the whole window: first it captures the window as it is drawn (the old
/// theme), then it draws that picture through the reveal shader, opening from where the new theme
/// was picked until the new window shows everywhere. Takes no input: the window under it does.
pub struct ThemeWipe {
    from: Option<drawnui::skia::Point>,
    picture: std::cell::RefCell<Option<(drawnui::skia::Image, drawnui::skia::Rect)>>,
    progress: f32,
}

impl ThemeWipe {
    /// The old window is captured: the new one can be built under it.
    pub fn captured(&self) -> bool {
        self.picture.borrow().is_some()
    }

    pub fn set_progress(&mut self, progress: f32) {
        self.progress = progress;
    }
}

impl Control for ThemeWipe {
    fn paint(&self, cx: &mut PaintCx) {
        use drawnui::skia::{ClipOp, Paint, Path};
        let rect = cx.rect;
        if self.picture.borrow().is_none() {
            // The canvas shows the old window right now: keep it (nothing to draw over it).
            if let Some(t) = drawnui::effects::snapshot(cx, rect) {
                *self.picture.borrow_mut() = Some((t.image, t.bounds));
            }
            return;
        }
        let picture = self.picture.borrow();
        let Some((image, bounds)) = picture.as_ref() else { return };
        let center = self.from.unwrap_or_else(|| rect.center());
        // The circle reaches the farthest corner at the end.
        let reach = [(rect.left, rect.top), (rect.right, rect.top), (rect.left, rect.bottom), (rect.right, rect.bottom)]
            .iter()
            .map(|(x, y)| ((x - center.x).powi(2) + (y - center.y).powi(2)).sqrt())
            .fold(0.0, f32::max);
        let canvas = cx.canvas;
        if let Some(shader) = wipe_shader(image, *bounds, center, self.progress, reach, cx.scale) {
            let mut paint = Paint::default();
            paint.set_shader(shader);
            canvas.draw_rect(rect, &paint);
            return;
        }
        canvas.save();
        canvas.clip_path(&Path::circle(center, reach * self.progress, None), ClipOp::Difference, true);
        canvas.draw_image_rect(image, None, *bounds, &Paint::default());
        canvas.restore();
    }
}

/// The wipe control, over everything; `from` is the click in canvas pixels.
pub fn theme_wipe(from: Option<drawnui::skia::Point>) -> Build<ThemeWipe> {
    // Input goes through to the window under it: where the new theme is uncovered, hover and clicks
    // work while the picture still opens.
    Build::new(ThemeWipe { from, picture: Default::default(), progress: 0.0 }).fill().input_transparent(true)
}

// ---------------------------------------------------------------- Settings

const DRAWNUI_MARK: &str = include_str!("../../assets/brand/drawnui-mark.svg");
const NX_LOGO: &str = include_str!("../../assets/brand/nx-logo.svg");
const NX_LOGO_ON_DARK: &str = include_str!("../../assets/brand/nx-logo-on-dark.svg");

impl Section {
    fn title(self) -> String {
        tr(match self {
            Section::Appearance => "section-appearance",
            Section::Layers => "section-layers",
            Section::Code => "section-code",
            Section::Connection => "section-connection",
            Section::About => "section-about",
        })
    }

    fn icon(self) -> &'static str {
        match self {
            Section::Appearance => icons::PALETTE,
            Section::Layers => icons::LAYERS,
            Section::Code => icons::CODE,
            Section::Connection => icons::PLUG,
            Section::About => icons::INFO,
        }
    }
}

/// Settings: the sections on the left, the selected one's options on the right.
fn settings_page(app: &mut App) -> Build<SkiaLayout> {
    let show = |s: Section| move |me: &mut Mut<'_, SkiaLayout>, app: &App| me.set_is_visible(app.section == s);
    SkiaGrid::new().fill().row_definitions("40, *").row_spacing(0).background_color(c(BG)).children((
        page_header(&tr("settings"), &mut app.back_button).row(0),
        SkiaGrid::new().fill().row(1).column_definitions("208, *").column_spacing(0).children((
            SkiaLayer::new().fill().column(0).background_color(c(PANEL)).children((
                SkiaStack::new()
                    .spacing(2)
                    .padding((8, 12))
                    .accessibility_role(Aria::RADIO_GROUP)
                    .accessibility_label(tr("settings-sections"))
                    .children(SECTIONS.iter().map(|s| nav_item(*s)).collect::<Vec<_>>()),
                SkiaLayer::new().width_request(1).fill_y().horizontal_options(LayoutOptions::End).background_color(c(LINE)),
            )),
            panel_scroll().fill().column(1).assign(&mut app.settings_scroll).content(SkiaLayer::new().fill_x().padding((40, 28, 40, 40)).children((
                appearance_section(app).observe(show(Section::Appearance)),
                layers_section().observe(show(Section::Layers)),
                code_section().observe(show(Section::Code)),
                connection_section(app).observe(show(Section::Connection)),
                about_section().observe(show(Section::About)),
            ))),
        )),
    ))
}

/// A pushed page's header: one button, the arrow and the page's name, goes back to the designer.
fn page_header(title: &str, back: &mut Handle<SkiaShape>) -> Build<SkiaLayout> {
    SkiaLayout::new().fill().background_color(c(HEADER)).padding((8, 0)).children((
        interactive(
            SkiaShape::new()
                .corner_radius(6)
                .padding((6, 0, 10, 0))
                .height_request(28)
                .vertical_options(LayoutOptions::Center)
                .accessibility_role(Aria::BUTTON)
                .accessibility_label(tr_args("back-to-designer", &[("page", title.to_owned().into())]))
                .on_tapped(|_me, app: &mut App, cx| {
                    if left_click(cx) {
                        app.close_page(cx)
                    }
                }),
            |_, _| Color::TRANSPARENT,
        )
        .assign(back)
        .children(SkiaRow::new().spacing(8).vertical_options(LayoutOptions::Center).children((
            icon(icons::ARROW_LEFT, 16.0, MUTED),
            bold(13.0, TEXT).text(title).vertical_options(LayoutOptions::Center).accessibility_role(Aria::PRESENTATION),
        ))),
        SkiaLayer::new().height_request(1).fill_x().vertical_options(LayoutOptions::End).background_color(c(LINE)),
    ))
}

// ---------------------------------------------------------------- Help

/// Help: what the designer does and how, in the order it is used.
fn help_page(app: &mut App) -> Build<SkiaLayout> {
    let port = app.hub_port();
    SkiaGrid::new().fill().row_definitions("40, *").row_spacing(0).background_color(c(BG)).children((
        page_header(&tr("help"), &mut app.help_back_button).row(0),
        panel_scroll().fill().row(1).content(SkiaStack::new().spacing(0).fill_x().maximum_width_request(760.0).padding((40, 28, 40, 40)).children((
            bold(20.0, TEXT).text(tr("help-title")).accessibility_role(Aria::HEADING),
            text(13.0, MUTED).text(tr("help-intro")).max_lines(3).fill_x().margin((0, 6, 0, 18)),
            // The topics in the order the designer is used.
            SkiaStack::new().spacing(0).fill_x().children(vec![
                help_topic(icons::PLUG, "help-connect", 3, None),
                help_topic(icons::SMARTPHONE, "help-phones", 3, None),
                help_topic(icons::LAYOUT_PANEL_TOP, "help-screens", 1, None),
                help_topic(icons::LAYERS, "help-layers", 3, None),
                help_topic(icons::SLIDERS_HORIZONTAL, "help-properties", 3, None),
                help_topic(icons::CODE, "help-write", 4, None),
                help_topic(icons::SQUARE_PLUS, "help-controls", 3, None),
                help_topic(icons::ARROW_UP_DOWN, "help-move", 3, None),
                help_topic(icons::COPY, "help-copy", 3, None),
                help_topic(icons::KEYBOARD, "help-keyboard", 3, None),
                help_topic(icons::BOT, "help-agents", 2, Some(format!("http://127.0.0.1:{}/mcp", port + 1))),
            ]),
        ))),
    ))
}

/// One Help topic: an icon, its title (`key`) and its lines (`key-1`.. `key-N`); `address` fills a line's
/// `{ $address }`.
fn help_topic(svg: &'static str, key: &str, lines: usize, address: Option<String>) -> Build<SkiaLayout> {
    let lines: Vec<Build<SkiaLabel>> = (1..=lines)
        .map(|i| {
            let line = match &address {
                Some(a) => tr_args(&format!("{key}-{i}"), &[("address", a.clone().into())]),
                None => tr(&format!("{key}-{i}")),
            };
            text(13.0, MUTED).text(line).max_lines(4).fill_x()
        })
        .collect();
    SkiaStack::new().spacing(0).fill_x().children((
        SkiaLayer::new().height_request(1).fill_x().background_color(c(LINE)),
        SkiaRow::new().spacing(14).fill_x().padding((0, 16)).children((
            SkiaShape::new()
                .corner_radius(8)
                .background_color(c(PANEL))
                .width_request(34)
                .height_request(34)
                .children(icon(svg, 17.0, ACCENT).center()),
            SkiaStack::new().spacing(6).fill_x().vertical_options(LayoutOptions::Center).children((
                bold(14.0, TEXT).text(tr(key)).accessibility_role(Aria::HEADING),
                SkiaStack::new().spacing(4).fill_x().children(lines),
            )),
        )),
    ))
}

fn nav_item(s: Section) -> Build<SkiaShape> {
    interactive(
        SkiaShape::new()
            .corner_radius(6)
            .fill_x()
            .height_request(32)
            .padding((10, 0))
            .accessibility_role(Aria::RADIO)
            .accessibility_label(s.title())
            .observe(move |me, app: &App| me.set_accessibility_is_pressed(Some(app.section == s)))
            .on_tapped(move |_me, app: &mut App, cx| {
                if left_click(cx) {
                    app.show_section(s);
                }
            }),
        move |app, _| if app.section == s { c(SELECTION) } else { Color::TRANSPARENT },
    )
        .children(SkiaRow::new().spacing(10).vertical_options(LayoutOptions::Center).children((
            icon(s.icon(), 15.0, MUTED).observe(move |me, app: &App| me.set_tint_color(c(if app.section == s { ACCENT } else { MUTED }))),
            text(13.0, TEXT).text(s.title()),
        )))
}

/// A section: its title, a line saying what it is about, then its options.
fn section(title: &str, hint: &str, rows: impl IntoChildren) -> Build<SkiaLayout> {
    SkiaStack::new().spacing(0).fill_x().maximum_width_request(680.0).children((
        bold(20.0, TEXT).text(title).accessibility_role(Aria::HEADING),
        text(13.0, MUTED).text(hint).max_lines(3).fill_x().margin((0, 6, 0, 18)),
        SkiaStack::new().spacing(0).fill_x().children(rows),
    ))
}

/// One option: its name and what it does on the left, the control on the right; a line above.
fn option_row(title: &str, hint: &str, control: impl IntoChildren) -> Build<SkiaLayout> {
    SkiaStack::new().spacing(0).fill_x().children((
        SkiaLayer::new().height_request(1).fill_x().background_color(c(LINE)),
        SkiaLayer::new().fill_x().padding((0, 14)).children((
            SkiaStack::new().spacing(3).fill_x().margin((0, 0, 260, 0)).vertical_options(LayoutOptions::Center).children((
                bold(13.0, TEXT).text(title),
                text(12.0, MUTED).text(hint).max_lines(4).fill_x(),
            )),
            SkiaRow::new().spacing(4).horizontal_options(LayoutOptions::End).vertical_options(LayoutOptions::Center).children(control),
        )),
    ))
}

fn appearance_section(app: &mut App) -> Build<SkiaLayout> {
    let mut light = Handle::<SkiaLayout>::default();
    let cards: Vec<Build<SkiaLayout>> = Theme::all().map(|t| if t.id() == "light" { theme_card(t).assign(&mut light) } else { theme_card(t) }).collect();
    app.light_segment = light;
    section(
        &tr("section-appearance"),
        &tr("appearance-hint"),
        // Language first (the words of everything below), then the theme.
        SkiaStack::new().spacing(0).fill_x().children((
            option_row(&tr("language"), &tr("language-hint"), language_button(app)),
            SkiaLayer::new().height_request(1).fill_x().background_color(c(LINE)),
            SkiaStack::new().spacing(3).fill_x().padding((0, 14, 0, 12)).children((
                bold(13.0, TEXT).text(tr("theme")),
                text(12.0, MUTED).text(tr("theme-hint")).max_lines(2).fill_x(),
            )),
            // Three cards a row: the dark themes, then the light ones and high contrast.
            SkiaWrap::new().spacing(12).fill_x().maximum_width_request(3.0 * 144.0 + 2.0 * 12.0).horizontal_options(LayoutOptions::Start).accessibility_role(Aria::RADIO_GROUP).accessibility_label(tr("theme")).children(cards),
            SkiaLayer::new().height_request(1).fill_x().background_color(c(LINE)).margin((0, 16, 0, 0)),
            option_row(&tr("animate-changes"), &tr("animate-changes-hint"), setting_switch(tr("animate-changes"), |app| !app.settings.instant_changes, |app, on| {
                app.settings.instant_changes = !on;
                app.save_settings();
            })),
            SkiaLayer::new().height_request(1).fill_x().background_color(c(LINE)),
            option_row(&tr("selection-color"), &tr("selection-color-hint"), selection_swatches()),
        )),
    )
}

/// The selection box's color in the app before one is picked (the agents' own).
pub const DEFAULT_SELECTION: &str = "#FFFF00FF";

/// The Selection color's choices: magenta (the agents' own), lime, cyan, orange, white, then the picker.
fn selection_swatches() -> Vec<Build<SkiaShape>> {
    let choices: [(Option<&str>, &str, &str); 5] = [
        (None, DEFAULT_SELECTION, "color-magenta"),
        (Some("#FFA3E635"), "#FFA3E635", "color-lime"),
        (Some("#FF22D3EE"), "#FF22D3EE", "color-cyan"),
        (Some("#FFFB923C"), "#FFFB923C", "color-orange"),
        (Some("#FFFFFFFF"), "#FFFFFFFF", "color-white"),
    ];
    let mut swatches: Vec<Build<SkiaShape>> = choices
        .into_iter()
        .map(|(value, shown, name)| {
            let value = value.map(str::to_owned);
            let on = value.clone();
            let fill = parse_argb(shown).unwrap_or(Color::WHITE);
            SkiaShape::new()
                .shape_type(ShapeType::Circle)
                .width_request(22)
                .height_request(22)
                .vertical_options(LayoutOptions::Center)
                .background_color(fill)
                .accessibility_role(Aria::RADIO)
                .accessibility_label(tr(name))
                .on_hovered(|me, _app: &mut App, _cx, over| me.set_scale(if over { 1.15 } else { 1.0 }))
                .observe(move |me, app: &App| {
                    let picked = app.settings.selection_color == on;
                    me.set_stroke_width(if picked { 2.0 } else { 1.0 });
                    me.set_stroke_color(c(if picked { ACCENT } else { LINE }));
                    me.set_accessibility_is_pressed(Some(picked));
                })
                .on_tapped(move |_me, app: &mut App, cx| {
                    if left_click(cx) {
                        app.set_selection_color(value.clone(), cx)
                    }
                })
        })
        .collect();
    // Any other color: the picker, as for a property's color.
    swatches.push(
        interactive(SkiaShape::new().corner_radius(6).width_request(26).height_request(26).vertical_options(LayoutOptions::Center), |_, _| Color::TRANSPARENT)
            .accessibility_role(Aria::BUTTON)
            .accessibility_label(tr("selection-color-other"))
            .on_tapped(|me, app: &mut App, cx| {
                if left_click(cx) {
                    let anchor = points_rect(me.base());
                    app.open_selection_picker(anchor, cx);
                }
            })
            .children(icon(icons::PALETTE, 16.0, MUTED).center()),
    );
    swatches
}

/// A theme to pick: a small picture of the window in its colors, and its name.
fn theme_card(theme: Theme) -> Build<SkiaLayout> {
    let p = move |t: Tone| theme.color(t);
    let bar = move |t: Tone, w: f32, h: f32, left: f32, top: f32| SkiaShape::new().corner_radius(h / 2.0).width_request(w).height_request(h).margin((left, top, 0.0, 0.0)).background_color(p(t));
    // The card's own stroke never changes width (a changing stroke resizes an auto-sized shape and
    // moves its content at 125 %): its 1 pt frame only changes color, and the selected look's second
    // point is a ring laid over the card, outside the layout.
    let card = interactive(
        SkiaShape::new()
            .corner_radius(10)
            .padding(6)
            // The picture's width and the padding: a card is as wide as its picture.
            .width_request(144.0)
            .stroke_width(1)
            .accessibility_role(Aria::RADIO)
            .accessibility_label(theme_name(theme))
            .observe(move |me, app: &App| {
                let on = app.settings.theme == theme;
                me.set_stroke_color(c(if on { ACCENT } else { LINE }));
                me.set_accessibility_is_pressed(Some(on));
            })
            .on_tapped(move |_me, app: &mut App, cx| {
                if left_click(cx) {
                    app.set_theme(theme, cx)
                }
            }),
        |_, _| Color::TRANSPARENT,
    )
    .children(SkiaStack::new().spacing(6).children((
        // The window in miniature: title bar, a side panel with a selected row, text, an accent button.
        SkiaShape::new().corner_radius(6).width_request(132).height_request(78).background_color(p(BG)).stroke_width(1).stroke_color(p(LINE)).children(SkiaLayer::new().fill().children((
            SkiaLayer::new().fill_x().height_request(12).background_color(p(HEADER)),
            SkiaLayer::new().width_request(36).fill_y().margin((0, 12, 0, 0)).background_color(p(PANEL)),
            bar(SELECTION, 28.0, 6.0, 4.0, 18.0),
            bar(MUTED, 22.0, 3.0, 6.0, 30.0),
            bar(MUTED, 18.0, 3.0, 6.0, 38.0),
            bar(TEXT, 54.0, 4.0, 46.0, 20.0),
            bar(MUTED, 70.0, 3.0, 46.0, 30.0),
            bar(MUTED, 48.0, 3.0, 46.0, 38.0),
            bar(ACCENT, 34.0, 10.0, 46.0, 54.0),
        ))),
        text(12.0, TEXT).text(theme_name(theme)).horizontal_options(LayoutOptions::Center),
    )));
    SkiaLayer::new().horizontal_options(LayoutOptions::Start).children((
        card,
        SkiaShape::new()
            .fill()
            .corner_radius(10)
            .stroke_width(2)
            .stroke_color(c(ACCENT))
            .input_transparent(true)
            .observe(move |me, app: &App| me.set_is_visible(app.settings.theme == theme)),
    ))
}

/// A settings row's switch: `get` reads the setting, `set` changes it.
fn setting_switch(label: String, get: fn(&App) -> bool, set: fn(&mut App, bool)) -> Build<SkiaSwitch> {
    SkiaSwitch::new()
        .accessibility_label(label)
        // The style's width, a lower track: it sits on a settings row, not on a phone.
        .width_request(46.0)
        .height_request(20.0)
        .color_frame_on(Some(c(ACCENT)))
        .color_frame_off(Some(c(SWITCH_OFF)))
        .color_thumb_on(Some(c(ON_ACCENT)))
        .color_thumb_off(Some(c(SWITCH_THUMB_OFF)))
        .observe(move |me, app: &App| me.set_is_toggled(get(app)))
        .on_toggled(move |_me, app: &mut App, _cx, on| set(app, on))
}

fn layers_section() -> Build<SkiaLayout> {
    section(
        &tr("section-layers"),
        &tr("layers-hint"),
        option_row(
            &tr("library-parts"),
            &tr("library-parts-hint"),
            SkiaSwitch::new()
                .accessibility_label(tr("library-parts"))
                // The style's width, a lower track: it sits on a settings row, not on a phone.
                .width_request(46.0)
                .height_request(20.0)
                .color_frame_on(Some(c(ACCENT)))
                .color_frame_off(Some(c(SWITCH_OFF)))
                .color_thumb_on(Some(c(ON_ACCENT)))
                .color_thumb_off(Some(c(SWITCH_THUMB_OFF)))
                .observe(|me, app: &App| me.set_is_toggled(app.settings.show_internal))
                .on_toggled(|_me, app: &mut App, cx, on| app.set_show_internal(on, cx)),
        ),
    )
}

/// Code: when the changes made here are written to the app's code (Manual, Auto, Always).
fn code_section() -> Build<SkiaLayout> {
    section(
        &tr("section-code"),
        &tr("code-hint"),
        option_row(
            &tr("write-mode"),
            &tr("write-mode-hint"),
            segments(
                &tr("write-mode"),
                super::WriteMode::ALL.iter().map(|&mode| segment(&mode.title(), move |app| app.settings.write_mode == mode, move |app, _cx| app.set_write_mode(mode))).collect::<Vec<_>>(),
            ),
        ),
    )
}

fn connection_section(app: &mut App) -> Build<SkiaLayout> {
    let port = app.hub_port();
    let hub = format!("ws://127.0.0.1:{port}");
    let mcp = format!("http://127.0.0.1:{}/mcp", port + 1);
    let register = format!("claude mcp add --transport http drawnui-designer {mcp}");
    section(
        &tr("section-connection"),
        &tr("connection-hint"),
        (
            option_row(&tr("connection-apps"), &hub, copy_button(0, hub.clone())),
            option_row(&tr("connection-mcp"), &mcp, copy_button(1, mcp.clone())),
            option_row(&tr("connection-claude"), &register, copy_button(2, register.clone())),
            option_row(
                &tr("connection-devices"),
                &tr("connection-devices-hint"),
                SkiaSwitch::new()
                    .accessibility_label(tr("connection-devices"))
                    .width_request(46.0)
                    .height_request(20.0)
                    .color_frame_on(Some(c(ACCENT)))
                    .color_frame_off(Some(c(SWITCH_OFF)))
                    .color_thumb_on(Some(c(ON_ACCENT)))
                    .color_thumb_off(Some(c(SWITCH_THUMB_OFF)))
                    .observe(|me, app: &App| me.set_is_toggled(app.settings.allow_devices))
                    .on_toggled(|_me, app: &mut App, _cx, on| app.set_allow_devices(on)),
            ),
            device_address_row(),
        ),
    )
}

/// The address apps on other devices connect to, shown while the hub lets them.
fn device_address_row() -> Build<SkiaLayout> {
    SkiaStack::new()
        .spacing(0)
        .fill_x()
        .children((
            SkiaLayer::new().height_request(1).fill_x().background_color(c(LINE)),
            SkiaLayer::new().fill_x().padding((0, 14)).children((
                SkiaStack::new().spacing(3).fill_x().margin((0, 0, 260, 0)).vertical_options(LayoutOptions::Center).children((
                    bold(13.0, TEXT).text(tr("connection-devices-address")),
                    text(12.0, MUTED).max_lines(4).fill_x().observe(|me, app: &App| me.set_text(app.device_url.clone().unwrap_or_default())),
                )),
                SkiaRow::new().spacing(4).horizontal_options(LayoutOptions::End).vertical_options(LayoutOptions::Center).children((
                    text(12.0, OK).text(tr("copied")).observe(|me, app: &App| me.set_is_visible(app.copied == Some(3))),
                    icon_button(icons::COPY, &tr("copy"), |app, cx| {
                        if let Some(url) = app.device_url.clone() {
                            app.copy(3, url, cx);
                        }
                    }),
                )),
            )),
        ))
        .observe(|me, app: &App| me.set_is_visible(app.device_url.is_some()))
}

/// Copies `value`; says "Copied" once done.
fn copy_button(row: usize, value: String) -> (Build<SkiaLabel>, Build<SkiaShape>) {
    (
        text(12.0, OK).text(tr("copied")).observe(move |me, app: &App| me.set_is_visible(app.copied == Some(row))),
        icon_button(icons::COPY, &tr("copy"), move |app, cx| app.copy(row, value.clone(), cx)),
    )
}

fn about_section() -> Build<SkiaLayout> {
    SkiaStack::new().spacing(0).fill_x().maximum_width_request(680.0).children((
        SkiaStack::new().spacing(6).fill_x().margin((0, 8, 0, 0)).children((
            SkiaSvg::from_string(DRAWNUI_MARK).width_request(96).height_request(96).horizontal_options(LayoutOptions::Center).accessibility_role(Aria::IMG).accessibility_label(tr("about-logo")),
            bold(22.0, TEXT).text(tr("app-title")).horizontal_options(LayoutOptions::Center).margin((0, 6, 0, 0)).accessibility_role(Aria::HEADING),
            text(12.0, FAINT).text(tr_args("about-version", &[("version", env!("CARGO_PKG_VERSION").into())])).horizontal_options(LayoutOptions::Center),
            text(13.0, MUTED)
                .text(tr("about-description"))
                .max_lines(3)
                .fill_x()
                .horizontal_text_alignment(TextAlignment::Center)
                .margin((40, 6, 40, 0)),
            SkiaRow::new().spacing(20).horizontal_options(LayoutOptions::Center).margin((0, 6, 0, 0)).children((
                link("drawnui.net", "https://drawnui.net"),
                link("GitHub", "https://github.com/DrawnUi/DrawnUi.Designer"),
            )),
        )),
        update_option(),
        section_label(&tr("made-with")),
        credit(
            SkiaSvg::from_string(DRAWNUI_MARK).width_request(34).height_request(34).center(),
            "DrawnUI for Rust",
            &tr("credit-drawnui-rust"),
            "github.com/DrawnUi/DrawnUi.Rust",
            "https://github.com/DrawnUi/DrawnUi.Rust",
        ),
        section_label(&tr("uses")),
        credit(
            SkiaSvg::from_string(if is_light() { NX_LOGO } else { NX_LOGO_ON_DARK }).width_request(36).height_request(18).center(),
            "NX",
            &tr("credit-nx"),
            "github.com/nx-lang/nx",
            "https://github.com/nx-lang/nx",
        ),
        credit(
            icon(icons::FILE_CODE, 20.0, MUTED).center(),
            "tree-sitter C#",
            &tr("credit-tree-sitter"),
            "github.com/tree-sitter/tree-sitter-c-sharp",
            "https://github.com/tree-sitter/tree-sitter-c-sharp",
        ),
        credit(
            icon(icons::PLUG, 20.0, MUTED).center(),
            "MCP Rust SDK",
            &tr("credit-mcp"),
            "github.com/modelcontextprotocol/rust-sdk",
            "https://github.com/modelcontextprotocol/rust-sdk",
        ),
        credit(
            icon(icons::SPARKLES, 20.0, MUTED).center(),
            "Lucide",
            &tr("credit-lucide"),
            "lucide.dev",
            "https://lucide.dev",
        ),
    ))
}

fn section_label(caption: &str) -> Build<SkiaLabel> {
    bold(11.0, MUTED).text(caption.to_uppercase()).margin((0, 32, 0, 10)).accessibility_role(Aria::HEADING).accessibility_label(caption)
}

/// A link: opens `url` in the browser.
fn link(caption: &str, url: &'static str) -> Build<SkiaLabel> {
    text(12.0, ACCENT)
        .text(caption)
        .accessibility_role(Aria::LINK)
        .accessibility_hint(url)
        .on_hovered(|_me, app: &mut App, _cx, _over| app.hover_changes = app.hover_changes.wrapping_add(1))
        .observe(|me, _app: &App| {
            let over = me.base().is_hovered();
            me.set_text_color(c(if over { FIELD } else { ACCENT }));
        })
        .on_tapped(move |_me, _app: &mut App, cx| {
            if left_click(cx) {
                cx.open_url(url);
            }
        })
}

/// A card for something the designer is made with or uses: its logo, name, what it does, a link.
fn credit(logo: impl IntoChildren, name: &str, what: &str, caption: &str, url: &'static str) -> Build<SkiaShape> {
    SkiaShape::new()
        .corner_radius(8)
        .stroke_width(1)
        .stroke_color(c(LINE))
        .background_color(c(PANEL))
        .fill_x()
        .padding(14)
        .margin((0, 0, 0, 8))
        .children(SkiaRow::new().spacing(14).fill_x().children((
            SkiaShape::new().corner_radius(8).background_color(c(HEADER)).width_request(48).height_request(48).vertical_options(LayoutOptions::Center).children(logo),
            SkiaStack::new().spacing(3).fill_x().vertical_options(LayoutOptions::Center).children((
                bold(13.0, TEXT).text(name).accessibility_role(Aria::HEADING),
                text(12.0, MUTED).text(what).max_lines(3).fill_x(),
                link(caption, url),
            )),
        )))
}

/// The title bar pill (the last app): a connected app (green dot, shown one highlighted, a restart button), or a
/// recent app that is not running (grey dot, a run button), or one being built (amber dot).
fn bind_pill(cell: &PillHandles, app: &App, _index: usize, cx: &mut Cx) {
    use designer_protocol::RunState;
    let index = app.pill_slot();
    let engine_head = |engine: &str| engine.split('-').nth(1).unwrap_or(engine).to_uppercase();
    let (name, engine, project, connected, shown, platform) = match app.apps.get(index) {
        Some(a) => (a.info.name.clone(), a.info.engine.clone(), a.info.project.clone().unwrap_or_default(), true, Some(a.id) == app.app, a.info.platform.clone()),
        None => match app.recent.get(index - app.apps.len()) {
            Some(r) => (r.name.clone(), r.engine.clone(), r.project.clone(), false, false, None),
            None => return,
        },
    };
    let (platform_name, platform_icon) = platform_chip(platform.as_deref().unwrap_or_default());
    let run = app.runs.get(&project);
    let detail = match run {
        Some((RunState::Building, _)) => tr("app-building"),
        Some((RunState::Starting, _)) => tr("app-starting"),
        Some((RunState::Failed, _)) => tr("app-failed"),
        _ => engine_head(&engine),
    };
    let dot = match run {
        Some((RunState::Failed, _)) => ERROR,
        Some(_) => WARN,
        None if connected => OK,
        None => FAINT,
    };
    set(cx, cell.name, name.clone());
    set(cx, cell.engine, detail.clone());
    set(cx, cell.platform_name, platform_name.to_owned());
    if let Some(mut me) = cx.get_mut(cell.platform_icon) {
        me.set_svg_string(platform_icon);
    }
    if let Some(mut me) = cx.get_mut(cell.platform) {
        me.set_is_visible(!platform_name.is_empty());
    }
    // Screen readers hear the platform with the engine: "Tutorials, MAUI, Android".
    let detail = if platform_name.is_empty() { detail } else { format!("{detail}, {platform_name}") };
    if let Some(mut me) = cx.get_mut(cell.dot) {
        me.set_background_color(c(dot));
    }
    if let Some(mut me) = cx.get_mut(cell.action) {
        // Restart for a connected app, run for a recent one; nothing while a run is on its way.
        me.set_svg_string(if connected { icons::ROTATE_CW } else { icons::PLAY });
        me.set_is_visible(run.is_none() || matches!(run, Some((RunState::Failed, _))));
    }
    if let Some(mut me) = cx.get_mut(cell.action_area) {
        me.set_tag(index.to_string());
        me.set_accessibility_label(tr_args(if connected { "app-restart" } else { "app-run" }, &[("name", name.clone().into())]));
    }
    if let Some(mut me) = cx.get_mut(cell.root) {
        let hovered = me.base().is_hovered();
        me.set_background_color(veiled(c(if shown { SELECTION } else { PANEL }), hovered, false));
        me.set_stroke_color(c(if shown { ACCENT } else { LINE }));
        me.set_accessibility_label(match (connected, shown) {
            (true, true) => tr_args("app-shown", &[("name", name.clone().into()), ("detail", detail.clone().into())]),
            (true, false) => tr_args("app-show", &[("name", name.clone().into()), ("detail", detail.clone().into())]),
            (false, _) => tr_args("app-run-detail", &[("name", name.clone().into()), ("detail", detail.clone().into())]),
        });
        me.set_opacity(if connected { 1.0 } else { 0.75 });
    }
}

#[derive(Default)]
pub struct PillHandles {
    root: Handle<SkiaShape>,
    dot: Handle<SkiaShape>,
    action: Handle<SkiaSvg>,
    action_area: Handle<SkiaShape>,
    name: Handle<SkiaLabel>,
    engine: Handle<SkiaLabel>,
    /// The chip with the platform the app runs on (Windows, Android, iOS, macOS).
    platform: Handle<SkiaShape>,
    platform_icon: Handle<SkiaSvg>,
    platform_name: Handle<SkiaLabel>,
}

/// The name and icon of a platform an app reports (`AppInfo::platform`); product names stay untranslated.
pub(super) fn platform_chip(platform: &str) -> (&'static str, &'static str) {
    match platform {
        "windows" => ("Windows", icons::MONITOR),
        "android" => ("Android", icons::SMARTPHONE),
        "ios" => ("iOS", icons::SMARTPHONE),
        "ipados" => ("iPadOS", icons::TABLET),
        "maccatalyst" | "macos" => ("macOS", icons::MONITOR),
        "linux" => ("Linux", icons::MONITOR),
        "web" => ("Web", icons::MONITOR),
        _ => ("", icons::MONITOR),
    }
}

fn app_pill() -> (Build<SkiaShape>, PillHandles) {
    let mut h = PillHandles::default();
    let pill = interactive(
        SkiaShape::new()
            .corner_radius(12)
            .stroke_width(1)
            .padding((10, 4))
            .accessibility_role(Aria::BUTTON)
            .assign(&mut h.root)
            .on_tapped(|me, app: &mut App, cx| {
                if !left_click(cx) {
                    return;
                }
                let anchor = points_rect(me.base());
                app.open_app_menu(anchor, cx);
            }),
        |app, index| c(if index.is_some_and(|i| app.apps.get(i).is_some_and(|a| Some(a.id) == app.app)) { SELECTION } else { PANEL }),
    )
        .children(SkiaRow::new().spacing(6).children((
            // Where the shown app runs, left of its name.
            SkiaShape::new()
                .corner_radius(7)
                .padding((6, 1, 7, 1))
                .background_color(c(HOVER))
                .vertical_options(LayoutOptions::Center)
                .is_visible(false)
                .assign(&mut h.platform)
                .children(SkiaRow::new().spacing(4).children((
                    icon(icons::MONITOR, 11.0, MUTED).assign(&mut h.platform_icon),
                    text(10.5, MUTED).assign(&mut h.platform_name),
                ))),
            SkiaShape::new()
                .shape_type(ShapeType::Circle)
                .width_request(7)
                .height_request(7)
                .background_color(c(OK))
                .vertical_options(LayoutOptions::Center)
                .assign(&mut h.dot),
            bold(12.0, TEXT).assign(&mut h.name),
            text(11.0, MUTED).assign(&mut h.engine),
            // Restart (connected) or run (recent): its own tap, so the pill's focus tap is not fired.
            interactive(
                SkiaShape::new()
                    .corner_radius(9)
                    .width_request(20)
                    .height_request(18)
                    .vertical_options(LayoutOptions::Center)
                    .accessibility_role(Aria::BUTTON),
                |_, _| Color::TRANSPARENT,
            )
                .assign(&mut h.action_area)
                .on_tapped(|me, app: &mut App, cx| {
                    if !left_click(cx) {
                        return;
                    }
                    let Ok(index) = me.base().p.tag.parse::<usize>() else { return };
                    if index < app.apps.len() {
                        app.restart_pill(index, cx);
                    } else {
                        app.tap_pill(index, cx);
                    }
                })
                .children(SkiaSvg::from_string(icons::ROTATE_CW).width_request(13).height_request(13).center().tint_color(c(MUTED)).assign(&mut h.action)),
            // Last: the list of every app (the restart before it belongs to the app shown).
            icon(icons::CHEVRON_DOWN, 12.0, MUTED),
        )));
    (pill, h)
}

fn screens_panel(app: &mut App) -> Build<SkiaLayout> {
    panel(
        &tr("panel-screens"),
        Vec::<drawnui::Detached>::new(),
        (
            panel_scroll().fill().content(SkiaStack::new().spacing(0).padding((0, 4)).accessibility_role(Aria::LIST).accessibility_label(tr("panel-screens")).assign(&mut app.screens_list).items(
                |app: &App| app.screen_rows().len(),
                || {
                    let (cell, h) = row_cell(false);
                    (
                        cell.on_tapped(|me, app: &mut App, cx| {
                            if !left_click(cx) {
                                return;
                            }
                            let Some(index) = me.base().context_index else { return };
                            match app.screen_rows().get(index) {
                                Some(super::ScreenRow::Screen(i)) => {
                                    if let Some(screen) = app.screens.get(*i) {
                                        app.send(HubToApp::OpenScreen { route: screen.route.clone() });
                                    }
                                }
                                Some(super::ScreenRow::Action(i)) => {
                                    if let Some(action) = app.action(*i) {
                                        app.send(HubToApp::RunAction { path: action.path.clone() });
                                    }
                                }
                                _ => {}
                            }
                        }),
                        h,
                    )
                },
                |cell: &RowHandles, app: &App, index: usize, cx: &mut Cx| {
                    let row = app.screen_rows().get(index).cloned();
                    // A header reads as one; screens and actions as list rows (cells are reused).
                    let header = matches!(row, Some(super::ScreenRow::Group(_)));
                    if let Some(mut me) = cx.get_mut(cell.label) {
                        me.set_font_family(if header { "Semibold" } else { "Default" });
                        me.set_font_size(if header { 12.0 } else { 13.0 });
                        me.set_text_color(c(if header { MUTED } else { TEXT }));
                    }
                    if let Some(mut me) = cx.get_mut(cell.icon) {
                        me.set_is_visible(!header);
                    }
                    set(cx, cell.detail, String::new());
                    match row {
                        Some(super::ScreenRow::Screen(i)) => {
                            let Some(s) = app.screens.get(i) else { return };
                            let title = s.title.clone().unwrap_or_else(|| s.route.clone());
                            if let Some(mut me) = cx.get_mut(cell.root) {
                                me.set_accessibility_role(Aria::LIST_ITEM);
                                me.set_accessibility_label(tr_args("screen-open", &[("title", title.clone().into())]));
                            }
                            set(cx, cell.label, title);
                            cell_icon(cx, cell, icons::LAYOUT_PANEL_TOP, MUTED, None);
                        }
                        Some(super::ScreenRow::Group(group)) => {
                            let title = if group.is_empty() { tr("actions") } else { group.replace('/', " \u{203A} ") };
                            if let Some(mut me) = cx.get_mut(cell.root) {
                                me.set_accessibility_role(Aria::HEADING);
                                me.set_accessibility_label(title.as_str());
                            }
                            set(cx, cell.label, title.to_uppercase());
                            cell_icon(cx, cell, icons::PLAY, MUTED, None);
                        }
                        Some(super::ScreenRow::Action(i)) => {
                            let Some(a) = app.action(i) else { return };
                            let name = super::action_name(&a.path).to_owned();
                            if let Some(mut me) = cx.get_mut(cell.root) {
                                me.set_accessibility_role(Aria::LIST_ITEM);
                                me.set_accessibility_label(tr_args("action-run", &[("name", name.clone().into())]));
                            }
                            set(cx, cell.label, name);
                            cell_icon(cx, cell, icons::PLAY, ACCENT, None);
                        }
                        None => {}
                    }
                },
            )),
            empty_state(icons::LAYOUT_PANEL_TOP, &tr("empty-screens-title"), &tr("empty-screens-hint"))
                .observe(|me, app: &App| me.set_is_visible(app.app.is_some() && app.screen_rows().is_empty())),
        ),
    )
}

/// The left column: Screens over Controls, split by a divider (half and half until dragged).
/// Embedded (DrawFiddle has no screens) Controls alone.
fn left_column(app: &mut App) -> Build<SkiaLayout> {
    SkiaGrid::new()
        .fill_y()
        .row_spacing(0)
        .assign(&mut app.left_column)
        .observe(|me, app: &App| me.set_row_definitions(app.left_rows()))
        .children((
            screens_panel(app).row(0).observe(|me, app: &App| me.set_is_visible(!app.embedded)),
            divider(Side::Controls).row(1).observe(|me, app: &App| me.set_is_visible(!app.embedded)),
            controls_panel(app).row(2),
        ))
}

/// Controls: what the shown app can create, by group, with a search field. A click arms place
/// mode (the next click in the app puts one there); a drag puts one where it drops in Layers.
fn controls_panel(app: &mut App) -> Build<SkiaLayout> {
    let has_kinds = |app: &App| app.app.is_some() && !app.kinds().is_empty();
    panel(
        &tr("panel-controls"),
        Vec::<drawnui::Detached>::new(),
        (
            SkiaGrid::new().fill().row_definitions("Auto, *").row_spacing(0).observe(move |me, app: &App| me.set_is_visible(has_kinds(app))).children((
                search_field(app).row(0),
                SkiaLayer::new().fill().row(1).children((
                    panel_scroll().fill().content(
                        SkiaStack::new()
                            .spacing(0)
                            .padding((0, 0, 0, 4))
                            .accessibility_role(Aria::LIST)
                            .accessibility_label(tr("panel-controls"))
                            .assign(&mut app.controls_list)
                            .items(|app: &App| app.control_rows.len(), control_cell, bind_control),
                    ),
                    empty_state(icons::SEARCH, &tr("empty-controls-match-title"), &tr("empty-controls-match-hint")).observe(|me, app: &App| me.set_is_visible(app.control_rows.is_empty())),
                )),
            )),
            empty_state(icons::SQUARE_PLUS, &tr("empty-controls-title"), &tr("empty-controls-hint")).observe(|me, app: &App| me.set_is_visible(app.app.is_none())),
            empty_state(icons::SQUARE_PLUS, &tr("empty-controls-title"), &tr("empty-controls-wait")).observe(|me, app: &App| me.set_is_visible(app.app.is_some() && app.kinds().is_empty())),
        ),
    )
}

/// The Controls search: filters by name as typed, any case.
fn search_field(app: &mut App) -> Build<SkiaLayout> {
    let look = |me: &mut Mut<'_, SkiaEditor>, app: &App| me.set_stroke_color(c(if app.search_focused { ACCENT } else if me.base().is_hovered() { MUTED } else { LINE }));
    SkiaLayer::new().fill_x().padding((8, 8, 8, 6)).children((
        SkiaEditor::new()
            .font_size(12)
            .text_color(c(TEXT))
            .background_color(c(INPUT))
            .corner_radius(4)
            .stroke_color(c(LINE))
            .stroke_width(1)
            .cursor_color(c(ACCENT))
            .selection_color(Color::new(0x664C8DFF))
            .placeholder_text(tr("controls-search"))
            .placeholder_color(c(FAINT))
            .padding((26, 4, 6, 4))
            .fill_x()
            .accessibility_label(tr("controls-search"))
            .assign(&mut app.controls_search_field)
            .on_hovered(|_me, app: &mut App, _cx, _over| app.hover_changes = app.hover_changes.wrapping_add(1))
            .on_focus_changed(|_me, app: &mut App, _cx, focused| app.search_focused = focused)
            .observe(look)
            .on_text_changed(|_me, app: &mut App, cx, text| app.set_controls_search(text, cx)),
        icon(icons::SEARCH, 13.0, FAINT).margin((9, 0, 0, 0)).input_transparent(true),
    ))
}

#[derive(Default)]
pub struct CtlHandles {
    icon: Handle<SkiaSvg>,
    name: Handle<SkiaLabel>,
    root: Handle<SkiaLayout>,
}

/// A Controls row's background: its group header's, or selected while its control waits to be placed.
fn control_row_bg(app: &App, index: Option<usize>) -> Color {
    match index.and_then(|i| app.control_rows.get(i)) {
        Some(super::CtlRow::Header { .. }) => c(GROUP_HEADER),
        Some(super::CtlRow::Item(k)) if app.kinds().get(*k).is_some_and(|k| app.placing.as_deref() == Some(k.type_name.as_str())) => c(SELECTION),
        _ => Color::TRANSPARENT,
    }
}

/// A row of Controls: a group header, or a control type (its icon and short name). A click arms
/// place mode, a drag drops one onto Layers.
fn control_cell() -> (Build<SkiaLayout>, CtlHandles) {
    let mut h = CtlHandles::default();
    let cell = interactive(
        SkiaLayout::new()
            .fill_x()
            .height_request(ROW)
            .accessibility_role(Aria::LIST_ITEM)
            .accessibility_can_interact(Some(true))
            .assign(&mut h.root)
            .on_tapped(|me, app: &mut App, cx| {
                if left_click(cx)
                    && let Some(index) = me.base().context_index
                {
                    app.tap_control_row(index, cx);
                }
            })
            // A press on a control may become a drag onto Layers; a press that does not move is a tap.
            .consume_gestures(|me, app: &mut App, cx, g| {
                let Some(index) = me.base().context_index else { return false };
                let Some(&super::CtlRow::Item(kind)) = app.control_rows.get(index) else { return false };
                let scale = me.base().scale.max(0.1);
                match g.kind {
                    GestureKind::Down if g.button == MouseButton::Left => {
                        let at = drawnui::skia::Point::new(g.location.x / scale, g.location.y / scale);
                        app.drag = Some(super::Drag { what: super::DragWhat::Control(kind), start: g.location, at, active: false });
                        app.pressed = Some(me.id());
                        true
                    }
                    GestureKind::Panning if app.drag.is_some() => {
                        app.drag_to(g.location, scale, cx);
                        true
                    }
                    GestureKind::Up if app.drag.is_some() => {
                        app.pressed = None;
                        // A drag ends here; a press that did not move goes on to be a tap.
                        app.end_drag(g.cancelled, cx)
                    }
                    _ => false,
                }
            })
            .observe(|me, app: &App| {
                let pressed = match me.base().context_index.and_then(|i| app.control_rows.get(i)) {
                    Some(super::CtlRow::Item(k)) => Some(app.kinds().get(*k).is_some_and(|k| app.placing.as_deref() == Some(k.type_name.as_str()))),
                    _ => None,
                };
                me.set_accessibility_is_pressed(pressed);
            }),
        control_row_bg,
    )
    .children(SkiaRow::new().spacing(8).margin((12, 0, 12, 0)).vertical_options(LayoutOptions::Center).children((
        SkiaSvg::from_string(icons::BOX).width_request(14).height_request(14).vertical_options(LayoutOptions::Center).assign(&mut h.icon),
        text(LIST_TEXT, TEXT).vertical_options(LayoutOptions::Center).assign(&mut h.name),
    )));
    (cell, h)
}

fn bind_control(cell: &CtlHandles, app: &App, index: usize, cx: &mut Cx) {
    let hovered = cx.base(cell.root).is_some_and(|b| b.is_hovered());
    match app.control_rows.get(index) {
        Some(super::CtlRow::Header { group, count }) => {
            let open = !app.folded_controls.contains(group) || !app.controls_search.trim().is_empty();
            set(cx, cell.name, format!("{}  {}  ({count})", if open { "\u{2212}" } else { "+" }, group_title(group).to_uppercase()));
            if let Some(mut me) = cx.get_mut(cell.name) {
                me.set_font_family("Semibold");
                me.set_font_size(12.0);
                me.set_text_color(c(MUTED));
            }
            if let Some(mut me) = cx.get_mut(cell.icon) {
                me.set_is_visible(false);
            }
            if let Some(mut me) = cx.get_mut(cell.root) {
                me.set_accessibility_role(Aria::BUTTON);
                me.set_accessibility_hint(String::new());
                me.set_accessibility_label(format!(
                    "{}, {}, {}",
                    group_title(group),
                    tr_args("controls-count", &[("count", (*count).into())]),
                    tr(if open { "group-open" } else { "group-folded" })
                ));
            }
        }
        Some(super::CtlRow::Item(k)) => {
            let Some(kind) = app.kinds().get(*k) else { return };
            let (svg, tone) = type_icon(&kind.type_name, kind.own);
            let name = short_type(&kind.type_name).to_owned();
            set(cx, cell.name, name.clone());
            if let Some(mut me) = cx.get_mut(cell.name) {
                me.set_font_family("Default");
                me.set_font_size(LIST_TEXT);
                me.set_text_color(c(TEXT));
            }
            if let Some(mut me) = cx.get_mut(cell.icon) {
                me.set_is_visible(true);
                me.set_svg_string(svg);
                me.set_tint_color(c(tone));
            }
            if let Some(mut me) = cx.get_mut(cell.root) {
                me.set_accessibility_role(Aria::LIST_ITEM);
                me.set_accessibility_label(format!("{name}, {}", group_title(super::App::kind_group(kind))));
                me.set_accessibility_hint(tr("control-row-hint"));
            }
        }
        None => {}
    }
    if let Some(mut me) = cx.get_mut(cell.root) {
        me.set_background_color(veiled(control_row_bg(app, Some(index)), hovered, false));
    }
}

fn layers_panel(app: &mut App) -> Build<SkiaLayout> {
    panel_on(
        LAYERS,
        &tr("panel-layers"),
        (
            text(11.0, FAINT).vertical_options(LayoutOptions::Center).observe(|me, app: &App| {
                me.set_text(if app.rows.is_empty() && !(app.app.is_some() && app.tree.revision > 0) { String::new() } else { tr_args("controls-count", &[("count", app.controls_in_app().into())]) });
                // In a page: its Undo / Redo buttons follow the history.
                app.post_history();
            }),
            // The desktop window has Pick in its title bar.
            pick_button().observe(|me, app: &App| me.set_is_visible(app.embedded)),
        ),
        (
            changes_strip(),
            // Both ways: a deep tree in a narrow panel scrolls sideways too (its width from layers_width).
            // In a page, under the strip of changes not in code while it shows.
            panel_scroll()
                .orientation(ScrollOrientation::Both)
                .scroll_bar_horizontal(scroll_bar())
                .fill()
                .observe(|me, app: &App| me.set_margin(Thickness::new(0.0, if changes_strip_shown(app) { CHANGES_STRIP } else { 0.0 }, 0.0, 0.0)))
                .assign(&mut app.layers_scroll)
                .on_scrolled(|_me, app: &mut App, _cx, offset| (app.layers_offset_x, app.layers_offset) = (offset.x, offset.y))
                // A click on the empty space below the last row lets go of the selection.
                .accessibility_label(tr("panel-layers"))
                .on_tapped(|me, app: &mut App, cx| {
                    let Some(at) = cx.gesture().filter(|_| left_click(cx)).map(|g| g.location) else { return };
                    let base = me.base();
                    let y = (at.y - base.rect.top) / base.scale.max(0.1) - app.layers_offset;
                    if y > 4.0 + app.rows.len() as f32 * ROW {
                        app.deselect(cx);
                    }
                })
                .content(SkiaStack::new().spacing(0).padding((0, 4)).observe(|me, app: &App| me.set_minimum_width_request(app.layers_width)).accessibility_role(Aria::LIST).accessibility_label(tr("panel-layers")).assign(&mut app.tree_list).items(
                |app: &App| app.rows.len(),
                || {
                    let (cell, h) = row_cell(true);
                    (
                        cell.on_tapped(|me, app: &mut App, cx| {
                            if !left_click(cx) {
                                return;
                            }
                            let time = cx.gesture().map_or(0.0, |g| g.time_ms);
                            if let Some(index) = me.base().context_index {
                                app.tap_tree_row(index, time, cx);
                            }
                        })
                        // A row dragged with the mouse moves its control (a touch pans the list);
                        // a press that does not move goes on to be a tap.
                        .consume_gestures(|me, app: &mut App, cx, g| {
                            let scale = me.base().scale.max(0.1);
                            match g.kind {
                                GestureKind::Down if g.button == MouseButton::Left && !g.touch => {
                                    let id = me.base().context_index.and_then(|i| app.rows.get(i)).map(|r| app.tree.nodes[r.node].id);
                                    // Not the root, nor a library part or a control of a template's cell, nor a control taken out here.
                                    app.row_press = id.filter(|id| app.tree.nodes.iter().any(|n| n.id == *id && n.parent.is_some() && !app.locked(n)) && !app.is_removed(*id)).map(|id| (id, g.location));
                                    app.swallow_up = false;
                                    app.row_press.is_some()
                                }
                                GestureKind::Panning if app.drag.is_some() => {
                                    app.drag_to(g.location, scale, cx);
                                    true
                                }
                                GestureKind::Panning if app.row_press.is_some() => {
                                    let (id, start) = app.row_press.expect("a press");
                                    if (g.location.x - start.x).hypot(g.location.y - start.y) > 4.0 * scale {
                                        app.row_press = None;
                                        let at = drawnui::skia::Point::new(start.x / scale, start.y / scale);
                                        app.drag = Some(super::Drag { what: super::DragWhat::Row(id), start, at, active: false });
                                        app.drag_to(g.location, scale, cx);
                                    }
                                    true
                                }
                                GestureKind::Up => {
                                    app.row_press = None;
                                    if std::mem::take(&mut app.swallow_up) {
                                        return true;
                                    }
                                    app.drag.is_some() && app.end_drag(g.cancelled, cx)
                                }
                                _ => false,
                            }
                        })
                        .observe(|me, app: &App| {
                            let shift = me.base().context_index.map_or(0.0, |i| app.row_shift(i));
                            me.set_translation_y(shift);
                        }),
                        h,
                    )
                },
                |cell: &RowHandles, app: &App, index: usize, cx: &mut Cx| {
                    let Some(row) = app.rows.get(index) else { return };
                    let node = &app.tree.nodes[row.node];
                    let own = node.assembly.as_deref().is_some_and(|a| !a.starts_with("DrawnUi"));
                    let (svg, color) = type_icon(&node.type_name, own);
                    let fold = row.has_children.then(|| app.collapsed.contains(&node.id));
                    if let Some(mut me) = cx.get_mut(cell.toggle) {
                        me.set_tag(index.to_string());
                        me.set_margin(Thickness::new(GUIDE_LEFT + row.depth as f32 * GUIDE_STEP, 0.0, 0.0, 0.0));
                        me.set_is_visible(row.has_children);
                    }
                    cell_icon(cx, cell, svg, color, Some((row, fold)));
                    // The name the code gives it first (the helper that made it), then its type, dimmed.
                    set(cx, cell.label, node.made_by.clone().unwrap_or_else(|| short_type(&node.type_name).to_owned()));
                    let named = match (&node.field, &node.tag) {
                        (Some(f), _) => f.rsplit('.').next().unwrap_or(f).to_owned(),
                        (None, Some(t)) => format!("#{t}"),
                        _ => String::new(),
                    };
                    let named = match &node.made_by {
                        Some(_) => [short_type(&node.type_name).to_owned(), named].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "),
                        None => named,
                    };
                    // A recycled cell: the item it showed when the tree was taken.
                    let item = node.template_index.map(|i| tr_args("row-item", &[("index", (i as i64).into())]));
                    let change = app.row_change(node.id);
                    // A control taken out here says so on its row.
                    let gone = change.is_some_and(|(removed, _)| removed).then(|| tr("row-removed-tag"));
                    let detail = [Some(named), item.clone(), gone].into_iter().flatten().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
                    // An empty label still takes its spacing in the row: hide it instead.
                    if let Some(mut me) = cx.get_mut(cell.detail) {
                        me.set_is_visible(!detail.is_empty());
                    }
                    set(cx, cell.detail, detail);
                    let preview = node.text.as_deref().filter(|t| !t.is_empty()).map(|t| format!("\u{201C}{t}\u{201D}"));
                    set(cx, cell.preview, preview.unwrap_or_default());
                    // Added or taken out here: a dot until it is written to code, Undo, and Write until written.
                    let unwritten = change.is_some_and(|(_, written)| !written);
                    if let Some(mut me) = cx.get_mut(cell.added) {
                        me.set_is_visible(unwritten);
                    }
                    let selected = app.selected == Some(index);
                    // The selected row keeps full brightness, the others are a little dimmer.
                    if let Some(mut me) = cx.get_mut(cell.label) {
                        me.set_text_color(c(if selected { TEXT } else { LAYERS_TEXT }));
                    }
                    if let Some(mut me) = cx.get_mut(cell.toggle) {
                        let name = short_type(&node.type_name);
                        me.set_accessibility_role(if row.has_children { Aria::BUTTON } else { Aria::PRESENTATION });
                        me.set_accessibility_label(tr_args(if fold == Some(true) { "unfold" } else { "fold" }, &[("name", name.to_owned().into())]));
                    }
                    if let Some(mut me) = cx.get_mut(cell.root) {
                        let hovered = me.base().is_hovered();
                        me.set_background_color(row_bg(selected, hovered));
                        let text = node.text.as_deref().filter(|t| !t.is_empty()).map(|t| format!(", \u{201C}{t}\u{201D}")).unwrap_or_default();
                        let field = node.field.as_deref().map(|f| format!(", {}", f.rsplit('.').next().unwrap_or(f))).unwrap_or_default();
                        let field = match &item {
                            Some(item) => format!("{field}, {item}"),
                            None => field,
                        };
                        let mut folded = match fold {
                            Some(true) => format!(", {}", tr("row-folded")),
                            Some(false) => format!(", {}", tr("row-open")),
                            None => String::new(),
                        };
                        if let Some((removed, written)) = change {
                            let moved = app.moved_key(node.id).is_some();
                            let key = match (removed, written) {
                                (false, false) if moved => "row-moved",
                                (false, true) if moved => "row-moved-written",
                                (false, false) => "row-added",
                                (false, true) => "row-added-written",
                                (true, false) => "row-removed",
                                (true, true) => "row-removed-written",
                            };
                            folded = format!("{folded}, {}", tr(key));
                        }
                        // The code's name and the type: "Circle, SkiaShape".
                        let name = match &node.made_by {
                            Some(made_by) => format!("{made_by}, {}", short_type(&node.type_name)),
                            None => short_type(&node.type_name).to_owned(),
                        };
                        me.set_accessibility_label(format!(
                            "{name}{field}{text}, {}{folded}{}",
                            tr_args("row-level", &[("level", (row.depth + 1).into())]),
                            if selected { format!(", {}", tr("row-selected")) } else { String::new() }
                        ));
                        // Controls not on screen now (scrolled away, under a pushed page) read dimmed.
                        // A control taken out here: its name dimmed, its Undo and Write not.
                        let removed = change.is_some_and(|(removed, _)| removed);
                        let carried = app.drag.is_some_and(|d| d.active) && app.dragged_node() == Some(node.id);
                        me.set_opacity(if carried { 0.35 } else if node.visible || removed { 1.0 } else { 0.45 });
                        if let Some(mut content) = cx.get_mut(cell.content) {
                            content.set_opacity(if removed { 0.5 } else { 1.0 });
                        }
                    }
                },
            )),
            // Undo and Write of the selected row and of the row under the mouse, at the right edge of
            // the visible list whatever its width and sideways scroll.
            row_strip(false),
            row_strip(true),
            // A connected app that shows no controls of its own yet (an empty DrawFiddle snippet: only
            // the page's own root layout, with nothing in it).
            // In DrawFiddle the user writes the code: the hint says where.
            empty_state(icons::LAYOUT_PANEL_TOP, &tr("empty-tree-title"), &tr(if app.embedded { "empty-tree-hint-page" } else { "empty-tree-hint" }))
                .assign(&mut app.empty_tree)
                // No control of the app's own to list (none, or only a page's own wrapper).
                .observe(|me, app: &App| me.set_is_visible(app.app.is_some() && app.tree.revision > 0 && app.outline.is_empty())),
            SkiaStack::new()
                .spacing(14)
                .center()
                .observe(|me, app: &App| me.set_is_visible(app.apps.is_empty()))
                .children((
                    empty_state(icons::UNPLUG, &tr("empty-app-title"), &tr("empty-app-hint")),
                    interactive(SkiaShape::new().accessibility_role(Aria::BUTTON), |_, _| c(ACCENT))
                        .corner_radius(6)
                        .padding((14, 7))
                        .horizontal_options(LayoutOptions::Center)
                        .observe(|me, app: &App| {
                            me.set_is_visible(!app.recent.is_empty());
                            me.set_accessibility_label(app.recent.first().map_or(String::new(), |r| tr_args("app-run", &[("name", r.name.clone().into())])));
                        })
                        .on_tapped(|_me, app: &mut App, cx| {
                            if !left_click(cx) {
                                return;
                            }
                            if let Some(r) = app.recent.first() {
                                app.run_project(r.project.clone(), cx);
                            }
                        })
                        .children(SkiaRow::new().spacing(6).children((
                            icon(icons::PLAY, 14.0, ON_ACCENT),
                            bold(12.0, ON_ACCENT).observe(|me, app: &App| {
                                let text = match app.recent.first() {
                                    Some(r) if app.run_underway(&r.project) => tr_args("app-starting-name", &[("name", r.name.clone().into())]),
                                    Some(r) => tr_args("app-run", &[("name", r.name.clone().into())]),
                                    None => String::new(),
                                };
                                me.set_text(text);
                            }),
                        ))),
                )),
            drop_indicator(app),
        ),
    )
}

/// Where a control dragged from Controls goes, over Layers: a line between two rows (it goes
/// there, among that row's siblings), or a frame around a container row (it goes in, last).
/// Where a dragged control lands, over Layers: the gap the rows open, the only drop mark, at the
/// indentation it lands at; dropped into a layout, a thin accent line under that layout's name. Both
/// follow the list as it scrolls, either way.
fn drop_indicator(app: &mut App) -> Build<SkiaLayout> {
    SkiaLayer::new().fill().input_transparent(true).accessibility_role(Aria::PRESENTATION).children((
        SkiaShape::new()
            .assign(&mut app.gap_box)
            .fill_x()
            .corner_radius(4)
            .stroke_width(1.5)
            .stroke_color(c(ACCENT))
            .background_color({
                let a = c(ACCENT);
                Color::from_argb(0x18, a.r(), a.g(), a.b())
            })
            .vertical_options(LayoutOptions::Start)
            .is_visible(false)
            .observe(|me, app: &App| {
                let g = app.gap;
                me.set_is_visible(g.open > 0.05);
                if g.open > 0.05 {
                    let depth = g.depth;
                    let height = (ROW - 4.0) * g.open;
                    let top = PROPS_PAD + g.pos * ROW + app.layers_offset + (ROW - height) / 2.0;
                    me.set_height_request(height);
                    me.set_opacity(g.open);
                    me.set_margin(Thickness::new(gap_left(depth) + app.layers_offset_x, top, 10.0, 0.0));
                }
            }),
        // The layout that takes it: a thin accent line under its icon and name.
        SkiaShape::new()
            .assign(&mut app.into_bar)
            .height_request(2)
            .corner_radius(1)
            .background_color(c(ACCENT))
            .horizontal_options(LayoutOptions::Start)
            .vertical_options(LayoutOptions::Start)
            .is_visible(false)
            .observe(|me, app: &App| {
                let into = app.drop.filter(|d| d.into && app.gap.open > 0.05);
                me.set_is_visible(into.is_some());
                if let Some(d) = into {
                    let name = app.rows.get(d.row).map_or(0, |r| short_type(&app.tree.nodes[r.node].type_name).chars().count());
                    let top = PROPS_PAD + d.row as f32 * ROW + app.layers_offset + app.row_shift(d.row) + ROW - 4.0;
                    me.set_width_request(14.0 + 6.0 + name as f32 * 7.6);
                    me.set_margin(Thickness::new(row_text_left(d.depth) + app.layers_offset_x, top, 0.0, 0.0));
                }
            }),
    ))
}

/// Where the gap starts for a control landing at tree depth `depth`: at the icons of that depth.
pub(crate) fn gap_left(depth: usize) -> f32 {
    row_text_left(depth) - 4.0
}

fn properties_panel(app: &mut App) -> Build<SkiaLayout> {
    panel(
        &tr("panel-properties"),
        (control_actions(), write_all_button(), order_switch()),
        (
            SkiaGrid::new().fill().row_definitions("Auto, *").observe(|me, app: &App| me.set_is_visible(app.selected.is_some())).children((
                selection_header().row(0),
                SkiaLayer::new().fill().row(1).children((
                    panel_scroll()
                        .fill()
                        .assign(&mut app.props_scroll)
                        .on_scrolled(|_me, app: &mut App, cx, offset| {
                            app.props_offset = offset.y;
                            app.place_editor(cx);
                        })
                        .content(SkiaStack::new().spacing(0).padding((0.0, PROPS_PAD)).accessibility_role(Aria::LIST).accessibility_label(tr("panel-properties")).assign(&mut app.props_list).items(
                            |app: &App| app.prop_rows.len(),
                            prop_cell,
                            bind_prop,
                        )),
                    edit_overlay(app),
                )),
            )),
            // "Select a control" only when there is one to select (Layers says why when there is none).
            empty_state(icons::CROSSHAIR, &tr("empty-selection-title"), &tr("empty-selection-hint"))
                .observe(|me, app: &App| me.set_is_visible(app.selected.is_none() && !app.rows.is_empty() && app.placing.is_none())),
            // A control waits to be placed: how to finish (with a selection, its status line says it).
            SkiaStack::new().spacing(8).padding(24).center().input_transparent(true).observe(|me, app: &App| me.set_is_visible(app.selected.is_none() && app.placing.is_some())).children((
                icon(icons::MOUSE_POINTER_CLICK, 28.0, ACCENT).horizontal_options(LayoutOptions::Center),
                bold(13.0, TEXT)
                    .horizontal_options(LayoutOptions::Center)
                    .accessibility_live(Aria::LIVE_POLITE)
                    .observe(|me, app: &App| me.set_text(app.placing.as_deref().map(|t| tr_args("place-title", &[("type", short_type(t).to_owned().into())])).unwrap_or_default())),
                text(12.0, MUTED).text(tr("place-hint")).max_lines(3).horizontal_text_alignment(TextAlignment::Center).horizontal_options(LayoutOptions::Center),
            )),
        ),
    )
}

/// Properties' header for the selected control: the name the code gives it (the helper that made it),
/// else its type; under it its type when the name is the code's, then the field that holds it.
pub(crate) fn selection_names(app: &App) -> (String, String) {
    let Some(n) = app.selected_node() else { return (String::new(), String::new()) };
    let field = n.field.as_deref().map(|f| f.rsplit('.').take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("."));
    let kind = n.made_by.is_some().then(|| short_type(&n.type_name).to_owned());
    (n.made_by.clone().unwrap_or_else(|| short_type(&n.type_name).to_owned()), [kind, field].into_iter().flatten().collect::<Vec<_>>().join(" · "))
}

/// The selected control: its type, the field that holds it, and one line for what the last edit
/// did. That line always takes its place, so the list below never moves.
fn selection_header() -> Build<SkiaLayout> {
    SkiaLayout::new().fill_x().padding((12, 10, 12, 8)).children(SkiaStack::new().spacing(2).children((
        bold(15.0, TEXT).observe(|me, app: &App| me.set_text(selection_names(app).0)),
        text(12.0, FIELD).height_request(17).observe(|me, app: &App| me.set_text(selection_names(app).1)),
        // A narrow panel cuts the line short with an ellipsis: the row fills, the text takes what the icon leaves.
        SkiaRow::new().spacing(6).height_request(17).fill_x().children((
            SkiaSvg::from_string(icons::CIRCLE_CHECK).width_request(13).height_request(13).vertical_options(LayoutOptions::Center).observe(|me, app: &App| {
                me.set_svg_string(if app.status_error { icons::CIRCLE_ALERT } else { icons::CIRCLE_CHECK });
                me.set_tint_color(c(if app.status_error { WARN } else { OK }));
                me.set_opacity(if app.status.is_empty() { 0.0 } else { 1.0 });
            }),
            // Cut to one line here; the whole text in its tooltip and for a screen reader.
            text(12.0, MUTED).max_lines(1).fill_x().vertical_options(LayoutOptions::Center).accessibility_role(Aria::STATUS).accessibility_live(Aria::LIVE_POLITE).observe(|me, app: &App| {
                me.set_text(app.status.as_str());
                me.set_tooltip(app.status.as_str());
                me.set_accessibility_label(app.status.as_str());
            }),
        )),
    )))
}

/// The list's top and bottom padding, in points (the edit field is placed from it).
pub const PROPS_PAD: f32 = 4.0;

/// The width a property row keeps at its right for Undo and Write (two 26 pt buttons, a gap),
/// left of the scroll bar's lane.
pub const ACTIONS: f32 = 56.0 + BAR_LANE;

/// A panel's scroll bar, points: its thickness, its distance from the edge, and how far beside it a
/// press still grabs it.
const BAR_THICKNESS: f32 = 5.0;
const BAR_MARGIN: f32 = 3.0;
const BAR_GRAB: f32 = 8.0;

/// The strip at a panel scroll's edge where a press is the bar's (the content gets no tap, a press
/// on the track jumps the list there), whether the bar shows or not: buttons stay out of it.
pub const BAR_LANE: f32 = BAR_MARGIN + BAR_THICKNESS + BAR_GRAB;

/// The one field that edits a value, laid over the edited row's value cell (the list never
/// moves): - / + for numbers, Enter applies in the app, Esc or leaving cancels.
fn edit_overlay(app: &mut App) -> Build<SkiaLayout> {
    SkiaRow::new()
        .spacing(4)
        .height_request(ROW)
        .margin((148.0, 0.0, ACTIONS, 0.0))
        .fill_x()
        .is_visible(false)
        .assign(&mut app.edit_overlay)
        .children((
            row_stepper("\u{2212}", -1.0).observe(|me, app: &App| me.set_is_visible(app.edit_step.is_some())),
            SkiaEditor::new()
                .font_size(12)
                .text_color(c(TEXT))
                .background_color(c(INPUT))
                .corner_radius(4)
                .stroke_color(c(ACCENT))
                .stroke_width(1)
                .cursor_color(c(ACCENT))
                .selection_color(Color::new(0x664C8DFF))
                .padding((6, 2))
                .fill_x()
                .vertical_options(LayoutOptions::Center)
                .accessibility_label(tr("value"))
                .assign(&mut app.editor)
                .on_text_changed(|_me, app: &mut App, _cx, text| app.edit_value = text.to_owned())
                .on_text_submitted(|_me, app: &mut App, cx, _text| {
                    app.apply_edit();
                    // The field lets go of the keyboard (in a browser the page's text input closes).
                    cx.focus(None::<drawnui::ControlId>);
                    app.end_edit(cx);
                })
                .on_focus_changed(|_me, _app: &mut App, cx, focused| {
                    // Clicking elsewhere ends the edit; what was typed and not applied is dropped.
                    // After this event: Esc unfocuses the field too, and its key handler (cancel,
                    // the value put back) must see the edit first.
                    if !focused {
                        cx.postpone_before_draw::<App>(|app, cx| {
                            if app.edit_prop.is_some() && app.edit_choices.is_empty() && cx.focused() != Some(app.editor.into()) {
                                app.end_edit(cx);
                            }
                        });
                    }
                }),
            row_stepper("+", 1.0).observe(|me, app: &App| me.set_is_visible(app.edit_step.is_some())),
        ))
}

/// Taps act on the left mouse button only; the right one is kept for context menus.
pub(super) fn left_click(cx: &Cx) -> bool {
    cx.gesture().is_none_or(|g| g.button == MouseButton::Left)
}

/// The dropdown of a property's choices, under its value. A tap sets one (a flags enum toggles it
/// and the dropdown stays open); the current ones carry a check.
pub fn choices_popup(app: &mut App) -> Build<SkiaShape> {
    // Each row kept, for typing to move the keyboard to it.
    app.choice_items = vec![Handle::default(); app.edit_choices.len()];
    let rows: Vec<_> = app
        .edit_choices
        .iter()
        .cloned()
        .zip(app.choice_items.iter_mut())
        .map(|(value, handle)| {
            let selected = value.clone();
            let tapped = value.clone();
            // The empty choice (it clears an optional property) reads "(empty)", not a blank row.
            let row = if value.is_empty() {
                menu_item(tr("choice-empty"), move |app| app.choice_on(&selected), move |app, cx| app.pick_choice(tapped.clone(), cx)).accessibility_label(tr("choice-empty-label"))
            } else {
                menu_item(value, move |app| app.choice_on(&selected), move |app, cx| app.pick_choice(tapped.clone(), cx))
            };
            row.assign(handle)
        })
        .collect();
    let longest = app.edit_choices.iter().map(|c| if c.is_empty() { tr("choice-empty").chars().count() } else { c.chars().count() }).max().unwrap_or(4);
    menu_panel(rows, menu_width(longest))
}

/// A menu's width from its longest item, in points: check, gaps, ~7 pt per character at 12 pt.
/// Its rows fill that width, so their hover and selected backgrounds span the whole menu (a Fill
/// row in a content-sized menu gets no width of its own in drawnui 0.1 preview).
fn menu_width(longest_chars: usize) -> f32 {
    (6.0 + 13.0 + 8.0 + longest_chars as f32 * 7.2 + 18.0).max(110.0)
}

/// A popup menu's panel: rounded, raised above the window's panels.
fn menu_panel(rows: Vec<Build<SkiaShape>>, width: f32) -> Build<SkiaShape> {
    SkiaShape::new()
        .accessibility_role(Aria::MENU)
        .width_request(width + 8.0)
        .corner_radius(6)
        .background_color(c(HEADER))
        .stroke_color(c(LINE))
        .stroke_width(1)
        .padding(4)
        .children(SkiaStack::new().spacing(2).children(rows))
}

/// One row of a popup menu, the same in every menu: the row under the mouse lights up, the
/// current value reads selected (accent background, check, bold text).
fn menu_item(label: String, selected: impl Fn(&App) -> bool + Clone + 'static, tap: impl Fn(&mut App, &mut Cx) + 'static) -> Build<SkiaShape> {
    let key = label.clone();
    let hovered = label.clone();
    let (sel_bg, sel_check, sel_text) = (selected.clone(), selected.clone(), selected);
    let checked = sel_bg.clone();
    SkiaShape::new()
        .corner_radius(4)
        .fill_x()
        .height_request(ROW)
        .padding((6, 0, 14, 0))
        .accessibility_role(Aria::MENU_ITEM_RADIO)
        .accessibility_label(label.clone())
        .observe(move |me, app: &App| me.set_accessibility_is_pressed(Some(checked(app))))
        .on_hovered(move |_me, app: &mut App, _cx, on| {
            if on {
                app.menu_hover = Some(key.clone());
            } else if app.menu_hover.as_deref() == Some(key.as_str()) {
                app.menu_hover = None;
            }
        })
        .on_tapped(move |_me, app: &mut App, cx| if left_click(cx) { tap(app, cx) })
        .observe(move |me, app: &App| {
            let hover = app.menu_hover.as_deref() == Some(hovered.as_str());
            let bg = if sel_bg(app) {
                c(if hover { MENU_SELECTED_HOVER } else { MENU_SELECTED })
            } else if hover {
                c(MENU_HOVER)
            } else {
                Color::TRANSPARENT
            };
            me.set_background_color(bg);
        })
        .children(
            SkiaRow::new().spacing(8).vertical_options(LayoutOptions::Center).children((
                SkiaSvg::from_string(icons::CHECK)
                    .width_request(13)
                    .height_request(13)
                    .tint_color(c(ACCENT))
                    .vertical_options(LayoutOptions::Center)
                    .observe(move |me, app: &App| me.set_opacity(if sel_check(app) { 1.0 } else { 0.0 })),
                text(12.0, TEXT).text(label).observe(move |me, app: &App| {
                    let on = sel_text(app);
                    me.set_font_family(if on { "Semibold" } else { "Default" });
                    me.set_text_color(c(if on { ON_SELECTED } else { TEXT }));
                }),
            )),
        )
}

/// The Properties order switch: Set, A to Z (default), Groups.
fn order_switch() -> Build<SkiaLayout> {
    segments(&tr("order"), (sort_toggle(&tr("order-set"), PropsOrder::Set), sort_toggle(&tr("order-groups"), PropsOrder::Groups)))
}

/// A theme's name in the current language (the themes file's name when the language has none).
fn theme_name(theme: Theme) -> String {
    let key = format!("theme-{}", theme.id());
    let name = tr(&key);
    if name == key { theme.name().to_owned() } else { name }
}

/// A property group's name in the current language (groups are keyed by their English name).
fn group_title(group: &str) -> String {
    tr(&format!("group-{}", group.to_lowercase()))
}

/// What the Language button says: the chosen language's own name, or "System (its language)".
fn language_label(app: &App) -> String {
    match &app.settings.language {
        Some(code) => i18n::native_name(code).to_owned(),
        None => tr_args("language-system", &[("language", i18n::native_name(i18n::system_language()).into())]),
    }
}

/// The Language button: the current language and a chevron; opens the language menu under it.
fn language_button(app: &mut App) -> Build<SkiaShape> {
    interactive(
        SkiaShape::new()
            .corner_radius(6)
            .stroke_width(1)
            .stroke_color(c(LINE))
            .padding((10, 5, 8, 5))
            .vertical_options(LayoutOptions::Center)
            .accessibility_role(Aria::BUTTON)
            .assign(&mut app.language_button)
            .observe(|me, app: &App| me.set_accessibility_label(format!("{}: {}", tr("language"), language_label(app))))
            .on_tapped(|me, app: &mut App, cx| {
                if !left_click(cx) {
                    return;
                }
                let s = me.base().scale.max(0.1);
                let r = me.base().rect; // layout pixels: the page's scroll is not applied yet
                let scrolled = cx.find::<SkiaScroll>(app.settings_scroll).map_or(0.0, |sc| sc.viewport_offset_y());
                // The scroll's offset: 0 at the start, negative when scrolled down.
                let anchor = drawnui::skia::Rect::new(r.left / s, r.top / s + scrolled, r.right / s, r.bottom / s + scrolled);
                let center = drawnui::skia::Point::new(r.center_x(), r.center_y() + scrolled * s);
                app.open_language_menu(anchor, center, cx);
            }),
        |_, _| c(PANEL),
    )
    .children(SkiaRow::new().spacing(6).vertical_options(LayoutOptions::Center).children((
        text(12.0, TEXT).vertical_options(LayoutOptions::Center).observe(|me, app: &App| me.set_text(language_label(app))),
        icon(icons::CHEVRON_DOWN, 14.0, MUTED),
    )))
}

/// The language menu: System first (it follows Windows), then every language by its own name.
pub fn language_menu(app: &App) -> Build<SkiaShape> {
    let mut options: Vec<(String, Option<String>)> =
        vec![(tr_args("language-system", &[("language", i18n::native_name(i18n::system_language()).into())]), None)];
    options.extend(i18n::LANGUAGES.iter().map(|(code, name)| (name.to_string(), Some(code.to_string()))));
    let longest = options.iter().map(|(label, _)| label.chars().count()).max().unwrap_or(8);
    let rows = options
        .into_iter()
        .map(|(label, value)| {
            let (selected, picked) = (value.clone(), value);
            menu_item(label, move |app: &App| app.settings.language == selected, move |app, cx| app.pick_language(picked.clone(), cx))
        })
        .collect();
    let _ = app;
    menu_panel(rows, menu_width(longest))
}

/// A panel's scroll with its vertical bar: it shows while the content moves and while the pointer
/// is over the panel (desktop), fades out a second after both stop (touch: after scrolling stops),
/// and its thumb can be dragged. Nothing draws while the bar is hidden.
fn panel_scroll() -> Build<SkiaScroll> {
    SkiaScroll::new()
        .scroll_bar(scroll_bar())
        .on_hovered(|me, _app: &mut App, _cx, over| me.set_keep_scroll_bars_visible(over))
}

fn scroll_bar() -> Build<SkiaScrollBar> {
    SkiaScrollBar::new().thumb_color(c(GUIDE)).thickness(BAR_THICKNESS).edge_margin(BAR_MARGIN).grab_padding(BAR_GRAB).is_draggable(true)
}

/// The width a Layers row needs, estimated from its depth and text lengths (the row's own layout:
/// indent, icon, type, field or tag, text preview), so the list can scroll sideways without
/// measuring rows that are not shown. Long text previews count up to 48 characters.
pub(crate) fn layers_row_width(node: &designer_protocol::Node, depth: usize) -> f32 {
    let chars = |s: &str| s.chars().count() as f32;
    let mut w = GUIDE_LEFT + (depth as f32 + 1.5) * GUIDE_STEP - 7.0 + 14.0 + 6.0 + chars(node.made_by.as_deref().unwrap_or_else(|| short_type(&node.type_name))) * 7.6;
    if node.made_by.is_some() {
        w += 6.0 + (chars(short_type(&node.type_name)) + 3.0) * 7.0;
    }
    if let Some(name) = node.field.as_deref().map(|f| f.rsplit('.').next().unwrap_or(f)).or(node.tag.as_deref()) {
        w += 6.0 + (chars(name) + 1.0) * 7.0;
    }
    if let Some(index) = node.template_index {
        w += 6.0 + (8.0 + index.to_string().len() as f32) * 7.0;
    }
    if let Some(text) = node.text.as_deref().filter(|t| !t.is_empty()) {
        w += 6.0 + (chars(text).min(48.0) + 2.0) * 6.6;
    }
    w + 12.0 + 8.0
}

/// A control's rect in points, for a popup under it (the title bar does not scroll).
fn points_rect(base: &drawnui::Base) -> drawnui::skia::Rect {
    let s = base.scale.max(0.1);
    let r = base.rect;
    drawnui::skia::Rect::new(r.left / s, r.top / s, r.right / s, r.bottom / s)
}

/// The connected apps by platform, in the order the platforms connected: (platform, app indexes).
pub(crate) fn app_menu_groups(app: &App) -> Vec<(String, Vec<usize>)> {
    let mut groups: Vec<(String, Vec<usize>)> = app.platforms().into_iter().map(|p| (p, vec![])).collect();
    let mut others = vec![];
    for (i, a) in app.apps.iter().enumerate() {
        match a.info.platform.as_ref().and_then(|p| groups.iter_mut().find(|(g, _)| g == p)) {
            Some((_, list)) => list.push(i),
            None => others.push(i),
        }
    }
    if !others.is_empty() {
        groups.push((String::new(), others));
    }
    groups
}

/// Where a project lives, short: the folder that holds the project's repository or samples folder
/// ("DrawnUi", "designer-android-test"), so copies of one project tell apart.
fn project_place(project: &str) -> String {
    let path = std::path::Path::new(project);
    let dir = if path.extension().is_some() { path.parent().unwrap_or(path) } else { path };
    dir.ancestors()
        .find_map(|d| d.parent().filter(|p| p.join(".git").exists() || d.join(".git").exists()).map(|_| d))
        .and_then(|d| d.file_name())
        .or_else(|| dir.parent().and_then(|p| p.file_name()))
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// A group title in a menu (a platform), not clickable.
fn menu_header(caption: &str) -> Build<SkiaShape> {
    SkiaShape::new()
        .fill_x()
        .padding((8, 6, 8, 2))
        .background_color(Color::TRANSPARENT)
        .children(bold(10.0, FAINT).text(caption.to_uppercase()).accessibility_role(Aria::HEADING))
}

/// The title bar's apps menu: the connected apps under their platform, the shown one checked,
/// then the recent apps that are not running (a pick runs them).
pub fn app_menu(app: &App) -> Build<SkiaShape> {
    let engine_head = |engine: &str| engine.split('-').nth(1).unwrap_or(engine).to_uppercase();
    // Apps with the same name (copies of one project) say which folder they come from.
    let names: Vec<&str> = app.apps.iter().map(|a| a.info.name.as_str()).chain(app.recent.iter().map(|r| r.name.as_str())).collect();
    let twin = |name: &str| names.iter().filter(|n| **n == name).count() > 1;
    let mut rows: Vec<Build<SkiaShape>> = vec![];
    let mut longest = 8;
    for (platform, list) in app_menu_groups(app) {
        if !platform.is_empty() {
            rows.push(menu_header(platform_chip(&platform).0));
        }
        for i in list {
            let a = &app.apps[i];
            let mut label = format!("{} · {}", a.info.name, engine_head(&a.info.engine));
            if twin(&a.info.name) {
                label = format!("{label} · {}", a.info.project.as_deref().map(project_place).unwrap_or_default());
            }
            longest = longest.max(label.chars().count());
            let id = a.id;
            rows.push(menu_item(label, move |app: &App| app.app == Some(id), move |app, cx| app.pick_app(i, cx)));
        }
    }
    if !app.recent.is_empty() {
        rows.push(menu_header(&tr("apps-not-running")));
    }
    for (i, r) in app.recent.iter().enumerate() {
        let mut parts = vec![r.name.clone(), engine_head(&r.engine)];
        if let Some(p) = &r.platform {
            parts.push(platform_chip(p).0.to_owned());
        }
        if twin(&r.name) {
            parts.push(project_place(&r.project));
        }
        let label = parts.join(" · ");
        longest = longest.max(label.chars().count());
        let slot = app.apps.len() + i;
        rows.push(menu_item(label, |_: &App| false, move |app, cx| app.pick_app(slot, cx)));
    }
    menu_panel(rows, menu_width(longest))
}

/// One segment of the Properties order switch: Set, A to Z (default) or Groups.
fn sort_toggle(caption: &str, order: PropsOrder) -> Build<SkiaShape> {
    segment(caption, move |app| app.props_order == order, move |app, cx| app.set_props_order(order, cx))
}

/// The Layers header's Pick button: while on, the next click in the app selects a control here.
fn pick_button() -> Build<SkiaShape> {
    let mut me = Handle::<SkiaShape>::default();
    interactive(
        SkiaShape::new()
            .assign(&mut me)
            .corner_radius(6)
            .stroke_width(1)
            .padding((8, 3))
            .vertical_options(LayoutOptions::Center)
            .accessibility_role(Aria::BUTTON)
            .accessibility_label(tr("pick-label"))
            .observe(|me, app: &App| {
                me.set_stroke_color(c(if app.pick_mode { ACCENT } else { LINE }));
                me.set_accessibility_is_pressed(Some(app.pick_mode));
            })
            .on_tapped(|_me, app: &mut App, cx| if left_click(cx) { app.toggle_pick() }),
        |app, _| c(if app.pick_mode { SELECTION } else { PANEL }),
    )
        .children(SkiaRow::new().spacing(5).children((
            button_icon(icons::CROSSHAIR, 13.0, me, |app| app.pick_mode),
            bold(11.0, TEXT).observe(|me, app: &App| me.set_text(tr(if app.pick_mode { "pick-active" } else { "pick" }))),
        )))
}

/// The title bar's Pick, right of the apps: the pill's look, lit while the next click in the app
/// picks a control (Ctrl+P).
fn top_pick_button() -> Build<SkiaShape> {
    let mut me = Handle::<SkiaShape>::default();
    interactive(
        SkiaShape::new()
            .assign(&mut me)
            .corner_radius(12)
            .stroke_width(1)
            .padding((10, 4))
            .vertical_options(LayoutOptions::Center)
            .accessibility_role(Aria::BUTTON)
            .accessibility_label(tr("pick-label-keys"))
            .tooltip(tr("pick-label-keys"))
            .observe(|me, app: &App| {
                me.set_stroke_color(c(if app.pick_mode { ACCENT } else { LINE }));
                me.set_accessibility_is_pressed(Some(app.pick_mode));
            })
            .on_tapped(|_me, app: &mut App, cx| if left_click(cx) { app.toggle_pick() }),
        |app, _| c(if app.pick_mode { SELECTION } else { PANEL }),
    )
    .children(SkiaRow::new().spacing(6).children((
        button_icon(icons::CROSSHAIR, 13.0, me, |app| app.pick_mode),
        bold(12.0, TEXT).observe(|me, app: &App| me.set_text(tr(if app.pick_mode { "pick-active" } else { "pick" }))),
    )))
}

/// The height of the strip of changes not in code under the Layers header (in a page).
const CHANGES_STRIP: f32 = 30.0;

fn changes_strip_shown(app: &App) -> bool {
    app.embedded && app.unwritten_count() > 0
}

/// In a page, under the Layers header while there are changes not in code: how many on the left (cut
/// short with an ellipsis where Layers is narrow), Write all on the right.
fn changes_strip() -> Build<SkiaLayout> {
    SkiaGrid::new()
        .fill_x()
        .height_request(CHANGES_STRIP)
        .vertical_options(LayoutOptions::Start)
        .column_definitions("Auto, *, Auto")
        .column_spacing(6)
        .padding((12, 0, 8, 0))
        .background_color(c(HEADER))
        .is_visible(false)
        .observe(|me, app: &App| me.set_is_visible(changes_strip_shown(app)))
        .children((
            icon(icons::CODE, 13.0, ACCENT).column(0).vertical_options(LayoutOptions::Center),
            // The column's width: cut short with an ellipsis, never under Write all.
            SkiaLayer::new().fill().column(1).is_clipped_to_bounds(true).children(
                text(11.0, MUTED).max_lines(1).fill_x().vertical_options(LayoutOptions::Center).observe(|me, app: &App| me.set_text(tr_args("changes-not-in-code", &[("count", app.unwritten_count().into())]))),
            ),
            interactive(
                SkiaShape::new()
                    .corner_radius(6)
                    .stroke_width(1)
                    .stroke_color(c(ACCENT))
                    .padding((8, 3))
                    .column(2)
                    .vertical_options(LayoutOptions::Center)
                    .accessibility_role(Aria::BUTTON)
                    .observe(|me, app: &App| {
                        let label = tr_args("write-changes-label", &[("count", app.unwritten_count().into())]);
                        me.set_accessibility_label(label.clone());
                        me.set_tooltip(label);
                    })
                    .on_tapped(|_me, app: &mut App, cx| {
                        if left_click(cx) {
                            app.write_everything(cx);
                        }
                    }),
                |_, _| c(PANEL),
            )
            .children(bold(11.0, TEXT).text(tr("write-all"))),
        ))
}

/// How many changes are not in code, with Write all: in the desktop window's title bar, shown while
/// there are some (a page has `changes_strip` under the Layers header instead).
fn changes_button() -> Build<SkiaShape> {
    interactive(
        SkiaShape::new()
            .corner_radius(6)
            .stroke_width(1)
            .stroke_color(c(ACCENT))
            .padding((8, 3))
            .vertical_options(LayoutOptions::Center)
            .accessibility_role(Aria::BUTTON)
            .is_visible(false)
            .observe(|me, app: &App| {
                let count = app.unwritten_count();
                #[cfg(target_os = "emscripten")]
                crate::web::changes(count);
                me.set_is_visible(count > 0 && !app.embedded);
                me.set_accessibility_label(tr_args("write-changes-label", &[("count", count.into())]));
                me.set_tooltip(tr_args("write-changes-label", &[("count", count.into())]));
            })
            .on_tapped(|_me, app: &mut App, cx| {
                if left_click(cx) {
                    app.write_everything(cx);
                }
            }),
        |_, _| c(PANEL),
    )
    .children(SkiaRow::new().spacing(6).children((
        icon(icons::CODE, 13.0, ACCENT),
        text(11.0, MUTED).observe(|me, app: &App| me.set_text(tr_args("changes-not-in-code", &[("count", app.unwritten_count().into())]))),
        bold(11.0, TEXT).text(tr("write-all")),
    )))
}

/// A newer release on GitHub: its version in the title bar; a click opens what can be done with it.
#[cfg(not(target_os = "emscripten"))]
fn update_button() -> Build<SkiaShape> {
    use crate::update::Install;
    interactive(
        SkiaShape::new()
            .corner_radius(6)
            .stroke_width(1)
            .stroke_color(c(ACCENT))
            .padding((8, 3))
            .vertical_options(LayoutOptions::Center)
            .accessibility_role(Aria::BUTTON)
            .is_visible(false)
            .observe(|me, app: &App| {
                let label = app.offered().map(|u| update_line(app, &u.version)).unwrap_or_default();
                me.set_is_visible(app.offered().is_some());
                me.set_accessibility_label(label.as_str());
                me.set_tooltip(label.as_str());
            })
            .on_tapped(|me, app: &mut App, cx| {
                if left_click(cx) && app.install != Install::Downloading {
                    let s = me.base().scale.max(0.1);
                    let r = me.base().rect;
                    app.open_update_menu(drawnui::skia::Rect::new(r.left / s, r.top / s, r.right / s, r.bottom / s), cx);
                }
            }),
        |_, _| c(PANEL),
    )
    .children(SkiaRow::new().spacing(6).children((
        icon(icons::DOWNLOAD, 13.0, ACCENT),
        text(11.0, TEXT).observe(|me, app: &App| {
            let version = app.offered().map(|u| u.version.clone()).unwrap_or_default();
            me.set_text(tr_args(if app.install == Install::Downloading { "update-downloading" } else { "update-pill" }, &[("version", version.into())]));
        }),
    )))
}

/// The web build has no updates of its own (the page ships it).
#[cfg(target_os = "emscripten")]
fn update_button() -> Build<SkiaShape> {
    SkiaShape::new().is_visible(false)
}

/// Settings, About: Check for updates (desktop).
fn update_option() -> Build<SkiaLayout> {
    let row = SkiaStack::new().spacing(0).fill_x().margin((0, 16, 0, 0));
    #[cfg(not(target_os = "emscripten"))]
    let row = row.children(option_row(&tr("check-updates"), &tr("check-updates-hint"), setting_switch(tr("check-updates"), |app| !app.settings.updates_off, |app, on| app.set_check_updates(on))));
    #[cfg(target_os = "emscripten")]
    let row = row.is_visible(false);
    row
}

/// What the update stands at, in a sentence: available, downloading, or why it failed.
#[cfg(not(target_os = "emscripten"))]
fn update_line(app: &App, version: &str) -> String {
    use crate::update::Install;
    match &app.install {
        Install::Idle => tr_args("update-available", &[("version", version.to_owned().into())]),
        Install::Downloading => tr_args("update-downloading", &[("version", version.to_owned().into())]),
        Install::Failed(why) => tr_args("update-failed", &[("version", version.to_owned().into()), ("why", why.clone().into())]),
    }
}

/// The update's menu: Update and restart (after a failure, the download page instead), What's new, Later.
#[cfg(not(target_os = "emscripten"))]
pub fn update_menu(app: &App) -> Build<SkiaShape> {
    use crate::update::Install;
    let Some(update) = app.offered().cloned() else { return menu_panel(vec![], 110.0) };
    let title = update_line(app, &update.version);
    let failed = matches!(app.install, Install::Failed(_));
    let mut rows = vec![SkiaShape::new().fill_x().padding((10, 6)).accessibility_role(Aria::PRESENTATION).children(text(12.0, if failed { ERROR } else { MUTED }).fill_x().max_lines(3).text(title.clone()))];
    if failed {
        let url = update.url.clone();
        rows.push(menu_item(tr("update-download"), |_| false, move |app, cx| {
            app.close_popup(cx);
            cx.open_url(&url);
        }));
    } else {
        rows.push(menu_item(tr("update-install"), |_| false, |app, cx| app.update_and_restart(cx)));
    }
    let page = update.page.clone();
    rows.push(menu_item(tr("update-whats-new"), |_| false, move |app, cx| {
        app.close_popup(cx);
        cx.open_url(&page);
    }));
    rows.push(menu_item(tr("update-later"), |_| false, |app, cx| app.skip_update(cx)));
    let longest = [title.chars().count().min(48), tr("update-install").chars().count(), tr("update-whats-new").chars().count()].into_iter().max().unwrap_or(20);
    menu_panel(rows, menu_width(longest)).accessibility_label(title)
}

/// The question before Restart, Run or closing with changes not in code: write them first, go on
/// without them, or stay.
pub fn unwritten_menu(app: &App) -> Build<SkiaShape> {
    use super::writes::AfterFlush;
    let (write, skip) = match app.confirm {
        Some(AfterFlush::Run(_)) => ("unwritten-write-run", "unwritten-skip-run"),
        Some(AfterFlush::Close | AfterFlush::Update) => ("unwritten-write-close", "unwritten-skip-close"),
        _ => ("unwritten-write-restart", "unwritten-skip-restart"),
    };
    // After a flush that could not write them all: no Write, only on without them, or stay.
    let title = tr_args(if app.confirm_failed { "unwritten-failed-title" } else { "unwritten-title" }, &[("count", app.unwritten_count().into())]);
    let mut rows = vec![SkiaShape::new().fill_x().padding((10, 6)).accessibility_role(Aria::PRESENTATION).children(text(12.0, MUTED).text(title.clone()))];
    // Why, the first few (each on up to three lines).
    let shown = if app.confirm_failed { &app.failed_reasons[..app.failed_reasons.len().min(3)] } else { &[] };
    for reason in shown {
        rows.push(SkiaShape::new().fill_x().padding((10, 2, 10, 4)).accessibility_role(Aria::PRESENTATION).children(text(11.0, ERROR).fill_x().max_lines(3).text(format!("• {reason}"))));
    }
    if app.confirm_failed && app.failed_reasons.len() > shown.len() {
        let more = tr_args("unwritten-more-reasons", &[("count", (app.failed_reasons.len() - shown.len()).into())]);
        rows.push(SkiaShape::new().fill_x().padding((10, 2, 10, 4)).accessibility_role(Aria::PRESENTATION).children(text(11.0, MUTED).text(more)));
    }
    if !app.confirm_failed {
        rows.push(menu_item(tr(write), |_| false, |app, cx| app.answer_unwritten(true, cx)));
    }
    let rows = rows.into_iter().chain([
        menu_item(tr(skip), |_| false, |app, cx| app.answer_unwritten(false, cx)),
        menu_item(tr("unwritten-cancel"), |_| false, |app, cx| {
            app.confirm = None;
            app.close_popup(cx);
        }),
    ]).collect();
    // The reasons wrap at about 60 characters.
    let wide = if shown.is_empty() { 0 } else { 60 };
    let longest = [title.chars().count(), tr(write).chars().count(), tr(skip).chars().count(), wide].into_iter().max().unwrap_or(20);
    let label = std::iter::once(title).chain(shown.iter().cloned()).collect::<Vec<_>>().join(". ");
    menu_panel(rows, menu_width(longest)).accessibility_label(label)
}

/// A run failure's first line, and when it only leads into the build's error lines ("the build
/// failed:"), the first of them with it.
pub(crate) fn first_reason(message: &str) -> String {
    let mut lines = message.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = lines.next().unwrap_or_default();
    match first.strip_suffix(':') {
        Some(head) => lines.next().map_or_else(|| head.to_owned(), |next| format!("{head}: {next}")),
        None => first.to_owned(),
    }
}

/// The bottom line: the link to the hub and how many apps are connected.
fn status_bar() -> Build<SkiaLayout> {
    SkiaLayout::new().fill().background_color(c(HEADER)).padding((12, 0)).children(
        SkiaRow::new().spacing(6).vertical_options(LayoutOptions::Center).children((
            SkiaShape::new()
                .shape_type(ShapeType::Circle)
                .width_request(7)
                .height_request(7)
                .vertical_options(LayoutOptions::Center)
                .observe(|me, app: &App| me.set_background_color(c(if app.connected { OK } else { WARN }))),
            text(11.0, MUTED).accessibility_role(Aria::STATUS).accessibility_live(Aria::LIVE_POLITE).observe(|me, app: &App| {
                me.set_text(if app.connected {
                    // Where apps can come from: Wi-Fi, and the phones on a cable.
                    let mut parts = vec![tr_args("status-connected", &[("count", app.apps.len().into())])];
                    parts.push(tr(if app.device_url.is_some() { "status-wifi-on" } else { "status-wifi-off" }));
                    let mut cables = vec![];
                    if app.cables.0 > 0 {
                        cables.push(format!("{} Android", app.cables.0));
                    }
                    if app.cables.1 > 0 {
                        cables.push(format!("{} iPhone", app.cables.1));
                    }
                    if !cables.is_empty() {
                        parts.push(tr_args("status-usb", &[("phones", cables.join(", ").into())]));
                    }
                    parts.join(" · ")
                } else {
                    tr("status-connecting")
                })
            }),
            // Joined the hub of another designer: said while connected to it.
            text(11.0, WARN).max_lines(1).accessibility_role(Aria::STATUS).observe(|me, app: &App| {
                let notice = app.other_hub.as_deref().filter(|_| app.connected).map_or(String::new(), |v| app.other_hub_text(v));
                me.set_text(if notice.is_empty() { String::new() } else { format!("·  {notice}") });
                me.set_tooltip(notice.as_str());
                me.set_accessibility_label(notice.as_str());
            }),
            // A run that failed says why (the build's first error line), until the next run.
            text(11.0, ERROR).max_lines(1).accessibility_role(Aria::STATUS).observe(|me, app: &App| {
                let failed = app.runs.iter().find(|(_, (state, _))| *state == designer_protocol::RunState::Failed);
                me.set_text(failed.map_or(String::new(), |(project, (_, message))| {
                    // The app's name (a Rust app's project is its Cargo.toml), else the project file's.
                    let name = app.recent.iter().find(|r| r.project == *project).map(|r| r.name.clone()).or_else(|| app.apps.iter().find(|a| a.info.project.as_deref() == Some(project.as_str())).map(|a| a.info.name.clone()));
                    let name = name.unwrap_or_else(|| std::path::Path::new(project).file_name().map_or(project.clone(), |n| n.to_string_lossy().into_owned()));
                    format!("·  {}", tr_args("status-run-failed", &[("name", name.into()), ("message", first_reason(message).into())]))
                }));
                // Every line of the failure in its tooltip and for a screen reader.
                let full = failed.map_or(String::new(), |(_, (_, message))| message.clone());
                me.set_tooltip(full.as_str());
                me.set_accessibility_label(full.as_str());
            }),
        )),
    )
}

#[derive(Default)]
pub struct RowHandles {
    root: Handle<SkiaLayout>,
    guides: Handle<Guides>,
    toggle: Handle<SkiaLayout>,
    content: Handle<SkiaLayout>,
    icon: Handle<SkiaSvg>,
    label: Handle<SkiaLabel>,
    detail: Handle<SkiaLabel>,
    preview: Handle<SkiaLabel>,
    /// A control added or taken out here: the dot (not in code yet).
    added: Handle<SkiaShape>,
}

/// A list row: an icon, a label and dimmer details. With `foldable`, tree lines run on the left
/// and a +/- box on a node with children folds or unfolds it.
/// A Layers or Screens row's background: selected, and the veil while the mouse is on it.
fn row_bg(selected: bool, hovered: bool) -> Color {
    veiled(if selected { c(SELECTION) } else { Color::TRANSPARENT }, hovered, false)
}

fn row_cell(foldable: bool) -> (Build<SkiaLayout>, RowHandles) {
    let mut h = RowHandles::default();
    let cell = SkiaLayout::new()
        .fill_x()
        .height_request(ROW)
        .padding((0, 0, 12, 0))
        .accessibility_role(if foldable { Aria::LIST_ITEM } else { Aria::BUTTON })
        .accessibility_can_interact(Some(true))
        .assign(&mut h.root)
        // Hover: the row is drawn again with the veil (the selection comes from the app state).
        .on_hovered(move |me, app: &mut App, _cx, over| {
            app.hover_changes = app.hover_changes.wrapping_add(1);
            let index = me.base().context_index;
            let selected = foldable && index.is_some_and(|i| app.selected == Some(i));
            me.set_background_color(row_bg(selected, over));
            // The Layers row under the mouse: its Undo and Write show at the list's right edge.
            if foldable {
                if over {
                    (app.hover_row, app.strip_row) = (index, index);
                } else if app.hover_row == index {
                    app.hover_row = None;
                }
            }
        })
        // A right click on a Layers row selects it and opens the Layers menu where it was.
        .on_context_menu(move |me, app: &mut App, cx, menu| {
            let Some(index) = me.base().context_index.filter(|_| foldable) else { return false };
            app.select_row(index, cx);
            let at = menu.location;
            app.open_layers_menu(drawnui::skia::Rect::new(at.x, at.y, at.x, at.y), cx);
            true
        })
        .children((
        Build::new(Guides::default()).fill().input_transparent(true).assign(&mut h.guides),
        // The +/- box's hit area, one level wide, moved onto the box when the row is bound.
        SkiaLayout::new()
            .width_request(if foldable { GUIDE_STEP } else { 0.0 })
            .height_request(ROW)
            .z_index(1)
            .assign(&mut h.toggle)
            .on_tapped(move |me, app: &mut App, cx| {
                if !left_click(cx) {
                    return;
                }
                // The row index is written into the hit area's tag when the row is bound.
                if foldable && let Ok(index) = me.base().p.tag.parse::<usize>() {
                    app.toggle_row(index, cx);
                }
            }),
        SkiaRow::new().spacing(6).vertical_options(LayoutOptions::Center).assign(&mut h.content).children((
            SkiaSvg::from_string(icons::BOX).width_request(14).height_request(14).vertical_options(LayoutOptions::Center).assign(&mut h.icon),
            // Layers (the foldable list) names its controls a little dimmer than the side panels' text.
            text(13.0, if foldable { LAYERS_TEXT } else { TEXT }).assign(&mut h.label),
            // Added here and not in code yet: the dot of a property changed live.
            SkiaShape::new().shape_type(ShapeType::Circle).width_request(6).height_request(6).vertical_options(LayoutOptions::Center).background_color(c(ACCENT)).is_visible(false).assign(&mut h.added),
            text(12.0, FIELD).assign(&mut h.detail),
            text(12.0, MUTED).assign(&mut h.preview),
        )),
    ));
    (cell, h)
}

/// Where a Layers row's icon starts at `depth`, points from the row's left.
pub(crate) fn row_text_left(depth: usize) -> f32 {
    GUIDE_LEFT + (depth as f32 + 1.5) * GUIDE_STEP - 7.0
}

/// A menu item's key, as the platform writes it: Ctrl+Shift+C, or ⇧⌘C on macOS.
fn shortcut_text(action: super::MenuAction) -> String {
    menubar::key_text(action.shortcut())
}

/// A Layers menu item's name.
fn action_name(action: super::MenuAction) -> String {
    use super::MenuAction as A;
    tr(match action {
        A::Undo => "menu-undo",
        A::Redo => "menu-redo",
        A::Copy => "menu-copy",
        A::Paste => "menu-paste",
        A::Duplicate => "menu-duplicate",
        A::CopyStyle => "menu-copy-style",
        A::PasteStyle => "menu-paste-style",
        A::Delete => "menu-delete",
    })
}

/// Undo and Redo with what they take back or do again ("Undo Delete SkiaShape").
pub(crate) fn action_label(app: &App, action: super::MenuAction) -> String {
    use super::MenuAction as A;
    match action {
        A::Undo => app.undo_label().map_or_else(|| tr("menu-undo"), |s| tr_args("menu-undo-step", &[("step", s.into())])),
        A::Redo => app.redo_label().map_or_else(|| tr("menu-redo"), |s| tr_args("menu-redo-step", &[("step", s.into())])),
        a => action_name(a),
    }
}

/// The Layers menu: Undo, Redo; Copy, Paste, Duplicate; Copy style, Paste style; Delete. Each with
/// its key; one that cannot act now reads disabled.
pub fn layers_menu(app: &App) -> Build<SkiaShape> {
    use super::MenuAction as A;
    let longest = A::ALL.iter().map(|&a| action_label(app, a).chars().count() + shortcut_text(a).chars().count()).max().unwrap_or(12);
    let mut rows = vec![];
    for a in A::ALL {
        // Groups: the history, the control, its style, taking it out.
        if matches!(a, A::Copy | A::CopyStyle | A::Delete) {
            rows.push(menu_separator());
        }
        rows.push(action_item(action_label(app, a), shortcut_text(a), app.action_enabled(a), a));
    }
    menu_panel(rows, menu_width(longest + 3)).accessibility_label(tr("layers-menu"))
}

/// A line between groups of a menu.
fn menu_separator() -> Build<SkiaShape> {
    SkiaShape::new().fill_x().height_request(1).margin((6, 2, 6, 2)).background_color(c(LINE)).accessibility_role(Aria::SEPARATOR)
}

/// An item of an action menu, styled as the other menus' rows: its name, its key at the right,
/// lit under the mouse; a disabled one is dimmed, takes no click and reads unavailable.
fn action_item(label: String, keys: String, enabled: bool, action: super::MenuAction) -> Build<SkiaShape> {
    let key = label.clone();
    let hovered = label.clone();
    SkiaShape::new()
        .corner_radius(4)
        .fill_x()
        .height_request(ROW)
        .padding((10, 0, 10, 0))
        .accessibility_role(Aria::MENU_ITEM)
        .accessibility_label(label.clone())
        .accessibility_hint(keys.clone())
        .accessibility_can_interact(Some(enabled))
        .on_hovered(move |_me, app: &mut App, _cx, on| {
            if on && enabled {
                app.menu_hover = Some(key.clone());
            } else if app.menu_hover.as_deref() == Some(key.as_str()) {
                app.menu_hover = None;
            }
        })
        .on_tapped(move |_me, app: &mut App, cx| {
            if enabled && left_click(cx) {
                app.menu_action(action, cx);
            }
        })
        .observe(move |me, app: &App| {
            let hover = app.menu_hover.as_deref() == Some(hovered.as_str());
            me.set_background_color(if hover { c(MENU_HOVER) } else { Color::TRANSPARENT });
        })
        .children((
            text(12.0, if enabled { TEXT } else { FAINT }).text(label).vertical_options(LayoutOptions::Center),
            text(11.0, FAINT).text(keys).horizontal_options(LayoutOptions::End).vertical_options(LayoutOptions::Center).accessibility_role(Aria::PRESENTATION),
        ))
}

/// The Layers row a strip of Undo and Write is for, and its control: the selected row, or
/// (`hovered`) the row under the mouse when it is another one. Only a row added or taken out here.
fn strip_target(app: &App, hovered: bool) -> Option<(usize, u64)> {
    let row = if hovered { app.strip_row.filter(|r| (app.hover_row.is_some() || app.strip_hovered) && Some(*r) != app.selected)? } else { app.selected? };
    let id = app.tree.nodes[app.rows.get(row)?.node].id;
    app.row_change(id).map(|_| (row, id))
}

/// Undo (`write` false) or Write of a control added or taken out here, named for it
/// ("Write SkiaLabel to code"); `target` says which control, None hides it.
fn change_button(write: bool, target: impl Fn(&App) -> Option<u64> + Clone + 'static) -> Build<SkiaShape> {
    let (show, tap) = (target.clone(), target);
    interactive(SkiaShape::new().corner_radius(4).width_request(26).height_request(22).vertical_options(LayoutOptions::Center).is_visible(false), |_, _| Color::TRANSPARENT)
        .accessibility_role(Aria::BUTTON)
        .observe(move |me, app: &App| {
            let id = show(app).filter(|id| !write || app.row_change(*id).is_some_and(|(_, written)| !written));
            me.set_is_visible(id.is_some());
            if let Some(node) = id.and_then(|id| app.tree.nodes.iter().find(|n| n.id == id)) {
                me.set_accessibility_label(tr_args(if write { "write-prop" } else { "undo-prop" }, &[("name", short_type(&node.type_name).to_owned().into())]));
            }
        })
        .on_tapped(move |_me, app: &mut App, cx| {
            if left_click(cx)
                && let Some(id) = tap(app)
            {
                if write { app.write_row(id, cx) } else { app.undo_row(id, cx) }
            }
        })
        .children(SkiaSvg::from_string(if write { icons::CODE } else { icons::UNDO_2 }).width_request(14).height_request(14).center().tint_color(c(if write { ACCENT } else { MUTED })))
}

/// Undo and Write of a Layers row, laid over the row's end at the right edge of the visible list,
/// on the row's own background: they stay there while the list scrolls sideways.
fn row_strip(hovered: bool) -> Build<SkiaShape> {
    let target = move |app: &App| strip_target(app, hovered).map(|t| t.1);
    SkiaShape::new()
        .height_request(ROW)
        .padding((6, 0, 2, 0))
        .horizontal_options(LayoutOptions::End)
        .vertical_options(LayoutOptions::Start)
        .is_visible(false)
        .on_hovered(move |_me, app: &mut App, _cx, over| {
            if hovered {
                app.strip_hovered = over;
            }
        })
        .observe(move |me, app: &App| {
            let row = strip_target(app, hovered).map(|t| t.0);
            // The offset is 0 at the start and negative when scrolled down.
            let top = PROPS_PAD + row.unwrap_or(0) as f32 * ROW + app.layers_offset;
            me.set_is_visible(row.is_some() && top > -ROW / 2.0);
            me.set_margin(Thickness::new(0.0, top, BAR_LANE, 0.0));
            let base = if hovered { c(PANEL) } else { c(SELECTION) };
            me.set_background_color(veiled(base, hovered || app.hover_row == app.selected, false));
        })
        .children(SkiaRow::new().spacing(2).vertical_options(LayoutOptions::Center).children((change_button(false, target), change_button(true, target))))
}

/// Properties' header: the selected control's own Undo and Write when it was added or taken out
/// here, so they are there whatever the panels' widths.
fn control_actions() -> Build<SkiaLayout> {
    let target = |app: &App| app.selected_node().map(|n| n.id).filter(|id| app.row_change(*id).is_some());
    SkiaRow::new().spacing(2).vertical_options(LayoutOptions::Center).children((change_button(false, target), change_button(true, target)))
}

/// Properties' Write all: shown while the selected control has more than one live edit; writes
/// each of them to code.
fn write_all_button() -> Build<SkiaShape> {
    interactive(
        SkiaShape::new()
            .corner_radius(6)
            .stroke_width(1)
            .stroke_color(c(ACCENT))
            .padding((8, 3))
            .vertical_options(LayoutOptions::Center)
            .accessibility_role(Aria::BUTTON)
            .is_visible(false)
            .observe(|me, app: &App| {
                let count = app.pending_count();
                me.set_is_visible(count > 1);
                me.set_accessibility_label(tr_args("write-all-label", &[("count", count.into())]));
            })
            .on_tapped(|_me, app: &mut App, cx| {
                if left_click(cx) {
                    app.write_all(cx);
                }
            }),
        |_, _| c(PANEL),
    )
    .children(SkiaRow::new().spacing(5).children((icon(icons::CODE, 13.0, ACCENT), bold(11.0, TEXT).text(tr("write-all")))))
}

/// Where the tree lines start, and how wide one level is.
const GUIDE_LEFT: f32 = 6.0;
const GUIDE_STEP: f32 = 20.0;

/// Sets a row's icon and its color, and its place in the tree: `tree` is None for a flat list row.
fn cell_icon(cx: &mut Cx, cell: &RowHandles, svg: &'static str, color: Tone, tree: Option<(&super::Row, Option<bool>)>) {
    if let Some(mut me) = cx.get_mut(cell.icon) {
        me.set_svg_string(svg);
        me.set_tint_color(c(color));
    }
    // The icon sits centered on the next level's line, so a child's line hangs from it.
    let left = match tree {
        Some((row, _)) => GUIDE_LEFT + (row.depth as f32 + 1.5) * GUIDE_STEP - 7.0,
        None => 12.0,
    };
    if let Some(mut me) = cx.get_mut(cell.content) {
        me.set_margin(Thickness::new(left, 0.0, 0.0, 0.0));
    }
    if let Some(mut me) = cx.get_mut(cell.guides) {
        let g = me.control_mut();
        *g = match tree {
            Some((row, fold)) => Guides { shown: true, depth: row.depth, lines: row.lines, last: row.last, fold },
            None => Guides::default(),
        };
        me.mark(Dirty::DRAW);
    }
}

/// The tree lines of one row: a vertical line for every ancestor with siblings below, an elbow to
/// the row's own icon, and a +/- box on a node with children. Paints only; layout is the row's.
#[derive(Default)]
pub struct Guides {
    shown: bool,
    depth: usize,
    lines: u64,
    last: bool,
    /// None: no children; Some(true): folded; Some(false): open.
    fold: Option<bool>,
}

impl Control for Guides {
    fn paint(&self, cx: &mut PaintCx) {
        if !self.shown {
            return;
        }
        use drawnui::skia::{Paint, PaintStyle, Rect as SkRect};
        let (r, s) = (cx.rect, cx.scale);
        // Crisp 1 px lines: on pixel centers.
        let x_of = |level: f32| (r.left + (GUIDE_LEFT + (level + 0.5) * GUIDE_STEP) * s).round() + 0.5;
        // Rows meet at fractional pixels at 125 % and similar scales: overlap by one pixel so the
        // lines run on without gaps.
        let (top, bottom) = (r.top.floor(), r.bottom.ceil() + 1.0);
        let mid = ((r.top + r.bottom) / 2.0).round() + 0.5;
        let mut line = Paint::default();
        line.set_color(sk(GUIDE));
        line.set_stroke_width(1.0);
        line.set_style(PaintStyle::Stroke);
        let canvas = cx.canvas;
        for k in 0..self.depth.min(63) {
            if self.lines & (1 << k) != 0 {
                let x = x_of(k as f32);
                canvas.draw_line((x, top), (x, bottom), &line);
            }
        }
        let d = self.depth as f32;
        let x = x_of(d);
        canvas.draw_line((x, top), (x, if self.last { mid } else { bottom }), &line);
        canvas.draw_line((x, mid), (x_of(d + 1.0) - 7.0 * s, mid), &line);
        if self.fold == Some(false) {
            // An open node's children hang from under its icon.
            let xc = x_of(d + 1.0);
            canvas.draw_line((xc, mid + 8.0 * s), (xc, bottom), &line);
        }
        if let Some(folded) = self.fold {
            let half = (4.5 * s).round();
            let bx = SkRect::new(x - half, mid - half, x + half, mid + half);
            let mut fill = Paint::default();
            fill.set_color(sk(PANEL));
            canvas.draw_rect(bx, &fill);
            let mut frame = line.clone();
            frame.set_color(sk(GUIDE_BOX));
            canvas.draw_rect(bx, &frame);
            let mut sign = line.clone();
            sign.set_color(sk(GUIDE_SIGN));
            let arm = (2.5 * s).round();
            canvas.draw_line((x - arm, mid), (x + arm + 1.0, mid), &sign);
            if folded {
                canvas.draw_line((x, mid - arm), (x, mid + arm + 1.0), &sign);
            }
        }
    }
}

#[derive(Default)]
pub struct PropHandles {
    root: Handle<SkiaLayout>,
    dot: Handle<SkiaShape>,
    name: Handle<SkiaLabel>,
    display: Handle<SkiaLayout>,
    value: Handle<SkiaLabel>,
    swatch: Handle<SkiaShape>,
    undo: Handle<SkiaShape>,
    code: Handle<SkiaShape>,
    code_icon: Handle<SkiaSvg>,
}

/// A small - / + button inside the edited row; for numbers only.
fn row_stepper(caption: &str, steps: f64) -> Build<SkiaShape> {
    interactive(
        SkiaShape::new()
            .corner_radius(4)
            .width_request(22)
            .height_request(20)
            .vertical_options(LayoutOptions::Center)
            .accessibility_role(Aria::BUTTON)
            .accessibility_label(tr(if steps < 0.0 { "decrease" } else { "increase" }))
            .on_tapped(move |_me, app: &mut App, cx| if left_click(cx) { app.step_edit_value(steps, cx) }),
        |_, _| c(HOVER),
    )
        .children(bold(13.0, TEXT).text(caption).center())
}

/// A properties row, edited where it is (the list never moves): a dot when the value was changed
/// in the app but not written to code, the name, the value (a field while edited, a dropdown for
/// choices), and the code button that writes the value into the app's code.
fn prop_cell() -> (Build<SkiaLayout>, PropHandles) {
    let mut h = PropHandles::default();
    let cell = SkiaLayout::new()
        .fill_x()
        .height_request(ROW)
        .assign(&mut h.root)
        .on_hovered(|me, app: &mut App, cx, on| {
            let Some(index) = me.base().context_index else { return };
            let old = app.hover_prop;
            app.hover_prop = if on { Some(index) } else { old.filter(|&i| i != index) };
            for i in [old, app.hover_prop].into_iter().flatten() {
                cx.items_changed(app.props_list, i);
            }
        })
        .on_tapped(|me, app: &mut App, cx| {
            if !left_click(cx) {
                return;
            }
            let Some(index) = me.base().context_index else { return };
            // A dropdown opens under the value, not at the row's left edge: the row's rect, in
            // points, from where the value column starts.
            let s = me.base().scale.max(0.1);
            let r = me.base().rect; // layout pixels: the list's scroll is not applied yet
            let scrolled = cx.find::<SkiaScroll>(app.props_scroll).map_or(0.0, |sc| sc.viewport_offset_y());
            // 0 at the start, negative when scrolled down.
            let top = r.top / s + scrolled;
            let anchor = Some(drawnui::skia::Rect::new(r.left / s + 148.0, top, r.right / s - ACTIONS, top + r.height() / s));
            let tap = cx.gesture().map(|g| drawnui::skia::Point::new(g.location.x / s, g.location.y / s));
            match app.prop_rows.get(index).cloned() {
                Some(PropRow::Prop(i)) => app.edit_prop_at(i, anchor, tap, cx),
                Some(PropRow::Header { .. }) => app.tap_prop_row(index, cx),
                None => {}
            }
        })
        .children((
            SkiaShape::new()
                .shape_type(ShapeType::Circle)
                .width_request(6)
                .height_request(6)
                .margin((4, 0, 0, 0))
                .vertical_options(LayoutOptions::Center)
                .background_color(c(ACCENT))
                .assign(&mut h.dot),
            text(LIST_TEXT, MUTED).width_request(140).margin((12, 0, 0, 0)).vertical_options(LayoutOptions::Center).assign(&mut h.name),
            // The value as it reads.
            SkiaRow::new().spacing(6).margin((152.0, 0.0, ACTIONS, 0.0)).vertical_options(LayoutOptions::Center).assign(&mut h.display).children((
                SkiaShape::new()
                    .corner_radius(3)
                    .width_request(12)
                    .height_request(12)
                    .stroke_color(c(LINE))
                    .stroke_width(1)
                    .vertical_options(LayoutOptions::Center)
                    .assign(&mut h.swatch),
                text(12.0, TEXT).assign(&mut h.value),
            )),
            // Undo, then Write: the live edit back, or the newest write to code taken back.
            SkiaRow::new().spacing(2).margin((0.0, 0.0, BAR_LANE, 0.0)).horizontal_options(LayoutOptions::End).children((
                interactive(SkiaShape::new().corner_radius(4).width_request(26).height_request(22).vertical_options(LayoutOptions::Center), |_, _| Color::TRANSPARENT)
                    .accessibility_role(Aria::BUTTON)
                    .assign(&mut h.undo)
                    .on_tapped(|me, app: &mut App, cx| {
                        if !left_click(cx) {
                            return;
                        }
                        let Ok(index) = me.base().p.tag.parse::<usize>() else { return };
                        app.undo_prop(index, cx);
                    })
                    .children(icon(icons::UNDO_2, 14.0, MUTED).center()),
                // Writes the value into the app's code.
                interactive(SkiaShape::new().corner_radius(4).width_request(26).height_request(22).vertical_options(LayoutOptions::Center), |_, _| Color::TRANSPARENT)
                    .accessibility_role(Aria::BUTTON)
                    .assign(&mut h.code)
                    .on_tapped(|me, app: &mut App, cx| {
                        if !left_click(cx) {
                            return;
                        }
                        let Ok(index) = me.base().p.tag.parse::<usize>() else { return };
                        app.write_prop(index, cx);
                    })
                    .children(SkiaSvg::from_string(icons::CODE).width_request(14).height_request(14).center().assign(&mut h.code_icon)),
            )),
        ));
    (cell, h)
}

// How many rows were bound (tests check an edit does not rebind the list over and over).
// Per thread: tests run side by side, each on its own thread.
#[cfg(test)]
thread_local! {
    pub static PROP_BINDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn bind_prop(cell: &PropHandles, app: &App, index: usize, cx: &mut Cx) {
    #[cfg(test)]
    PROP_BINDS.with(|b| b.set(b.get() + 1));
    // The edit field lies over an edited row's value: the row's own value hides under it.
    let show = |cx: &mut Cx, editing_text: bool, header: bool| {
        if let Some(mut me) = cx.get_mut(cell.display) {
            me.set_is_visible(!editing_text && !header);
        }
    };
    match app.prop_rows.get(index) {
        Some(PropRow::Header { group, count }) => {
            let open = app.group_open(group);
            set(cx, cell.name, format!("{}  {}  ({count})", if open { "\u{2212}" } else { "+" }, group_title(group).to_uppercase()));
            if let Some(mut me) = cx.get_mut(cell.name) {
                me.set_font_family("Semibold");
                me.set_text_color(c(MUTED));
                me.set_width_request(260.0);
            }
            show(cx, false, true);
            for h in [cell.undo, cell.code] {
                if let Some(mut me) = cx.get_mut(h) {
                    me.set_is_visible(false);
                }
            }
            if let Some(mut me) = cx.get_mut(cell.dot) {
                me.set_is_visible(false);
            }
            if let Some(mut me) = cx.get_mut(cell.root) {
                me.set_background_color(veiled(c(GROUP_HEADER), app.hover_prop == Some(index), false));
                me.set_accessibility_role(Aria::BUTTON);
                me.set_accessibility_label(tr_args("group-row", &[("group", group_title(group).into()), ("count", (*count).into()), ("state", tr(if open { "group-open" } else { "group-folded" }).into())]));
            }
        }
        Some(PropRow::Prop(i)) => {
            let Some(p) = app.props.get(*i) else { return };
            let node = app.selected_node().map_or(0, |n| n.id);
            let pending = app.pending.get(&(node, p.name.clone()));
            let editing = app.edit_prop.as_deref() == Some(p.name.as_str());
            let editing_text = editing && app.edit_choices.is_empty() && app.picker.is_none();
            let hovered = app.hover_prop == Some(index);
            set(cx, cell.name, p.name.clone());
            if let Some(mut me) = cx.get_mut(cell.name) {
                me.set_font_family("Default");
                // Changed properties read brighter than the ones at their default.
                me.set_text_color(c(if app.prop_changed(*i) { TEXT } else { MUTED }));
                me.set_width_request(140.0);
            }
            let shown = if editing { app.edit_value.clone() } else { pending.cloned().unwrap_or_else(|| p.value.clone()) };
            // A gradient reads as its type and number of colors, with a strip of it.
            let grad = p.type_name.ends_with(".SkiaGradient").then(|| super::gradient::Grad::parse(&shown)).flatten();
            let label = match &grad {
                Some(g) => g.summary(),
                None if p.type_name.ends_with(".SkiaGradient") => tr("gradient-none"),
                // An optional choice that is cleared.
                None if shown.is_empty() && p.choices.as_ref().is_some_and(|c| c.iter().any(String::is_empty)) => tr("choice-empty"),
                None => shown.clone(),
            };
            set(cx, cell.value, label);
            if let Some(mut me) = cx.get_mut(cell.value) {
                me.set_text_color(c(if p.is_set || pending.is_some() { TEXT } else { FAINT }));
            }
            let color = p.type_name.ends_with(".Color").then(|| parse_argb(&shown)).flatten();
            if let Some(mut me) = cx.get_mut(cell.swatch) {
                me.set_is_visible(color.is_some() || grad.is_some());
                me.set_width_request(if grad.is_some() { 34.0 } else { 12.0 });
                me.set_fill_gradient(grad.as_ref().map(|g| Box::new(g.strip())));
                if let Some(c) = color {
                    me.set_background_color(c);
                }
            }
            show(cx, editing_text, false);
            if let Some(mut me) = cx.get_mut(cell.dot) {
                me.set_is_visible(pending.is_some());
            }
            if let Some(mut me) = cx.get_mut(cell.code) {
                me.set_tag(i.to_string());
                me.set_is_visible(pending.is_some() || hovered || editing);
            }
            if let Some(mut me) = cx.get_mut(cell.undo) {
                me.set_tag(i.to_string());
                me.set_is_visible(app.can_undo(*i));
            }
            if let Some(mut me) = cx.get_mut(cell.code_icon) {
                me.set_tint_color(c(if pending.is_some() { ACCENT } else { MUTED }));
            }
            if let Some(mut me) = cx.get_mut(cell.root) {
                me.set_background_color(if editing { c(SELECTION) } else { veiled(Color::TRANSPARENT, hovered, false) });
                me.set_accessibility_role(Aria::LIST_ITEM);
                me.set_accessibility_can_interact(Some(true));
                let state = if pending.is_some() { format!(", {}", tr("prop-changed")) } else if p.is_set { format!(", {}", tr("prop-set")) } else { String::new() };
                me.set_accessibility_label(format!("{}, {shown}{state}", p.name));
            }
            if let Some(mut me) = cx.get_mut(cell.undo) {
                me.set_accessibility_label(tr_args("undo-prop", &[("name", p.name.clone().into())]));
            }
            if let Some(mut me) = cx.get_mut(cell.code) {
                me.set_accessibility_label(tr_args("write-prop", &[("name", p.name.clone().into())]));
            }
        }
        None => {}
    }
}

/// `#AARRGGBB` / `#RRGGBB` as the agent sends colors.
fn parse_argb(value: &str) -> Option<Color> {
    let hex_digits = value.trim().trim_start_matches('#');
    let v = u32::from_str_radix(hex_digits, 16).ok()?;
    match hex_digits.len() {
        8 => Some(Color::new(v)),
        6 => Some(Color::new(0xFF00_0000 | v)),
        _ => None,
    }
}

fn set(cx: &mut Cx, label: Handle<SkiaLabel>, value: impl Into<String>) {
    if let Some(mut me) = cx.get_mut(label) {
        me.set_text(value.into());
    }
}

/// Which panel a divider resizes.
#[derive(Clone, Copy)]
enum Side {
    Screens,
    Properties,
    /// The line between Screens and Controls (it moves up and down).
    Controls,
}

/// A 1 px line with a 9 pt grab area: dragging it resizes the panel beside it; the line turns
/// accent blue while the mouse is over it or a drag is on.
fn divider(side: Side) -> Build<SkiaLayout> {
    let mut line = Handle::<SkiaShape>::default();
    let across = matches!(side, Side::Controls);
    let layer = if across { SkiaLayer::new().height_request(9).fill_x().margin((0, -4, 0, -4)) } else { SkiaLayer::new().width_request(9).fill_y().margin((-4, 0, -4, 0)) };
    let rule = if across { SkiaShape::new().height_request(1).fill_x().vertical_options(LayoutOptions::Center) } else { SkiaShape::new().width_request(1).fill_y().horizontal_options(LayoutOptions::Center) };
    layer.z_index(1).children((
        rule.background_color(c(LINE)).assign(&mut line),
        Build::new(Grip(if across { Cursor::ResizeUpDown } else { Cursor::ResizeLeftRight }))
            .fill()
            .accessibility_role(Aria::SEPARATOR)
            .accessibility_label(match side {
                Side::Screens => tr("resize-screens"),
                Side::Properties => tr("resize-properties"),
                Side::Controls => tr("resize-controls"),
            })
            .on_hovered(move |_me, _app: &mut App, cx, over| {
                if let Some(mut l) = cx.get_mut(line) {
                    l.set_background_color(c(if over { ACCENT } else { LINE }));
                }
            })
            .consume_gestures(move |me, app: &mut App, cx, gesture| {
                let scale = me.base().scale.max(0.1);
                match gesture.kind {
                    GestureKind::Down => true,
                    GestureKind::Panning => {
                        let dx = gesture.delta.x / scale;
                        match side {
                            Side::Screens => app.screens_w = (app.screens_width() + dx).clamp(120.0, 480.0),
                            Side::Properties => app.properties_w = (app.properties_width() - dx).clamp(260.0, 720.0),
                            Side::Controls => {
                                // The share of the column's height Screens takes.
                                let height = cx.base(app.left_column).map_or(0.0, |b| b.rect.height());
                                if height > 0.0 {
                                    let now = if app.screens_share > 0.0 { app.screens_share } else { 0.5 };
                                    app.screens_share = (now + gesture.delta.y / height).clamp(0.15, 0.85);
                                }
                            }
                        }
                        true
                    }
                    GestureKind::Up => {
                        match side {
                            Side::Controls => app.save_screens_share(),
                            _ => app.save_panel_widths(),
                        }
                        true
                    }
                    _ => false,
                }
            }),
    ))
}

/// A divider's grab area: draws nothing, shows its resize cursor over its whole area (a drag
/// handler alone gives no cursor).
struct Grip(Cursor);

impl Control for Grip {
    fn cursor(&self, _local: drawnui::skia::Point) -> Option<Cursor> {
        Some(self.0)
    }
}
