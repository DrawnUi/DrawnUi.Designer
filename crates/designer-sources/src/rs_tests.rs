//! The Rust reader and writer against the DrawFiddle Welcome template, a snippet whose helpers make
//! the controls, and the designer agent's sample app.

use std::collections::HashMap;

use super::*;

/// The DrawFiddle "Welcome" Rust template (rust-welcome), as the page sends it.
const WELCOME: &str = r###"// The Rust twin of the C# and TSX "Welcome" fiddles: same controls, same names, same screen.
use drawnui::prelude::*;

#[derive(Default)]
struct App {
    count: i32,
}

fn title(count: i32) -> String {
    if count == 0 { "Click Me!".to_owned() } else { format!("Clicked {count} times!") }
}

fn build(_app: &mut App) -> Build<SkiaLayout> {
    SkiaLayout::column()
        .use_cache(CacheType::Operations)
        .spacing(18)
        .padding(24)
        .horizontal_options(LayoutOptions::Center)
        .vertical_options(LayoutOptions::Center)
        .children((
            SkiaSvg::new("/_content/DrawnUi.Fiddle/seed/drawnui.svg")
                .width_request(150)
                .lock_ratio(1)
                .use_cache(CacheType::Operations)
                .horizontal_options(LayoutOptions::Center),
            SkiaLabel::new("Welcome to DrawnUI")
                .font_family("FontTextTitle")
                .font_size(26)
                .text_color(Color::WHITE)
                .horizontal_options(LayoutOptions::Center),
            SkiaRichLabel::new("Draw with **Skia** everywhere. This Rust draws with **skia-safe** in the browser and on the desktop, and the same engine draws **C#** and **React** apps.")
                .font_family("FontText")
                .font_size(15)
                .line_spacing(1.3)
                .text_color(Color::from_rgb(0x94, 0xA3, 0xB8))
                .width_request(380)
                .horizontal_text_alignment(TextAlignment::Center)
                .horizontal_options(LayoutOptions::Center),
            SkiaRichLabel::new("")
                .font_family("FontText")
                .font_family_fallback("FontEmoji")
                .font_size(13)
                .text_color(Color::from_rgb(0x67, 0xE8, 0xF9))
                .horizontal_options(LayoutOptions::Center)
                // The app state is the source of the text: observe re-reads it after every change.
                .observe(|me, app: &App| me.set_text(format!("State drives the button title: \"{}\" 🔥", title(app.count)))),
            SkiaButton::new("Click Me!")
                .background_color(Color::from_rgb(0xDC, 0x14, 0x3C))
                .text_color(Color::WHITE)
                .corner_radius(10)
                .horizontal_options(LayoutOptions::Center)
                .observe(|me, app: &App| me.set_text(title(app.count)))
                .on_tapped(|_me, app: &mut App, _cx| {
                    app.count += 1;
                    println!("Button clicked {} time(s)", app.count);
                }),
        ))
}
"###;

/// Helpers make the controls: a Kids-like snippet.
const KIDS: &str = r###"use drawnui::prelude::*;

#[derive(Default)]
struct App;

fn circle(colour: Color, size: f32) -> Build<SkiaShape> {
    SkiaShape::new().shape_type(ShapeType::Circle).width_request(size).height_request(size).background_color(colour)
}

fn square(colour: Color) -> Build<SkiaShape> {
    SkiaShape::new().width_request(40).height_request(40).background_color(colour)
}

fn text(t: &str) -> Build<SkiaLabel> {
    SkiaLabel::new(t).font_size(18)
}

fn stack(children: impl IntoChildren) -> Build<SkiaLayout> {
    SkiaStack::new().spacing(8).children(children)
}

fn build(_app: &mut App) -> Build<SkiaLayout> {
    stack((
        text("Shapes"),
        circle(Color::from_rgb(255, 0, 0), 40.0),
        square(Color::BLUE),
        circle(Color::from_rgb(0, 128, 0), 24.0),
    ))
}
"###;

const SAMPLE: &str = include_str!("../../../agents/rust/sample/src/main.rs");

use designer_protocol::{ControlSpec, Node as RtNode, SpecProp, Tree as RtTree};

fn rt(nodes: &[(u64, Option<u64>, &str)]) -> RtTree {
    RtTree { revision: 1, nodes: nodes.iter().map(|&(id, parent, t)| RtNode { id, parent, type_name: t.into(), ..Default::default() }).collect(), ..Default::default() }
}

/// The welcome window as the agent reports it: the button's label is the library's.
fn welcome_tree() -> RtTree {
    let mut t = rt(&[(1, None, "SkiaLayout"), (2, Some(1), "SkiaSvg"), (3, Some(1), "SkiaLabel"), (4, Some(1), "SkiaRichLabel"), (5, Some(1), "SkiaRichLabel"), (6, Some(1), "SkiaButton"), (7, Some(6), "SkiaLabel")]);
    t.nodes[6].internal = true;
    t
}

fn kids_tree() -> RtTree {
    rt(&[(1, None, "SkiaLayout"), (2, Some(1), "SkiaLabel"), (3, Some(1), "SkiaShape"), (4, Some(1), "SkiaShape"), (5, Some(1), "SkiaShape")])
}

fn files(text: &str) -> Vec<(String, String)> {
    vec![("fiddle:///snippet.rs".to_string(), text.to_string())]
}

fn keys(text: &str, tree: &RtTree) -> Vec<(u64, String)> {
    let mut out: Vec<(u64, String)> = crate::match_runtime(&files(text), tree).into_iter().map(|m| (m.node, m.key)).collect();
    out.sort();
    out
}

/// The one span two texts differ in: (what was there, what is there now).
fn diff(old: &str, new: &str) -> (String, String) {
    let head = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    let tail = old[head..].bytes().rev().zip(new[head..].bytes().rev()).take_while(|(a, b)| a == b).count();
    (old[head..old.len() - tail].to_string(), new[head..new.len() - tail].to_string())
}

fn set(text: &str, tree: &RtTree, node: u64, prop: &str, type_name: &str, value: &str) -> Result<String, String> {
    let v = crate::value_from_runtime(type_name, value, value).map_err(|r| r.key)?;
    crate::set_property(&files(text), tree, node, prop, &v).map(|e| e.text).map_err(|r| r.key)
}

/// Only `old` became `new` in one place, nothing else in the file moved.
#[track_caller]
fn changed(before: &str, after: &str, old: &str, new: &str) {
    let ok = after.match_indices(new).any(|(p, _)| format!("{}{old}{}", &after[..p], &after[p + new.len()..]) == before);
    let (o, n) = diff(before, after);
    assert!(ok, "expected `{old}` -> `{new}`, got `{o}` -> `{n}`
{after}");
}

fn spec(type_name: &str, props: &[(&str, &str, &str)]) -> ControlSpec {
    ControlSpec { type_name: type_name.into(), props: props.iter().map(|(n, v, t)| SpecProp { name: n.to_string(), value: v.to_string(), type_name: t.to_string() }).collect(), children: vec![] }
}

fn insert(text: &str, tree: &RtTree, parent: u64, index: usize, s: &ControlSpec) -> Result<String, String> {
    crate::insert_spec(&files(text), tree, parent, index, s, &[], &HashMap::new()).map(|e| e.text).map_err(|r| r.key)
}

#[test]
fn reads_controls_and_calls() {
    let src = read_rust("fiddle:///snippet.rs", WELCOME);
    let els: Vec<(&str, &str)> = src.nodes.iter().filter(|n| n.role == SourceRole::Element).map(|n| (n.key.as_str(), n.name.as_deref().unwrap_or(""))).collect();
    assert_eq!(els, [("build/0", "SkiaLayout"), ("build/1", "SkiaSvg"), ("build/2", "SkiaLabel"), ("build/3", "SkiaRichLabel"), ("build/4", "SkiaRichLabel"), ("build/5", "SkiaButton")]);
    let label = src.nodes.iter().position(|n| n.key == "build/2").unwrap();
    let calls: Vec<&str> = src.nodes.iter().filter(|n| n.parent == Some(label)).filter_map(|n| n.name.as_deref()).collect();
    assert_eq!(calls, ["font_family", "font_size", "text_color", "horizontal_options"]);
    // Code set: the text from the constructor, the builder calls; not the handlers.
    let code = crate::element_code("fiddle:///snippet.rs", WELCOME);
    assert_eq!(code["build/5"].names, ["text", "background_color", "text_color", "corner_radius", "horizontal_options"]);
    // `vec![..]` and macro arguments read as expressions.
    let kids = read_rust("a.rs", "fn build() -> Build<SkiaLayout> { SkiaLayout::new().children(vec![SkiaLabel::new(\"a\"), SkiaLabel::new(\"b\")]) }");
    assert_eq!(kids.nodes.iter().filter(|n| n.role == SourceRole::Element).count(), 3);
}

#[test]
fn matches_the_fiddle_welcome_from_its_build_function() {
    let got = keys(WELCOME, &welcome_tree());
    assert_eq!(got, [(1, "build/0".into()), (2, "build/1".into()), (3, "build/2".into()), (4, "build/3".into()), (5, "build/4".into()), (6, "build/5".into())]);
}

#[test]
fn matches_through_helpers_and_their_parameters() {
    let ms = crate::match_runtime(&files(KIDS), &kids_tree());
    let key = |n: u64| ms.iter().find(|m| m.node == n).map(|m| (m.key.clone(), m.via.clone())).unwrap();
    assert_eq!(key(1).0, "stack/0");
    assert_eq!(key(2).0, "text/0");
    assert_eq!(key(3).0, "circle/0");
    assert_eq!(key(4).0, "square/0");
    assert_eq!(key(5).0, "circle/0");
    assert_ne!(key(3).1, key(5).1, "each circle by its own call");
    // Layers names: the helper that made each.
    let names = crate::made_by(&files(KIDS), &ms);
    let mut got: Vec<(u64, &str)> = names.iter().map(|(n, s)| (*n, s.as_str())).collect();
    got.sort();
    assert_eq!(got, [(1, "stack"), (2, "text"), (3, "circle"), (4, "square"), (5, "circle")]);
}

#[test]
fn matches_the_designer_sample() {
    let tree = rt(&[
        (1, None, "SkiaLayout"),
        (2, Some(1), "SkiaLabel"),
        (3, Some(1), "SkiaLabel"),
        (4, Some(1), "SkiaLayout"),
        (5, Some(4), "SkiaShape"),
        (6, Some(4), "SkiaShape"),
        (7, Some(4), "SkiaShape"),
        (8, Some(1), "SkiaShape"),
        (9, Some(8), "SkiaLayout"),
        (10, Some(9), "SkiaLabel"),
        (11, Some(9), "SkiaLabel"),
        (12, Some(1), "SkiaLayout"),
        (13, Some(12), "SkiaButton"),
        (14, Some(12), "SkiaButton"),
        (15, Some(1), "SkiaLayout"),
        (16, Some(15), "SkiaSwitch"),
        (17, Some(15), "SkiaLabel"),
    ]);
    let ms = crate::match_runtime(&[("C:/app/src/main.rs".into(), SAMPLE.into())], &tree);
    assert_eq!(ms.len(), 17, "{ms:?}");
    let names = crate::made_by(&[("C:/app/src/main.rs".into(), SAMPLE.into())], &ms);
    assert_eq!(names.get(&5).map(String::as_str), Some("swatch"));
    assert_eq!(names.get(&13).map(String::as_str), Some("button"));
    assert_eq!(names.get(&1), None, "the window's own build function names nothing");
    // A value through `swatch(color, name)`: the call's argument.
    let v = crate::value_from_runtime("System.String", "blue", "sky").unwrap();
    let e = crate::set_property(&[("C:/app/src/main.rs".into(), SAMPLE.into())], &tree, 5, "tag", &v).unwrap();
    changed(SAMPLE, &e.text, "\"blue\"", "\"sky\"");
}

#[test]
fn writes_values_in_their_own_form() {
    let tree = welcome_tree();
    let t = |node, prop, ty, value| set(WELCOME, &tree, node, prop, ty, value).unwrap();
    changed(WELCOME, &t(3, "font_size", "System.Single", "30"), "26", "30");
    changed(WELCOME, &t(4, "line_spacing", "System.Single", "1.5"), "1.3", "1.5");
    changed(WELCOME, &t(3, "text", "System.String", "Hi \"there\""), "Welcome to DrawnUI", "Hi \\\"there\\\"");
    changed(WELCOME, &t(1, "horizontal_options", "LayoutOptions", "Start"), "Center", "Start");
    changed(WELCOME, &t(6, "background_color", "drawnui.skia.Color", "#FF0000FF"), "0xDC, 0x14, 0x3C", "0x00, 0x00, 0xFF");
    changed(WELCOME, &t(3, "text_color", "drawnui.skia.Color", "#FF000000"), "WHITE", "BLACK");
    changed(WELCOME, &t(6, "text_color", "drawnui.skia.Color", "#80102030"), "Color::WHITE", "Color::from_argb(0x80, 0x10, 0x20, 0x30)");
    changed(WELCOME, &t(1, "padding", "Thickness", "24,12,24,12"), "24", "(24, 12)");
    changed(WELCOME, &t(1, "padding", "Thickness", "1.5"), "24", "1.5");
    // Not set yet: a call at the end of the chain, on its own line like the others.
    changed(WELCOME, &t(2, "margin", "Thickness", "8"), "", "\n                .margin(8)");
    changed(WELCOME, &t(6, "is_visible", "System.Boolean", "False"), "", "\n                .is_visible(false)");
}

#[test]
fn writes_through_helper_parameters_and_at_the_use_site() {
    let tree = kids_tree();
    let t = |node, prop, ty, value| set(KIDS, &tree, node, prop, ty, value).unwrap();
    // `circle(colour, size)`: the first circle's own arguments.
    changed(KIDS, &t(3, "background_color", "drawnui.skia.Color", "#FF0000FF"), "255, 0, 0", "0, 0, 255");
    changed(KIDS, &t(5, "width_request", "System.Single", "30"), "24.0", "30.0");
    changed(KIDS, &t(4, "background_color", "drawnui.skia.Color", "#FFFFFFFF"), "BLUE", "WHITE");
    // A literal in the helper's body: every use shares it.
    changed(KIDS, &t(2, "font_size", "System.Single", "20"), "18", "20");
    // Not set by the helper: on this use only.
    changed(KIDS, &t(3, "tag", "System.String", "red"), "", ".tag(\"red\")");
    let after = t(3, "tag", "System.String", "red");
    assert!(after.contains("circle(Color::from_rgb(255, 0, 0), 40.0).tag(\"red\"),"), "{after}");
}

#[test]
fn inserts_into_the_children_tuple() {
    let tree = welcome_tree();
    let label = spec("SkiaLabel", &[]);
    // First, middle, last: one per line at the siblings' indent, trailing comma kept.
    let at = |i| insert(WELCOME, &tree, 1, i, &label).unwrap();
    changed(WELCOME, &at(0), "", "SkiaLabel::new(\"Label\"),\n            ");
    changed(WELCOME, &at(2), "", "SkiaLabel::new(\"Label\"),\n            ");
    changed(WELCOME, &at(9), "", "\n            SkiaLabel::new(\"Label\"),");
    // Values: the text into the constructor, the others as builder calls.
    let with = spec("SkiaLabel", &[("text", "Hi", "System.String"), ("font_size", "20", "System.Single"), ("horizontal_options", "Center", "LayoutOptions")]);
    changed(WELCOME, &insert(WELCOME, &tree, 1, 9, &with).unwrap(), "", "\n            SkiaLabel::new(\"Hi\").font_size(20).horizontal_options(LayoutOptions::Center),");
    // A control without children gets `.children(..)`; helpers' controls on this use only.
    let k = kids_tree();
    let after = insert(KIDS, &k, 4, 0, &label).unwrap();
    changed(KIDS, &after, "", ".children(SkiaLabel::new(\"Label\"))");
    assert!(after.contains("square(Color::BLUE).children(SkiaLabel::new(\"Label\")),"), "{after}");
    // Into the helper's parameter: the caller's tuple.
    let after = insert(KIDS, &k, 1, 1, &spec("SkiaShape", &[])).unwrap();
    changed(KIDS, &after, "", "SkiaShape::new(),\n        ");
    // A copy with children, laid out over lines.
    let mut card = spec("SkiaLayout", &[("spacing", "6", "System.Single")]);
    card.children = vec![spec("SkiaLabel", &[("text", "A", "System.String")]), spec("SkiaLabel", &[("text", "B", "System.String")])];
    let after = insert(WELCOME, &tree, 1, 9, &card).unwrap();
    changed(WELCOME, &after, "", "\n            SkiaLayout::new()\n                .spacing(6)\n                .children((\n                    SkiaLabel::new(\"A\"),\n                    SkiaLabel::new(\"B\"),\n                )),");
}

#[test]
fn removes_and_moves_items() {
    let tree = welcome_tree();
    let f = files(WELCOME);
    // The first item with the line break after it; the last with the one before it.
    let svg = &WELCOME[WELCOME.find("SkiaSvg::new").unwrap()..WELCOME.find("SkiaLabel::new(\"Welcome").unwrap()];
    changed(WELCOME, &crate::remove_element(&f, &tree, 2).unwrap().text, svg, "");
    let button = &WELCOME[WELCOME.find("\n            SkiaButton::new").unwrap()..WELCOME.find("}),\n        ))").unwrap() + 3];
    changed(WELCOME, &crate::remove_element(&f, &tree, 6).unwrap().text, button, "");
    // Helper-made: its call's item.
    let k = kids_tree();
    let after = crate::remove_element(&files(KIDS), &k, 4).unwrap().text;
    changed(KIDS, &after, "        square(Color::BLUE),\n", "");
    // A move: the item's own text, re-indented, at its new place.
    let moved = crate::move_element(&f, &tree, 6, 1, 0).unwrap().text;
    assert!(moved.contains(".children((\n            SkiaButton::new(\"Click Me!\")\n                .background_color("), "{moved}");
    assert_eq!(moved.matches("SkiaButton::new").count(), 1);
    assert_eq!(moved.len(), WELCOME.len());
    // The only child takes its `.children(..)` with it.
    let one = "fn build() -> Build<SkiaLayout> {\n    SkiaLayout::new()\n        .spacing(4)\n        .children(SkiaLabel::new(\"a\"))\n}\n";
    let t1 = rt(&[(1, None, "SkiaLayout"), (2, Some(1), "SkiaLabel")]);
    changed(one, &crate::remove_element(&files(one), &t1, 2).unwrap().text, "\n        .children(SkiaLabel::new(\"a\"))", "");
}

#[test]
fn refuses_what_the_code_makes_at_run_time() {
    let looped = "fn build() -> Build<SkiaLayout> {\n    SkiaLayout::column().children((0..3).map(|i| SkiaLabel::new(format!(\"{i}\"))).collect::<Vec<_>>())\n}\n";
    let tree = rt(&[(1, None, "SkiaLayout"), (2, Some(1), "SkiaLabel"), (3, Some(1), "SkiaLabel"), (4, Some(1), "SkiaLabel")]);
    assert_eq!(keys(looped, &tree), [(1, "build/0".to_string())], "the loop's labels match nothing");
    assert_eq!(set(looped, &tree, 2, "font_size", "System.Single", "9"), Err("src-no-code".into()));
    assert_eq!(insert(looped, &tree, 1, 0, &spec("SkiaLabel", &[])), Err("src-insert-unmapped".into()));
    assert_eq!(crate::remove_element(&files(looped), &tree, 2).map_err(|r| r.key), Err("src-no-code".into()));
    // A condition ends the list.
    let cond = "fn build(app: &mut App) -> Build<SkiaLayout> {\n    SkiaLayout::column().children((SkiaLabel::new(\"a\"), if app.on { SkiaLabel::new(\"b\") } else { SkiaLabel::new(\"c\") }))\n}\n";
    let tree = rt(&[(1, None, "SkiaLayout"), (2, Some(1), "SkiaLabel"), (3, Some(1), "SkiaLabel")]);
    assert_eq!(keys(cond, &tree), [(1, "build/0".to_string()), (2, "build/1".to_string())]);
    assert_eq!(insert(cond, &tree, 1, 0, &spec("SkiaLabel", &[])), Err("src-insert-unmapped".into()));
    // A value the code computes.
    let computed = "fn build() -> Build<SkiaLabel> { SkiaLabel::new(\"a\").font_size(SIZE * 2.0) }\n";
    let tree = rt(&[(1, None, "SkiaLabel")]);
    assert_eq!(set(computed, &tree, 1, "font_size", "System.Single", "9"), Err("src-not-literal".into()));
    // `vec![..]` holds one type; a tuple takes 12.
    let v = "fn build() -> Build<SkiaLayout> {\n    SkiaLayout::new().children(vec![SkiaLabel::new(\"a\"), SkiaLabel::new(\"b\")])\n}\n";
    let tree = rt(&[(1, None, "SkiaLayout"), (2, Some(1), "SkiaLabel"), (3, Some(1), "SkiaLabel")]);
    assert_eq!(insert(v, &tree, 1, 0, &spec("SkiaShape", &[])), Err("src-insert-vec-type".into()));
    changed(v, &insert(v, &tree, 1, 2, &spec("SkiaLabel", &[])).unwrap(), "", ", SkiaLabel::new(\"Label\")");
    let twelve = format!("fn build() -> Build<SkiaLayout> {{ SkiaLayout::new().children(({})) }}\n", (0..12).map(|i| format!("SkiaLabel::new(\"{i}\")")).collect::<Vec<_>>().join(", "));
    let mut nodes = vec![(1, None, "SkiaLayout")];
    nodes.extend((2..14).map(|i| (i, Some(1), "SkiaLabel")));
    assert_eq!(insert(&twelve, &rt(&nodes), 1, 0, &spec("SkiaLabel", &[])), Err("src-insert-tuple-full".into()));
}
