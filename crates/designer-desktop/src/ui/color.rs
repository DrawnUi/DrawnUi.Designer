//! The color picker: a saturation / brightness square, a hue bar and an opacity bar drawn with Skia,
//! a hex field and the old and new color side by side. Opened from a color property's value; every
//! change is set live in the app, Esc puts the old color back.

use drawnui::prelude::*;
use drawnui::skia::{Color4f, Paint, PaintStyle, Point, RRect, Rect, TileMode, gradient};

use super::view::{ACCENT, HEADER, INPUT, LINE, MUTED, TEXT, c, text};
use super::App;
use crate::i18n::{tr, tr_args};

/// A color as the picker edits it: hue 0..360, saturation, brightness and opacity 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsva {
    pub h: f32,
    pub s: f32,
    pub v: f32,
    pub a: f32,
}

impl Hsva {
    /// From `#AARRGGBB` / `#RRGGBB` as the app sends colors.
    pub fn parse(value: &str) -> Option<Hsva> {
        let digits = value.trim().trim_start_matches('#');
        let v = u32::from_str_radix(digits, 16).ok()?;
        let argb = match digits.len() {
            8 => v,
            6 => 0xFF00_0000 | v,
            _ => return None,
        };
        let (a, r, g, b) = ((argb >> 24) as f32 / 255.0, ((argb >> 16) & 0xFF) as f32 / 255.0, ((argb >> 8) & 0xFF) as f32 / 255.0, (argb & 0xFF) as f32 / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let d = max - min;
        let h = if d == 0.0 {
            0.0
        } else if max == r {
            60.0 * (((g - b) / d).rem_euclid(6.0))
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        Some(Hsva { h, s: if max == 0.0 { 0.0 } else { d / max }, v: max, a })
    }

    /// Red, green, blue 0..1.
    fn rgb(self) -> (f32, f32, f32) {
        let c = self.v * self.s;
        let x = c * (1.0 - ((self.h / 60.0).rem_euclid(2.0) - 1.0).abs());
        let m = self.v - c;
        let (r, g, b) = match (self.h.rem_euclid(360.0) / 60.0) as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        (r + m, g + m, b + m)
    }

    /// `0xAARRGGBB`.
    pub fn argb(self) -> u32 {
        let (r, g, b) = self.rgb();
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
        (byte(self.a) << 24) | (byte(r) << 16) | (byte(g) << 8) | byte(b)
    }

    /// What the app is sent: always `#AARRGGBB`.
    pub fn value(self) -> String {
        format!("#{:08X}", self.argb())
    }

    /// What the hex field shows: `#RRGGBB` when opaque, else `#AARRGGBB`.
    pub fn short(self) -> String {
        let argb = self.argb();
        if argb >> 24 == 0xFF { format!("#{:06X}", argb & 0xFF_FFFF) } else { format!("#{argb:08X}") }
    }

    pub fn color(self) -> Color {
        Color::new(self.argb())
    }
}

/// Which part of the picker a [`ColorArea`] is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Area {
    /// Saturation left to right, brightness bottom to top.
    Plane,
    Hue,
    Opacity,
}

/// One drawn part of the picker; its color follows the app's picker through `observe`.
pub struct ColorArea {
    area: Area,
    color: Hsva,
}

const RADIUS: f32 = 6.0;

impl ColorArea {
    fn checkers(canvas: &drawnui::skia::Canvas, rect: Rect, cell: f32) {
        let mut light = Paint::default();
        light.set_color(drawnui::skia::Color::from_rgb(0xFF, 0xFF, 0xFF));
        let mut dark = Paint::default();
        dark.set_color(drawnui::skia::Color::from_rgb(0xCC, 0xCC, 0xCC));
        canvas.draw_rect(rect, &light);
        let (cols, rows) = ((rect.width() / cell).ceil() as i32, (rect.height() / cell).ceil() as i32);
        for y in 0..rows {
            for x in 0..cols {
                if (x + y) % 2 == 0 {
                    let mut r = Rect::from_xywh(rect.left + x as f32 * cell, rect.top + y as f32 * cell, cell, cell);
                    if r.intersect(rect) {
                        canvas.draw_rect(r, &dark);
                    }
                }
            }
        }
    }

    fn linear(from: Point, to: Point, colors: &[Color4f]) -> Option<drawnui::skia::Shader> {
        let g = gradient::Gradient::new(gradient::Colors::new(colors, None, TileMode::Clamp, None), gradient::Interpolation::default());
        gradient::shaders::linear_gradient((from, to), &g, None)
    }

    /// The thumb: a white ring with a dark outline, filled with the color under it.
    fn thumb(canvas: &drawnui::skia::Canvas, at: Point, radius: f32, fill: Color, scale: f32) {
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_color(fill);
        canvas.draw_circle(at, radius, &paint);
        paint.set_style(PaintStyle::Stroke);
        paint.set_stroke_width(2.0 * scale);
        paint.set_color(drawnui::skia::Color::WHITE);
        canvas.draw_circle(at, radius, &paint);
        paint.set_stroke_width(1.0 * scale);
        paint.set_color(drawnui::skia::Color::from_argb(0x66, 0, 0, 0));
        canvas.draw_circle(at, radius + 1.5 * scale, &paint);
    }
}

impl Control for ColorArea {
    fn paint(&self, cx: &mut PaintCx) {
        let (rect, scale) = (cx.rect, cx.scale);
        let canvas = cx.canvas;
        let round = RRect::new_rect_xy(rect, RADIUS * scale, RADIUS * scale);
        canvas.save();
        canvas.clip_rrect(round, None, true);
        let c4 = |c: Color| Color4f::from(c);
        match self.area {
            Area::Plane => {
                let hue = Hsva { s: 1.0, v: 1.0, a: 1.0, ..self.color };
                let mut paint = Paint::default();
                paint.set_color(hue.color());
                canvas.draw_rect(rect, &paint);
                paint.set_shader(Self::linear(Point::new(rect.left, 0.0), Point::new(rect.right, 0.0), &[c4(Color::WHITE), c4(Color::from_argb(0, 255, 255, 255))]));
                canvas.draw_rect(rect, &paint);
                paint.set_shader(Self::linear(Point::new(0.0, rect.top), Point::new(0.0, rect.bottom), &[c4(Color::from_argb(0, 0, 0, 0)), c4(Color::BLACK)]));
                canvas.draw_rect(rect, &paint);
            }
            Area::Hue => {
                let stops: Vec<Color4f> = (0..=6).map(|i| c4(Hsva { h: i as f32 * 60.0, s: 1.0, v: 1.0, a: 1.0 }.color())).collect();
                let mut paint = Paint::default();
                paint.set_shader(Self::linear(Point::new(rect.left, 0.0), Point::new(rect.right, 0.0), &stops));
                canvas.draw_rect(rect, &paint);
            }
            Area::Opacity => {
                Self::checkers(canvas, rect, (rect.height() / 2.0).max(3.0));
                let opaque = Hsva { a: 1.0, ..self.color }.color();
                let mut paint = Paint::default();
                paint.set_shader(Self::linear(Point::new(rect.left, 0.0), Point::new(rect.right, 0.0), &[c4(Color::from_argb(0, opaque.r(), opaque.g(), opaque.b())), c4(opaque)]));
                canvas.draw_rect(rect, &paint);
            }
        }
        canvas.restore();
        let (at, radius) = match self.area {
            Area::Plane => (Point::new(rect.left + self.color.s * rect.width(), rect.top + (1.0 - self.color.v) * rect.height()), 7.0 * scale),
            Area::Hue => (Point::new(rect.left + self.color.h / 360.0 * rect.width(), rect.center_y()), rect.height() / 2.0 + 2.0 * scale),
            Area::Opacity => (Point::new(rect.left + self.color.a * rect.width(), rect.center_y()), rect.height() / 2.0 + 2.0 * scale),
        };
        let fill = match self.area {
            Area::Plane => Hsva { a: 1.0, ..self.color }.color(),
            Area::Hue => Hsva { s: 1.0, v: 1.0, a: 1.0, ..self.color }.color(),
            Area::Opacity => self.color.color(),
        };
        Self::thumb(canvas, at, radius, fill, scale);
    }

    fn accessibility_role(&self) -> Option<&'static str> {
        Some(Aria::SLIDER)
    }

    fn accessibility_value(&self) -> Option<drawnui::AccessibilityValue> {
        let (now, max, text) = match self.area {
            Area::Plane => (
                (self.color.s * 100.0).round() as f64,
                100.0,
                tr_args("picker-plane-value", &[("saturation", ((self.color.s * 100.0).round() as i64).into()), ("brightness", ((self.color.v * 100.0).round() as i64).into())]),
            ),
            Area::Hue => (self.color.h.round() as f64, 360.0, format!("{}°", self.color.h.round())),
            Area::Opacity => ((self.color.a * 100.0).round() as f64, 100.0, format!("{}%", (self.color.a * 100.0).round())),
        };
        Some(drawnui::AccessibilityValue { now, min: 0.0, max, step: 1.0, text })
    }

    fn cursor(&self, _local: Point) -> Option<Cursor> {
        Some(Cursor::Pointer)
    }
}

/// The new color for a press or drag at `at` (pixels) on `area` laid out at `rect`.
fn color_at(area: Area, color: Hsva, rect: Rect, at: Point) -> Hsva {
    let x = ((at.x - rect.left) / rect.width().max(1.0)).clamp(0.0, 1.0);
    let y = ((at.y - rect.top) / rect.height().max(1.0)).clamp(0.0, 1.0);
    match area {
        Area::Plane => Hsva { s: x, v: 1.0 - y, ..color },
        Area::Hue => Hsva { h: x * 360.0, ..color },
        Area::Opacity => Hsva { a: x, ..color },
    }
}

/// The color for an arrow key on `area`: one step (Shift: ten).
fn color_for_key(area: Area, color: Hsva, key: &str, big: bool) -> Option<Hsva> {
    let step = if big { 0.1 } else { 0.01 };
    let (dx, dy) = match key {
        "ArrowLeft" => (-1.0, 0.0),
        "ArrowRight" => (1.0, 0.0),
        "ArrowUp" => (0.0, 1.0),
        "ArrowDown" => (0.0, -1.0),
        _ => return None,
    };
    Some(match area {
        Area::Plane => Hsva { s: (color.s + dx * step).clamp(0.0, 1.0), v: (color.v + dy * step).clamp(0.0, 1.0), ..color },
        Area::Hue => Hsva { h: (color.h + (dx + dy) * step * 360.0).clamp(0.0, 360.0), ..color },
        Area::Opacity => Hsva { a: (color.a + (dx + dy) * step).clamp(0.0, 1.0), ..color },
    })
}

fn area(area: Area, width: f32, height: f32, label: String) -> Build<ColorArea> {
    Build::new(ColorArea { area, color: Hsva { h: 0.0, s: 0.0, v: 0.0, a: 1.0 } })
        .width_request(width)
        .height_request(height)
        .accessibility_label(label)
        .accessibility_can_interact(Some(true))
        .observe(move |me, app: &App| {
            if let Some(color) = app.picker {
                if me.control_mut().color != color {
                    me.control_mut().color = color;
                    me.mark(drawnui::Dirty::DRAW);
                }
            }
        })
        .consume_gestures(move |me, app: &mut App, cx, gesture| {
            let Some(color) = app.picker else { return false };
            match gesture.kind {
                GestureKind::Down | GestureKind::Panning => {
                    let next = color_at(area, color, me.base().rect, gesture.location);
                    app.set_picker(next, false, cx);
                    true
                }
                GestureKind::Up => {
                    app.set_picker(color, true, cx);
                    true
                }
                _ => false,
            }
        })
        .on_key_down(move |_me, app: &mut App, cx, event| {
            let Some(color) = app.picker else { return false };
            match color_for_key(area, color, event.key, event.modifiers().shift) {
                Some(next) => {
                    app.set_picker(next, true, cx);
                    true
                }
                None => false,
            }
        })
}

/// The hex field: shows the color while it is not being typed in; a valid color typed applies live,
/// Enter applies it too.
fn hex_field() -> Build<SkiaEditor> {
    SkiaEditor::new()
        .font_size(12)
        .text_color(c(TEXT))
        .background_color(c(INPUT))
        .corner_radius(5)
        .stroke_color(c(LINE))
        .stroke_width(1)
        .cursor_color(c(ACCENT))
        .padding((8, 5))
        .fill_x()
        .vertical_options(LayoutOptions::Center)
        .accessibility_label(tr("picker-hex"))
        .observe(|me, app: &App| {
            if app.picker_typing {
                return;
            }
            if let Some(color) = app.picker {
                let short = color.short();
                if me.p.text != short {
                    me.set_text(short.as_str());
                }
            }
        })
        .on_focus_changed(|_me, app: &mut App, _cx, focused| app.picker_typing = focused)
        .on_text_changed(|_me, app: &mut App, cx, text| {
            if app.picker_typing && let Some(color) = Hsva::parse(text) {
                app.set_picker(color, false, cx);
            }
        })
        .on_text_submitted(|_me, app: &mut App, cx, text| {
            if let Some(color) = Hsva::parse(text) {
                app.set_picker(color, true, cx);
            }
        })
}

/// The width of the picker's areas, points.
pub const WIDTH: f32 = 232.0;

/// The picker's popup content, for the color being edited.
pub fn color_picker(_app: &App) -> Build<SkiaShape> {
    SkiaShape::new()
        .width_request(WIDTH + 22.0)
        .corner_radius(8)
        .background_color(c(HEADER))
        .stroke_color(c(LINE))
        .stroke_width(1)
        .accessibility_role(Aria::DIALOG)
        .accessibility_label(tr("picker"))
        .children(SkiaStack::new().spacing(10).padding(10).children(picker_parts()))
}

/// The picker itself (square, hue, opacity, old and new color, hex field, keys), for the color
/// picker and the gradient editor's selected stop.
pub fn picker_parts() -> Build<SkiaLayout> {
    SkiaStack::new().spacing(10).fill_x().children((
            area(Area::Plane, WIDTH, 150.0, tr("picker-plane")),
            area(Area::Hue, WIDTH, 12.0, tr("picker-hue")),
            area(Area::Opacity, WIDTH, 12.0, tr("picker-opacity")),
            SkiaRow::new().spacing(8).fill_x().children((
                // The old color, then the new one.
                SkiaShape::new()
                    .corner_radius(5)
                    .width_request(56)
                    .height_request(28)
                    .stroke_width(1)
                    .stroke_color(c(LINE))
                    .vertical_options(LayoutOptions::Center)
                    .accessibility_role(Aria::IMG)
                    .observe(|me, app: &App| me.set_accessibility_label(tr_args("picker-old-new", &[("old", app.picker_old.map(|c| c.short()).unwrap_or_default().into())])))
                    .children(SkiaRow::new().spacing(0).fill().children((
                        SkiaLayer::new().width_request(28).fill_y().observe(|me, app: &App| me.set_background_color(app.picker_old.map(|c| c.color()).unwrap_or(Color::TRANSPARENT))),
                        SkiaLayer::new().width_request(28).fill_y().observe(|me, app: &App| {
                            if let Some(color) = app.picker {
                                me.set_background_color(color.color());
                            }
                        }),
                    ))),
                hex_field(),
            )),
            text(11.0, MUTED).text(tr("picker-keys")).max_lines(3).fill_x(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_round_trip() {
        for v in ["#FFFF0000", "#FF00FF00", "#FF0000FF", "#80123456", "#FFFFFFFF", "#FF000000", "#00000000", "#FF4C8DFF"] {
            assert_eq!(Hsva::parse(v).unwrap().value(), v, "{v}");
        }
        assert_eq!(Hsva::parse("#4C8DFF").unwrap().value(), "#FF4C8DFF");
        assert_eq!(Hsva::parse("#FF4C8DFF").unwrap().short(), "#4C8DFF");
        assert_eq!(Hsva::parse("#804C8DFF").unwrap().short(), "#804C8DFF");
        assert!(Hsva::parse("red").is_none());
    }
}
