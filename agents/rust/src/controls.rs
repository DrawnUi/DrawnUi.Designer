//! The Controls panel and the tree edits: the controls a designer can place, insert (with a pasted
//! copy's properties and children), remove, move to any place in the app's tree, and copy.
//!
//! The engine makes a control from its type name (`designer::create`) and lists those names
//! (`designer::control_types`); this file only adds the panel's group and a new one's defaults.

use designer_protocol::{ControlKind, ControlSpec, SpecProp};
use drawnui::{designer, ControlId, Tree};

/// The panel's look of a type: group, holds children, the properties a new one gets.
type Meta = (&'static str, &'static str, bool, &'static [(&'static str, &'static str)]);

const EMPTY: (&str, &str) = ("background_color", "#22FFFFFF");
const BOX: &[(&str, &str)] = &[("height_request", "80"), ("minimum_width_request", "120"), EMPTY];
const MEDIA: &[(&str, &str)] = &[("width_request", "120"), ("height_request", "80"), EMPTY];

/// Groups as the .NET agent names them; a type the engine adds later shows under Other.
const GROUPS: &[&str] = &["Layout", "Text", "Input", "Media", "Shapes", "Other"];

/// `container`: measured by the agent's test `a_container_is_a_control_that_holds_and_draws_what_goes_into_it`
/// (a drawer slides its one content out of its box, a shell builds its own layers: both stay off).
const META: &[Meta] = &[
    // A stack filling the width, as a new layout is in the .NET designer (`SkiaLayout::new` is absolute).
    ("SkiaLayout", "Layout", true, &[("layout_type", "Column"), ("horizontal_options", "Fill"), ("height_request", "80"), ("minimum_width_request", "120"), EMPTY]),
    ("SkiaScroll", "Layout", false, BOX),
    ("SkiaCarousel", "Layout", true, BOX),
    ("SkiaShaderCarousel", "Layout", true, BOX),
    ("SkiaDrawer", "Layout", false, BOX),
    ("SkiaDecoratedGrid", "Layout", true, BOX),
    ("RescalingLayout", "Layout", true, &[EMPTY]),
    ("SkiaEditor", "Text", false, &[("width_request", "200")]),
    ("SkiaLabel", "Text", false, &[("text", "Label")]),
    ("SkiaLabelFps", "Text", false, &[]),
    ("SkiaRichLabel", "Text", false, &[]),
    ("SkiaButton", "Input", false, &[("text", "Button")]),
    ("SkiaCheckbox", "Input", false, &[]),
    ("SkiaProgress", "Input", false, &[("width_request", "200")]),
    ("SkiaRadioButton", "Input", false, &[("text", "Option")]),
    ("SkiaScrollBar", "Input", false, &[("height_request", "80")]),
    ("SkiaSlider", "Input", false, &[("width_request", "200")]),
    ("SkiaSwitch", "Input", false, &[]),
    ("SkiaGif", "Media", false, MEDIA),
    ("SkiaImage", "Media", false, MEDIA),
    ("SkiaImageTiles", "Media", false, MEDIA),
    ("SkiaLottie", "Media", false, MEDIA),
    ("SkiaSprite", "Media", false, MEDIA),
    ("SkiaSpriteSet", "Media", true, MEDIA),
    ("SkiaSvg", "Media", false, MEDIA),
    ("DrawnGame", "Other", true, BOX),
    ("SkiaShape", "Shapes", true, &[("width_request", "80"), ("height_request", "80"), ("background_color", "#33FFFFFF")]),
];

/// A spec or a tree may name the type with its path (`drawnui::...::SkiaLabel`).
fn short(type_name: &str) -> &str {
    type_name.rsplit([':', '.']).next().unwrap_or(type_name)
}

/// The engine's name for `type_name`, if it can make one.
fn creatable(type_name: &str) -> Option<&'static str> {
    let short = short(type_name);
    designer::control_types().iter().copied().find(|&t| t == short)
}

fn meta(type_name: &str) -> Option<&'static Meta> {
    let short = short(type_name);
    META.iter().find(|m| m.0 == short)
}

/// A control of `type_name` can be made here.
pub fn can_create(type_name: &str) -> bool {
    creatable(type_name).is_some()
}

/// Place mode: the deepest container whose drawn rect holds a canvas point (drawn or not: a
/// layout without a background counts), and the index after its child under the point, or `None`
/// for last. A Rust app has no page wrapper around it: its root is its own code.
pub fn place_target(t: &mut Tree, x: f32, y: f32) -> Option<(ControlId, Option<u32>)> {
    let under = |t: &Tree, id: ControlId| {
        t.base(id).is_some_and(|b| b.p.is_visible) && t.drawn_rect(id).is_some_and(|r| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
    };
    let mut at = t.root().filter(|&r| under(t, r))?;
    let mut found = None;
    loop {
        if !t.is_internal(at) && container(t, at).is_ok() {
            found = Some(at);
        }
        // Down into the child on top (drawn last) under the point.
        match t.children(at).iter().rev().copied().find(|&c| under(t, c)) {
            Some(child) => at = child,
            None => break,
        }
    }
    let found = found?;
    let index = t.cx().app_children(found).iter().rposition(|&c| under(t, c)).map(|i| i as u32 + 1);
    Some((found, index))
}

/// The Controls panel's list: every type the engine makes, by group, by name within.
pub fn kinds() -> Vec<ControlKind> {
    let mut out: Vec<ControlKind> = designer::control_types()
        .iter()
        .map(|&type_name| {
            let (group, container, defaults) = meta(type_name).map_or(("Other", false, &[][..]), |m| (m.1, m.2, m.3));
            ControlKind {
                type_name: type_name.into(),
                group: group.into(),
                own: false,
                defaults: defaults.iter().map(|&(n, v)| (n.into(), v.into())).collect(),
                container,
            }
        })
        .collect();
    let rank = |g: &str| GROUPS.iter().position(|&x| x == g).unwrap_or(GROUPS.len());
    out.sort_by(|a, b| rank(&a.group).cmp(&rank(&b.group)).then_with(|| a.type_name.cmp(&b.type_name)));
    out
}

/// Holds the app's children: a layout or a shape (a scroll holds one content), not a layout that
/// makes its children from items.
fn container(t: &Tree, id: ControlId) -> Result<(), String> {
    match t.type_name(id) {
        Some(name) if t.is_internal(id) => Err(format!("{name} is a part the library made, not in the app's code")),
        Some(name) if t.is_templated(id) => Err(format!("{name} makes its children from items: no control can go into it")),
        Some(name) if items_above(t, id).is_some() => Err(format!("{name} is made from the items of {}: no control can go into it", items_above(t, id).and_then(|l| t.type_name(l)).unwrap_or("a layout"))),
        Some(name) if meta(name).is_some_and(|m| m.2) => Ok(()),
        Some(name) => Err(format!("{name} does not hold children")),
        None => Err("no such control".into()),
    }
}

/// Sets `props` in order (booleans as the engine spells them); what does not take is named in `skipped`.
fn set_all(t: &mut Tree, id: ControlId, props: &[(String, String)], at: &str, skipped: &mut Vec<String>) {
    for (name, value) in props {
        let boolean = t.props(id).iter().any(|p| p.name == name && p.type_name == "bool");
        let value = if boolean { value.to_ascii_lowercase() } else { value.clone() };
        if let Err(e) = t.set_prop(id, name, &value) {
            skipped.push(format!("{at}{name} ({e})"));
        }
    }
}

/// Builds a pasted copy's child (its props, then its own children) last in `parent`.
fn build(t: &mut Tree, parent: ControlId, spec: &ControlSpec, path: &str, skipped: &mut Vec<String>) {
    let at = format!("{path} > {}", short(&spec.type_name));
    let Some(made) = designer::create(short(&spec.type_name)) else {
        skipped.push(format!("{at} (no such control here)"));
        return;
    };
    let id = t.cx().add_child(parent, made);
    let props: Vec<_> = spec.props.iter().map(|p| (p.name.clone(), p.value.clone())).collect();
    set_all(t, id, &props, &format!("{at}."), skipped);
    for child in &spec.children {
        build(t, id, child, &at, skipped);
    }
}

/// A new control of `type_name` in `parent` at `index` among its app children (`None`: last): its
/// defaults, or a pasted copy's props and children. Its id, the index it got, what was skipped.
pub fn insert(t: &mut Tree, parent: ControlId, index: Option<u32>, type_name: &str, props: &[(String, String)], children: &[ControlSpec]) -> Result<(ControlId, u32, Vec<String>), String> {
    container(t, parent)?;
    let name = creatable(type_name).ok_or_else(|| format!("{type_name} cannot be created here"))?;
    let made = designer::create(name).ok_or_else(|| format!("{type_name} cannot be created here"))?;
    let mut cx = t.cx();
    let count = cx.app_children(parent).len();
    let at = index.map_or(count, |i| (i as usize).min(count));
    let id = cx.insert_child(parent, at, made);
    let mut skipped = Vec::new();
    // A paste carries the copy's whole set state: the type's defaults would change it.
    if props.is_empty() && children.is_empty() {
        let defaults: Vec<_> = meta(name).map_or(&[][..], |m| m.3).iter().map(|&(n, v)| (n.to_owned(), v.to_owned())).collect();
        set_all(t, id, &defaults, "", &mut skipped);
    }
    set_all(t, id, props, "", &mut skipped);
    for child in children {
        build(t, id, child, name, &mut skipped);
    }
    Ok((id, at as u32, skipped))
}

/// The children the app put under `id`, in order: not its parts, not what the library made for
/// itself (a button's caption, an editor's text: `Tree::is_internal`, which `Cx::app_children`
/// still lists). A container has none of those (the containers test), so indexes into one match
/// the engine's.
fn app_children(t: &mut Tree, id: ControlId) -> Vec<ControlId> {
    let all = t.cx().app_children(id);
    all.into_iter().filter(|&c| !t.is_internal(c)).collect()
}

/// The layout whose items made a row that is `id` or holds it, if any: every row is the same
/// template code, so nothing inside one is changed one by one.
fn items_above(t: &Tree, id: ControlId) -> Option<ControlId> {
    std::iter::successors(t.parent(id), |&p| t.parent(p)).find(|&p| t.is_templated(p))
}

/// The control, if the app put it there: not the root, not a part of another control, not made
/// from a layout's items (a row or anything inside one).
fn app_child(t: &mut Tree, id: ControlId) -> Result<ControlId, String> {
    let name = t.type_name(id).ok_or("no such control")?.to_owned();
    let parent = t.parent(id).ok_or_else(|| format!("{name} is the canvas's root"))?;
    if let Some(items) = items_above(t, id) {
        return Err(format!("{name} is made from the items of {}, not in the app's code", t.type_name(items).unwrap_or("a layout")));
    }
    if !app_children(t, parent).contains(&id) {
        return Err(format!("{name} is a part of {}, not in the app's code", t.type_name(parent).unwrap_or("its parent")));
    }
    Ok(parent)
}

/// Takes a control out of its parent, live, before the next layout.
pub fn remove(t: &mut Tree, id: ControlId) -> Result<(), String> {
    app_child(t, id)?;
    t.cx().remove(id);
    Ok(())
}

/// Moves a control, with everything inside it, to `parent` (its own or another one) as its app
/// child number `index` (counted without it; past the end: last). Its id, handlers and state stay.
/// The index it got.
pub fn move_to(t: &mut Tree, id: ControlId, parent: ControlId, index: u32) -> Result<u32, String> {
    app_child(t, id)?;
    container(t, parent)?;
    let name = t.type_name(id).unwrap_or("the control").to_owned();
    let mut cx = t.cx();
    if !cx.move_to(id, parent, index as usize) {
        return Err(format!("{name} cannot go into itself or a control inside it"));
    }
    let at = cx.app_children(parent).iter().position(|&c| c == id).ok_or("not among its new parent's children")?;
    Ok(at as u32)
}

/// A control as a copy: its settable properties that are set, and its app children, recursively.
pub fn spec(t: &mut Tree, id: ControlId) -> Option<ControlSpec> {
    let type_name = t.type_name(id)?.to_owned();
    let props = t
        .props(id)
        .into_iter()
        .filter(|p| p.settable && p.is_set)
        .map(|p| {
            let typed = crate::agent::prop(p.clone()).type_name;
            SpecProp { name: p.name.into(), value: p.value, type_name: typed }
        })
        .collect();
    let children = app_children(t, id).into_iter().filter_map(|c| spec(t, c)).collect();
    Some(ControlSpec { type_name, props, children })
}
