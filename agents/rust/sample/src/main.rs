//! A small DrawnUI for Rust app with the DrawnUI Designer agent: run it in debug, start the
//! designer, and its tree, properties and live edits show there.

// Release builds are a plain Windows app with no console window; debug builds keep the console.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use drawnui::Aria;
use drawnui::prelude::*;

const BACKGROUND: Color = Color::from_rgb(18, 20, 28);
const TEXT: Color = Color::from_rgb(236, 239, 245);
const MUTED: Color = Color::from_rgb(150, 160, 180);
const CARD: Color = Color::from_rgb(34, 38, 52);
const ACCENT: Color = Color::from_rgb(64, 132, 246);
const CORAL: Color = Color::from_rgb(242, 104, 84);
const QUIET: Color = Color::from_rgb(62, 70, 92);

#[derive(Default)]
struct App {
    count: i32,
    /// The quick-actions menu while it is open.
    menu: Option<PopupId>,
    menu_switch: Handle<SkiaSwitch>,
}

/// One line of the quick-actions menu: highlighted under the mouse; a tap runs it and closes the menu.
fn menu_item(text: &'static str, run: fn(&mut App)) -> Build<SkiaShape> {
    SkiaShape::new()
        .corner_radius(6)
        .fill_x()
        .padding((12, 7, 12, 7))
        .accessibility_role(Aria::MENU_ITEM)
        .accessibility_label(text)
        .on_hovered(|me, _app: &mut App, _cx, on| me.set_background_color(if on { QUIET } else { Color::TRANSPARENT }))
        .on_tapped(move |_me, app: &mut App, cx| {
            run(app);
            if let Some(menu) = app.menu.take() {
                cx.close_popup_window(menu);
            }
        })
        .children(SkiaLabel::new(text).font_size(15).text_color(TEXT))
}

fn quick_actions() -> Build<SkiaShape> {
    SkiaShape::new()
        .corner_radius(8)
        .background_color(CARD)
        .stroke_color(Color::from_rgb(70, 80, 108))
        .stroke_width(1)
        .width_request(180)
        .padding(4)
        .tag("menu")
        .accessibility_role(Aria::MENU)
        .accessibility_label("Quick actions")
        .children(SkiaLayout::column().fill_x().children((
            menu_item("Add five", |app| app.count += 5),
            menu_item("Reset", |app| app.count = 0),
        )))
}

/// A color with 12 % white mixed in: the hover look of a filled button.
fn lift(c: Color) -> Color {
    let up = |v: u8| v + ((255 - v) as u32 * 12 / 100) as u8;
    Color::from_rgb(up(c.r()), up(c.g()), up(c.b()))
}

/// Every button of the app: its own color, lifted while the mouse is over it.
fn button(text: &str, color: Color, hint: &str) -> Build<SkiaButton> {
    SkiaButton::new(text)
        .background_color(color)
        .text_color(Color::WHITE)
        .accessibility_hint(hint)
        .on_hovered(move |me, _app: &mut App, _cx, on| me.set_background_color(if on { lift(color) } else { color }))
}

fn swatch(color: Color, name: &str) -> Build<SkiaShape> {
    SkiaShape::new().corner_radius(10).width_request(44).height_request(44).background_color(color).tag(name)
}

fn build(app: &mut App) -> Build<SkiaLayout> {
    SkiaLayout::column().spacing(18).padding(28).children((
        SkiaLabel::new("Designer sample").font_size(28).text_color(TEXT).tag("title").accessibility_role(Aria::HEADING),
        SkiaLabel::new("Open DrawnUI Designer: this window's controls show there, and edits apply live.").font_size(15).text_color(MUTED).accessibility_role(Aria::TEXT),
        SkiaLayout::row().spacing(10).tag("swatches").children((swatch(ACCENT, "blue"), swatch(CORAL, "coral"), swatch(Color::from_rgb(76, 186, 120), "green"))),
        SkiaShape::new()
            .corner_radius(16)
            .padding(20)
            .background_color(CARD)
            .stroke_color(Color::from_rgb(70, 80, 108))
            .stroke_width(1)
            .tag("card")
            .children((SkiaLayout::column().spacing(6).children((
                SkiaLabel::new("Taps").font_size(13).text_color(MUTED),
                SkiaLabel::new("0")
                    .font_size(40)
                    .text_color(CORAL)
                    .tag("count")
                    .accessibility_role(Aria::STATUS)
                    .accessibility_live(Aria::LIVE_POLITE)
                    .observe(|me, app: &App| {
                        me.set_text(app.count.to_string());
                        me.set_accessibility_label(format!("Taps: {}", app.count));
                    }),
            )),)),
        SkiaLayout::row().spacing(12).accessibility_role(Aria::TOOLBAR).accessibility_label("Taps").children((
            button("Add one", ACCENT, "Adds one to the taps").on_tapped(|_me, app: &mut App, _cx| app.count += 1),
            button("Reset", QUIET, "Sets the taps back to zero").on_tapped(|_me, app: &mut App, _cx| app.count = 0),
        )),
        // The switch shows whether the menu is open; turned on (by a tap or from the designer) it opens it.
        SkiaLayout::row().spacing(12).children((
            SkiaSwitch::new()
                .assign(&mut app.menu_switch)
                .tag("menu switch")
                .accessibility_label("Quick actions")
                .vertical_options(LayoutOptions::Center)
                .color_frame_on(ACCENT)
                .color_frame_off(QUIET)
                .on_toggled(|_me, app: &mut App, cx, on| match (on, app.menu) {
                    (true, None) => app.menu = Some(cx.open_popup_window(quick_actions(), app.menu_switch, PopupWindowOptions { cpu: true, ..Default::default() })),
                    (false, Some(menu)) => {
                        app.menu = None;
                        cx.close_popup_window(menu);
                    }
                    _ => {}
                })
                .observe(|me, app: &App| me.set_is_toggled(app.menu.is_some())),
            SkiaLabel::new("Quick actions").font_size(15).text_color(MUTED).vertical_options(LayoutOptions::Center).accessibility_role(Aria::PRESENTATION),
        )),
    ))
}

fn main() {
    drawnui::run("Designer sample", || {
        drawnui_designer_agent::attach!(
            Ui::new(App::default(), build)
                .font_bytes("Default", include_bytes!("../../../../crates/designer-desktop/assets/OpenSans-Regular.ttf"))
                .default_accessibility_role::<SkiaButton>(Aria::BUTTON)
                .background(BACKGROUND)
            .on_popup_window_closed(|app: &mut App, id, _cx| if app.menu == Some(id) { app.menu = None })
        )
    });
}
