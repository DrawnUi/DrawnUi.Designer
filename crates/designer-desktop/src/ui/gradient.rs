//! The gradient editor: a gradient property (DrawnUI SkiaGradient, sent by the agent as JSON) opens a
//! panel with its type, a bar with its colors as stops, its angle and the color picker for the
//! selected stop. Every change is set live in the app; Esc puts the old gradient back.

use drawnui::prelude::*;
use drawnui::skia::{Color4f, Paint, PaintStyle, Point, RRect, Rect, TileMode, gradient as sk};
use serde::{Deserialize, Serialize};

use super::color::{self, Hsva};
use super::view::{ACCENT, HEADER, LINE, MUTED, TEXT, c, segment, text};
use super::{App, PropRow};
use crate::i18n::{tr, tr_args};

/// The gradient types DrawnUI draws, in its own order (Conical draws as Linear).
pub const TYPES: [&str; 5] = ["Linear", "Circular", "Oval", "Sweep", "Conical"];

/// A gradient as the agent sends it (`DrawnUi.Draw.SkiaGradient` properties, JSON).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grad {
    #[serde(rename = "type")]
    pub kind: String,
    /// `#AARRGGBB` each.
    pub colors: Vec<String>,
    /// 0..1 per color; None = spread evenly.
    #[serde(default)]
    pub stops: Option<Vec<f32>>,
    /// Degrees (DrawnUI SkiaGradient.Angle: 0 = top to bottom, 90 = left to right); None = the start
    /// and end ratios decide.
    #[serde(default)]
    pub angle: Option<f32>,
    #[serde(default = "start")]
    pub start: [f32; 2],
    #[serde(default = "end")]
    pub end: [f32; 2],
    #[serde(default = "one")]
    pub opacity: f32,
}

fn start() -> [f32; 2] {
    [0.0, 0.0]
}
fn end() -> [f32; 2] {
    [0.0, 1.0]
}
fn one() -> f32 {
    1.0
}

impl Grad {
    /// The value of a gradient property; None for "" (no gradient) or text that is not one.
    pub fn parse(value: &str) -> Option<Grad> {
        serde_json::from_str::<Grad>(value).ok().filter(|g| !g.colors.is_empty())
    }

    /// What a property with no gradient starts from: left to right, the accent to violet.
    pub fn new_default() -> Grad {
        Grad { kind: "Linear".into(), colors: vec!["#FF4C8DFF".into(), "#FF8E5CFF".into()], stops: None, angle: Some(90.0), start: [0.0, 0.5], end: [1.0, 0.5], opacity: 1.0 }
    }

    pub fn json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Where each color sits, 0..1.
    pub fn positions(&self) -> Vec<f32> {
        match &self.stops {
            Some(s) if s.len() == self.colors.len() => s.clone(),
            _ if self.colors.len() < 2 => vec![0.0; self.colors.len()],
            _ => (0..self.colors.len()).map(|i| i as f32 / (self.colors.len() - 1) as f32).collect(),
        }
    }

    fn color(&self, i: usize) -> Hsva {
        self.colors.get(i).and_then(|v| Hsva::parse(v)).unwrap_or(Hsva { h: 0.0, s: 0.0, v: 0.0, a: 1.0 })
    }

    /// The colors left to right, for a strip or the bar.
    pub fn strip(&self) -> SkiaGradient {
        SkiaGradient {
            gradient_type: GradientType::Linear,
            colors: self.colors.iter().map(|v| Color::new(Hsva::parse(v).map_or(0, Hsva::argb))).collect(),
            color_positions: self.positions(),
            start_x_ratio: 0.0,
            start_y_ratio: 0.5,
            end_x_ratio: 1.0,
            end_y_ratio: 0.5,
            ..Default::default()
        }
    }

    /// "Linear · 3 colors" for the property's row.
    pub fn summary(&self) -> String {
        tr_args("gradient-summary", &[("type", self.kind.clone().into()), ("count", (self.colors.len() as i64).into())])
    }

    /// The angle in degrees: the one set, else the direction from start to end.
    pub fn degrees(&self) -> f32 {
        self.angle.unwrap_or_else(|| {
            let (dx, dy) = (self.end[0] - self.start[0], self.end[1] - self.start[1]);
            // DrawnUI's 0 is top to bottom, 90 left to right.
            (dx.atan2(dy).to_degrees() + 360.0) % 360.0
        })
    }

    /// The colors at their positions, sorted left to right; `keep` is followed to its new index.
    fn sorted(mut self, keep: usize) -> (Grad, usize) {
        let positions = self.positions();
        let mut items: Vec<(f32, String, usize)> = positions.into_iter().zip(self.colors).enumerate().map(|(i, (p, c))| (p, c, i)).collect();
        items.sort_by(|a, b| a.0.total_cmp(&b.0));
        let at = items.iter().position(|x| x.2 == keep).unwrap_or(0);
        self.stops = Some(items.iter().map(|x| x.0).collect());
        self.colors = items.into_iter().map(|x| x.1).collect();
        (self, at)
    }
}

/// The color between two stops at `t` (straight in RGB, as Skia draws).
fn mix(a: Hsva, b: Hsva, t: f32) -> String {
    let (x, y) = (a.argb(), b.argb());
    let ch = |shift: u32| {
        let (p, q) = (((x >> shift) & 0xFF) as f32, ((y >> shift) & 0xFF) as f32);
        ((p + (q - p) * t).round() as u32) << shift
    };
    format!("#{:08X}", ch(24) | ch(16) | ch(8) | ch(0))
}

impl App {
    /// Starts editing gradient property `index` (`value` its JSON, or "" for none) in its panel.
    pub(super) fn open_gradient(&mut self, value: &str, anchor: drawnui::skia::Rect, cx: &mut Cx) {
        let grad = Grad::parse(value).unwrap_or_else(Grad::new_default);
        let first = grad.color(0);
        self.gradient = Some(grad);
        self.gradient_stop = 0;
        (self.picker, self.picker_old, self.picker_typing) = (Some(first), Some(first), false);
        self.popup = Some(cx.open_popup_window(gradient_editor(self), anchor, PopupWindowOptions { cpu: true, ..Default::default() }));
    }

    /// The edited gradient changed: set live in the app; `done` (end of a drag, a click) also asks
    /// for the properties again.
    pub(super) fn set_gradient(&mut self, grad: Grad, done: bool, cx: &mut Cx) {
        let changed = self.gradient.as_ref() != Some(&grad);
        self.gradient = Some(grad.clone());
        if changed {
            self.edit_value = grad.json();
            self.apply_edit_live(done);
        } else if done && let Some(node) = self.selected_node().map(|n| n.id) {
            self.send(designer_protocol::HubToApp::GetProps { node });
        }
        self.refresh_edited_row(cx);
    }

    /// The picker's color goes to the selected stop.
    pub(super) fn set_stop_color(&mut self, color: Hsva, done: bool, cx: &mut Cx) {
        self.picker = Some(color);
        if let Some(mut g) = self.gradient.clone() {
            if let Some(slot) = g.colors.get_mut(self.gradient_stop) {
                *slot = color.value();
            }
            self.set_gradient(g, done, cx);
        }
    }

    /// Selects stop `i`: the picker shows its color.
    fn select_stop(&mut self, i: usize) {
        self.gradient_stop = i;
        if let Some(g) = &self.gradient {
            let color = g.color(i);
            (self.picker, self.picker_old) = (Some(color), Some(color));
        }
    }

    /// The row of the property being edited shows its new value at once.
    pub(super) fn refresh_edited_row(&mut self, cx: &mut Cx) {
        if let Some(name) = self.edit_prop.clone()
            && let Some(row) = self.prop_rows.iter().position(|r| matches!(r, PropRow::Prop(i) if self.props.get(*i).is_some_and(|p| p.name == name)))
        {
            cx.items_changed(self.props_list, row);
        }
    }
}

/// The bar: the gradient left to right over checkers, with a handle per color under it.
pub struct GradientBar {
    grad: Option<Grad>,
    selected: usize,
    /// A handle is being dragged.
    dragging: bool,
}

const BAR: f32 = 18.0;
const HANDLE: f32 = 6.0;

impl Control for GradientBar {
    fn paint(&self, cx: &mut PaintCx) {
        let Some(g) = &self.grad else { return };
        let (rect, scale) = (cx.rect, cx.scale);
        let canvas = cx.canvas;
        let bar = Rect::from_ltrb(rect.left + HANDLE * scale, rect.top, rect.right - HANDLE * scale, rect.top + BAR * scale);
        let round = RRect::new_rect_xy(bar, 4.0 * scale, 4.0 * scale);
        canvas.save();
        canvas.clip_rrect(round, None, true);
        let mut light = Paint::default();
        light.set_color(drawnui::skia::Color::WHITE);
        canvas.draw_rect(bar, &light);
        let mut dark = Paint::default();
        dark.set_color(drawnui::skia::Color::from_rgb(0xCC, 0xCC, 0xCC));
        let cell = BAR * scale / 2.0;
        let cols = (bar.width() / cell).ceil() as i32;
        for y in 0..2 {
            for x in 0..cols {
                if (x + y) % 2 == 0 {
                    canvas.draw_rect(Rect::from_xywh(bar.left + x as f32 * cell, bar.top + y as f32 * cell, cell, cell), &dark);
                }
            }
        }
        let colors: Vec<Color4f> = (0..g.colors.len()).map(|i| Color4f::from(Color::new(g.color(i).argb()))).collect();
        let positions = g.positions();
        let shader = sk::shaders::linear_gradient(
            (Point::new(bar.left, 0.0), Point::new(bar.right, 0.0)),
            &sk::Gradient::new(sk::Colors::new(&colors, Some(&positions), TileMode::Clamp, None), sk::Interpolation::default()),
            None,
        );
        let mut paint = Paint::default();
        paint.set_shader(shader);
        canvas.draw_rect(bar, &paint);
        canvas.restore();
        // The handles: a small color disc with a ring under the bar, the selected one in the accent.
        for (i, p) in positions.iter().enumerate() {
            let at = Point::new(bar.left + p * bar.width(), bar.bottom + (HANDLE + 3.0) * scale);
            let mut fill = Paint::default();
            fill.set_anti_alias(true);
            fill.set_color(Color::new(g.color(i).argb()));
            canvas.draw_circle(at, HANDLE * scale, &fill);
            let mut ring = Paint::default();
            ring.set_anti_alias(true);
            ring.set_style(PaintStyle::Stroke);
            let selected = i == self.selected;
            ring.set_stroke_width(if selected { 2.5 } else { 1.5 } * scale);
            ring.set_color(if selected { c(ACCENT) } else { drawnui::skia::Color::WHITE });
            canvas.draw_circle(at, HANDLE * scale, &ring);
        }
    }

    fn accessibility_role(&self) -> Option<&'static str> {
        Some(Aria::SLIDER)
    }

    fn accessibility_value(&self) -> Option<drawnui::AccessibilityValue> {
        let g = self.grad.as_ref()?;
        let at = *g.positions().get(self.selected)?;
        let now = (at * 100.0).round() as f64;
        Some(drawnui::AccessibilityValue {
            now,
            min: 0.0,
            max: 100.0,
            step: 1.0,
            text: tr_args("gradient-stop-value", &[("index", ((self.selected + 1) as i64).into()), ("count", (g.colors.len() as i64).into()), ("position", (now as i64).into())]),
        })
    }

    fn cursor(&self, _local: Point) -> Option<Cursor> {
        Some(Cursor::Pointer)
    }
}

/// A press, drag or key on the bar.
fn bar() -> Build<GradientBar> {
    Build::new(GradientBar { grad: None, selected: 0, dragging: false })
        .fill_x()
        .height_request(BAR + 2.0 * HANDLE + 6.0)
        .accessibility_label(tr("gradient-bar"))
        .accessibility_hint(tr("gradient-bar-hint"))
        .accessibility_can_interact(Some(true))
        .observe(|me, app: &App| {
            if me.control_mut().grad != app.gradient || me.control_mut().selected != app.gradient_stop {
                me.control_mut().grad = app.gradient.clone();
                me.control_mut().selected = app.gradient_stop;
                me.mark(drawnui::Dirty::DRAW);
            }
        })
        .consume_gestures(|me, app: &mut App, cx, gesture| {
            let Some(g) = app.gradient.clone() else { return false };
            let (rect, scale) = (me.base().rect, me.base().scale.max(0.1));
            let (left, width) = (rect.left + HANDLE * scale, (rect.width() - 2.0 * HANDLE * scale).max(1.0));
            let t = ((gesture.location.x - left) / width).clamp(0.0, 1.0);
            match gesture.kind {
                GestureKind::Down => {
                    let positions = g.positions();
                    let near = positions.iter().enumerate().map(|(i, p)| (i, ((p - t) * width).abs())).filter(|(_, d)| *d <= 9.0 * scale).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i);
                    match near {
                        Some(i) => app.select_stop(i),
                        None => {
                            // A new color where the bar was pressed, the color the gradient has there.
                            let after = positions.iter().position(|p| *p > t).unwrap_or(positions.len());
                            let color = match (after.checked_sub(1), positions.get(after)) {
                                (Some(a), Some(b)) => mix(g.color(a), g.color(after), (t - positions[a]) / (b - positions[a]).max(0.0001)),
                                (Some(a), None) => g.color(a).value(),
                                (None, _) => g.color(0).value(),
                            };
                            let mut next = g.clone();
                            let mut stops = positions;
                            stops.insert(after, t);
                            next.colors.insert(after, color);
                            next.stops = Some(stops);
                            app.gradient = Some(next.clone());
                            app.select_stop(after);
                            app.set_gradient(next, false, cx);
                        }
                    }
                    me.control_mut().dragging = true;
                    true
                }
                GestureKind::Panning if me.control_mut().dragging => {
                    let mut next = g.clone();
                    let mut stops = next.positions();
                    if let Some(s) = stops.get_mut(app.gradient_stop) {
                        *s = t;
                    }
                    next.stops = Some(stops);
                    let (next, at) = next.sorted(app.gradient_stop);
                    app.gradient_stop = at;
                    app.set_gradient(next, false, cx);
                    true
                }
                GestureKind::Up => {
                    me.control_mut().dragging = false;
                    app.set_gradient(g, true, cx);
                    true
                }
                _ => false,
            }
        })
        .on_key_down(|_me, app: &mut App, cx, event| {
            let Some(g) = app.gradient.clone() else { return false };
            let step = if event.modifiers().shift { 0.1 } else { 0.01 };
            match event.key {
                "Delete" | "Backspace" if g.colors.len() > 2 => {
                    let mut next = g.clone();
                    let mut stops = next.positions();
                    stops.remove(app.gradient_stop);
                    next.colors.remove(app.gradient_stop);
                    next.stops = Some(stops);
                    let at = app.gradient_stop.min(next.colors.len() - 1);
                    app.gradient = Some(next.clone());
                    app.select_stop(at);
                    app.set_gradient(next, true, cx);
                    true
                }
                "ArrowLeft" | "ArrowRight" => {
                    let mut next = g.clone();
                    let mut stops = next.positions();
                    if let Some(s) = stops.get_mut(app.gradient_stop) {
                        *s = (*s + if event.key == "ArrowLeft" { -step } else { step }).clamp(0.0, 1.0);
                    }
                    next.stops = Some(stops);
                    let (next, at) = next.sorted(app.gradient_stop);
                    app.gradient_stop = at;
                    app.set_gradient(next, true, cx);
                    true
                }
                "ArrowUp" | "ArrowDown" => {
                    let count = g.colors.len();
                    let at = if event.key == "ArrowUp" { (app.gradient_stop + 1) % count } else { (app.gradient_stop + count - 1) % count };
                    app.select_stop(at);
                    true
                }
                _ => false,
            }
        })
}

/// A small square button with a sign, for the angle.
fn step_button(sign: &'static str, label: String, delta: f32) -> Build<SkiaShape> {
    super::view::interactive(
        SkiaShape::new().corner_radius(5).width_request(26).height_request(24).stroke_width(1).stroke_color(c(LINE)).accessibility_role(Aria::BUTTON).accessibility_label(label).on_tapped(
            move |_me, app: &mut App, cx| {
                if !super::view::left_click(cx) {
                    return;
                }
                if let Some(mut g) = app.gradient.clone() {
                    g.angle = Some((g.degrees() + delta).rem_euclid(360.0));
                    app.set_gradient(g, true, cx);
                }
            },
        ),
        |_, _| Color::TRANSPARENT,
    )
    .children(text(13.0, TEXT).text(sign).center())
}

/// The panel: type, the bar, the angle (linear types), the color of the selected stop.
pub fn gradient_editor(app: &App) -> Build<SkiaShape> {
    let _ = app;
    let types: Vec<Build<SkiaShape>> = TYPES
        .iter()
        .map(|t| {
            let (on, pick) = (t.to_string(), t.to_string());
            segment(t, move |app: &App| app.gradient.as_ref().is_some_and(|g| g.kind == on), move |app, cx| {
                if let Some(mut g) = app.gradient.clone() {
                    g.kind = pick.clone();
                    app.set_gradient(g, true, cx);
                }
            })
        })
        .collect();
    SkiaShape::new()
        .width_request(color::WIDTH + 22.0)
        .corner_radius(8)
        .background_color(c(HEADER))
        .stroke_color(c(LINE))
        .stroke_width(1)
        .accessibility_role(Aria::DIALOG)
        .accessibility_label(tr("gradient-editor"))
        .children(SkiaStack::new().spacing(10).padding(10).children((
            // The types as one radio group that wraps to a second line in the narrow panel.
            SkiaWrap::new().spacing(4).fill_x().accessibility_role(Aria::RADIO_GROUP).accessibility_label(tr("gradient-type")).children(types),
            bar(),
            SkiaRow::new()
                .spacing(6)
                .fill_x()
                .observe(|me, app: &App| me.set_is_visible(app.gradient.as_ref().is_some_and(|g| matches!(g.kind.as_str(), "Linear" | "Conical"))))
                .children((
                    text(12.0, MUTED).text(tr("gradient-angle")).vertical_options(LayoutOptions::Center),
                    step_button("\u{2212}", tr("gradient-angle-less"), -15.0),
                    text(12.0, TEXT)
                        .vertical_options(LayoutOptions::Center)
                        .observe(|me, app: &App| me.set_text(app.gradient.as_ref().map_or(String::new(), |g| format!("{}°", g.degrees().round())))),
                    step_button("+", tr("gradient-angle-more"), 15.0),
                )),
            color::picker_parts(),
            text(11.0, MUTED).text(tr("gradient-bar-hint")).max_lines(3).fill_x(),
        )))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The editor drawn on its own for a look: target/designer-shots/26-gradient-editor.png.
    #[test]
    #[ignore]
    fn gradient_editor_shot() {
        let grad = Grad::parse(r##"{"type":"Linear","colors":["#FF667EEA","#FF764BA2","#FFF093FB"],"stops":[0,0.6,1],"angle":135,"start":[0,0],"end":[1,1],"opacity":1}"##).unwrap();
        let state = App { picker: Some(grad.color(1)), picker_old: Some(grad.color(1)), gradient: Some(grad), gradient_stop: 1, ..Default::default() };
        let font = |name: &str| std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("assets/{name}"))).expect("font file");
        let ui = drawnui::Ui::new(state, |app| SkiaLayer::new().fill().background_color(c(HEADER)).padding(20).children(gradient_editor(app)))
            .font_bytes("Default", &font("OpenSans-Regular.ttf"))
            .font_bytes("Semibold", &font("OpenSans-Semibold.ttf"));
        let mut host = drawnui::testing::Headless::new(ui, 320, 560, 1.0);
        host.settle();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/designer-shots");
        std::fs::create_dir_all(&dir).expect("shots folder");
        host.save_png(dir.join("26-gradient-editor.png").to_str().unwrap());
    }

    #[test]
    fn a_gradient_round_trips_and_spreads_its_colors() {
        let json = r##"{"type":"Linear","colors":["#FFFF0000","#FF0000FF","#FF00FF00"],"stops":null,"angle":45,"start":[0,0],"end":[1,1],"opacity":1}"##;
        let g = Grad::parse(json).unwrap();
        assert_eq!(g.positions(), [0.0, 0.5, 1.0]);
        assert_eq!(g.degrees(), 45.0);
        assert_eq!(Grad::parse(&g.json()).unwrap(), g);
        assert!(Grad::parse("").is_none());
        let (sorted, at) = Grad { stops: Some(vec![0.0, 0.9, 0.2]), ..g.clone() }.sorted(1);
        assert_eq!((sorted.stops.unwrap(), sorted.colors[2].as_str(), at), (vec![0.0, 0.2, 0.9], "#FF0000FF", 2));
        assert_eq!(mix(Hsva::parse("#FF000000").unwrap(), Hsva::parse("#FFFFFFFF").unwrap(), 0.5), "#FF808080");
    }
}
