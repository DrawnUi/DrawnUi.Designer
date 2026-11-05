//! Adding a control to the code that makes a parent's children, and taking one out.

use std::collections::HashMap;

use designer_protocol::{ControlSpec, NodeId, Refusal, Tree as RuntimeTree};

use super::*;
use crate::setcode::{CodeEdit, PropValue, byte_of, color_parse_call, thickness_args, value_from_runtime};

/// Writes `new T { defaults }` into the code that makes `parent`'s children, as child `index` among
/// the parent's runtime children (`index` past the end appends). `defaults` are (property, value
/// text) as the app sets them live; `types` gives each property's .NET type (as `getProps` reports
/// it), so the value is spelled the way [`set_property`] spells it. The parent's children must come
/// one by one from a list in the code (`Children = { .. }`, `new List<SkiaControl> { .. }`,
/// `.WithChildren(..)`); a parent with no children yet gets `Children = { .. }`.
pub fn insert_element(
    files: &[(String, String)],
    tree: &RuntimeTree,
    parent: NodeId,
    index: usize,
    type_name: &str,
    defaults: &[(String, String)],
    types: &[(String, String)],
) -> Result<CodeEdit, Refusal> {
    if let Some(r) = tsx_insert(files, tree, parent, index, crate::tsx::NewControl::Defaults { type_name, defaults, types }) {
        return r.map(|s| s.finish(files));
    }
    insert_with(files, tree, parent, index, |_| new_element(type_name, defaults, types)).map(|s| s.finish(files))
}

/// [`insert_element`] for a copy with everything inside it, each value spelled by the type the spec
/// gives it (the root's from `root_types` where the spec has none). Short and without children it is
/// one line (`new T { A = 1, B = 2 }`); else one property per line and its children inside
/// `Children = { .. }`, one per line, indented as the code around it. `forms` gives, by a spec node's
/// path of child indexes from the root, the form its original code writes its list in
/// (`new List<SkiaControl>`); without one, the collection initializer `{ .. }`.
pub fn insert_spec(files: &[(String, String)], tree: &RuntimeTree, parent: NodeId, index: usize, spec: &ControlSpec, root_types: &[(String, String)], forms: &HashMap<Vec<usize>, String>) -> Result<CodeEdit, Refusal> {
    if let Some(r) = tsx_insert(files, tree, parent, index, crate::tsx::NewControl::Spec { spec, root_types }) {
        return r.map(|s| s.finish(files));
    }
    insert_with(files, tree, parent, index, |style| spec_element(spec, root_types, style, &mut vec![], forms)).map(|s| s.finish(files))
}

/// How the code that makes each of `nodes` writes its children list: `""` for the collection
/// initializer `Children = { .. }`, else the list it creates (`new List<SkiaControl>`). Nodes with no
/// such list in code are left out.
pub fn children_forms(files: &[(String, String)], tree: &RuntimeTree, nodes: &[NodeId]) -> HashMap<NodeId, String> {
    let mut out = HashMap::new();
    for m in match_runtime(files, tree).into_iter().filter(|m| nodes.contains(&m.node) && !crate::tsx::is_tsx(&m.uri)) {
        let Some((_, text)) = files.iter().find(|(u, _)| *u == m.uri) else { continue };
        let ts = parse(text);
        let (src, handles) = walk(&ts, text, &m.uri);
        let Some(e) = src.nodes.iter().position(|n| n.key == m.key) else { continue };
        let Some(a) = (e + 1..src.nodes.len()).find(|&j| src.nodes[j].parent == Some(e) && src.nodes[j].role == SourceRole::Attribute && src.nodes[j].name.as_deref() == Some("Children")) else { continue };
        let assign = handles[a];
        let Some(value) = assign.child_by_field_name("right").filter(|_| assign.kind() == "assignment_expression") else { continue };
        let form = if is_creation(value) { text[value.start_byte()..initializer(value).map_or(value.end_byte(), |i| i.start_byte())].trim().to_owned() } else { String::new() };
        out.insert(m.node, form);
    }
    out
}

/// How a new element is laid out where it goes: one line, or lines at `indent` (its first line's
/// indent) going one `unit` deeper per level.
#[derive(Clone)]
struct Style {
    one_line: bool,
    indent: String,
    unit: String,
    nl: &'static str,
}

impl Style {
    fn one_line() -> Style {
        Style { one_line: true, indent: String::new(), unit: "    ".into(), nl: "\n" }
    }
}

/// The insert for a parent made by JSX (`None`: no TSX file, or the parent is not made by one).
fn tsx_insert(files: &[(String, String)], tree: &RuntimeTree, parent: NodeId, index: usize, new: crate::tsx::NewControl) -> Option<Result<Splice, Refusal>> {
    if !files.iter().any(|(u, _)| crate::tsx::is_tsx(u)) {
        return None;
    }
    let matches: HashMap<NodeId, RuntimeMatch> = match_runtime(files, tree).into_iter().map(|m| (m.node, m)).collect();
    let m = matches.get(&parent)?;
    if !crate::tsx::is_tsx(&m.uri) {
        return None;
    }
    if tree.nodes.iter().any(|n| n.parent == Some(parent) && n.template_index.is_some()) {
        return Some(Err(Refusal::new("src-insert-templated", &[])));
    }
    Some(crate::tsx::insert(files, tree, &matches, parent, index, new))
}

/// Places the element `element` makes as child `index` of `parent`'s code (see [`insert_element`]).
fn insert_with(files: &[(String, String)], tree: &RuntimeTree, parent: NodeId, index: usize, element: impl Fn(&Style) -> Result<String, Refusal>) -> Result<Splice, Refusal> {
    let rn = tree.nodes.iter().find(|n| n.id == parent).ok_or_else(|| Refusal::new("src-not-in-tree", &[]))?;
    if rn.internal {
        return Err(Refusal::new("src-library-part", &[]));
    }
    let kids: Vec<NodeId> = tree.nodes.iter().filter(|n| n.parent == Some(parent) && !n.internal).map(|n| n.id).collect();
    if tree.nodes.iter().any(|n| n.parent == Some(parent) && n.template_index.is_some()) {
        return Err(Refusal::new("src-insert-templated", &[]));
    }
    let matches: HashMap<NodeId, RuntimeMatch> = match_runtime(files, tree).into_iter().map(|m| (m.node, m)).collect();
    let m = matches.get(&parent).ok_or_else(|| Refusal::new("src-no-code", &[]))?;

    let text = files.iter().find(|(u, _)| *u == m.uri).expect("matched file").1.as_str();
    let ts = parse(text);
    let (src, handles) = walk(&ts, text, &m.uri);
    let e = src.nodes.iter().position(|n| n.key == m.key).ok_or_else(|| Refusal::new("src-key-missing", &[("key", &m.key)]))?;
    let attr = |name: &str| (e + 1..src.nodes.len()).find(|&j| src.nodes[j].parent == Some(e) && src.nodes[j].role == SourceRole::Attribute && src.nodes[j].name.as_deref() == Some(name));
    if attr("ItemTemplate").is_some() {
        return Err(Refusal::new("src-insert-templated", &[]));
    }
    let h = handles[e];
    let keys: HashMap<usize, &str> = (0..src.nodes.len()).filter(|&i| src.nodes[i].role == SourceRole::Element).map(|i| (handles[i].id(), src.nodes[i].key.as_str())).collect();

    // Where the children are written: the container (`{ .. }`, `[ .. ]`, `( .. )`) and its items.
    // ponytail: a parent with no list yet gets its new element on one line; lay it out if that reads badly.
    let (container, items, attr_key): (Option<Node>, Vec<Node>, Option<String>) = if let Some(a) = attr("Children") {
        let assign = handles[a];
        if assign.kind() != "assignment_expression" {
            // `Children.Add(..)` / `AddSubView(..)` calls in a constructor.
            return Err(Refusal::new("src-insert-no-list", &[]));
        }
        let value = assign.child_by_field_name("right").ok_or_else(|| Refusal::new("src-insert-no-list", &[]))?;
        match value_items(value) {
            Some((c, its)) => (Some(c), its, Some(src.nodes[a].key.clone())),
            // `new List<SkiaControl>()`: no initializer yet.
            None if is_creation(value) && initializer(value).is_none() => {
                if !kids.is_empty() {
                    return Err(Refusal::new("src-insert-unmapped", &[]));
                }
                let element = element(&Style::one_line())?;
                let at = value.end_byte();
                return Ok(Splice::new(&m.uri, at, at, format!(" {{ {element} }}"), &element));
            }
            None => return Err(Refusal::new("src-insert-no-list", &[])),
        }
    } else if let Some(call) = fluent_children(text, h) {
        let args = call.child_by_field_name("arguments").ok_or_else(|| Refusal::new("src-insert-no-list", &[]))?;
        (Some(args), call_args(call), None)
    } else {
        // No children in the code: add them, when the runtime parent has none either.
        if !kids.is_empty() {
            return Err(Refusal::new("src-insert-unmapped", &[]));
        }
        let element = element(&Style::one_line())?;
        if h.kind() == "constructor_declaration" {
            let value = format!("new List<SkiaControl> {{ {element} }}");
            let (a, b, t) = add_assignment(text, &src, &handles, e, "Children", &value).map_err(|_| Refusal::new("src-insert-no-list", &[]))?;
            return Ok(Splice::new(&m.uri, a, b, t, &element));
        }
        if initializer(h).is_none() {
            return Err(Refusal::new("src-insert-no-initializer", &[("key", &m.key)]));
        }
        let value = format!("{{ {element} }}");
        let (a, b, t) = add_assignment(text, &src, &handles, e, "Children", &value).map_err(|_| Refusal::new("src-insert-no-list", &[]))?;
        return Ok(Splice::new(&m.uri, a, b, t, &element));
    };
    let container = container.expect("a children container");

    // Each runtime child must be one item of the list, in order: then the runtime index is the
    // list index. A loop or a helper that makes several children does not line up.
    let item_key = |i: usize, item: Node| -> Option<String> {
        if let Some(el) = item_element(item) {
            return keys.get(&el.id()).map(|k| k.to_string());
        }
        attr_key.as_ref().map(|a| format!("{a}/{i}"))
    };
    let item_keys: Vec<Option<String>> = items.iter().enumerate().map(|(i, it)| item_key(i, *it)).collect();
    let lined_up = kids.len() == items.len()
        && kids.iter().zip(&item_keys).all(|(kid, key)| {
            let (Some(km), Some(key)) = (matches.get(kid), key) else { return false };
            km.uri == m.uri && (km.key == *key || km.via.iter().any(|v| v == key))
        });
    if !lined_up {
        return Err(Refusal::new("src-insert-unmapped", &[]));
    }

    let lines = Lines::new(text);
    let nl = newline(text);
    let i = index.min(items.len());
    if items.is_empty() {
        let element = element(&Style::one_line())?;
        let open = container.child(0).expect("open token");
        let close = container.child(container.child_count() - 1).expect("close token");
        let t = if open.kind() == "{" { format!(" {element} ") } else { element.clone() };
        return Ok(Splice::new(&m.uri, open.end_byte(), close.start_byte(), t, &element));
    }
    let multiline = items.iter().any(|it| lines.line_of(it.start_byte()) != lines.line_of(container.start_byte()));
    let indent = line_indent(text, &lines, items[i.min(items.len() - 1)].start_byte()).to_string();
    let unit = if indent.contains('\t') { "\t".to_owned() } else { "    ".to_owned() };
    let element = element(&Style { one_line: !multiline, indent: indent.clone(), unit, nl })?;
    let (at, t) = if i < items.len() {
        let t = if multiline { format!("{element},{nl}{indent}") } else { format!("{element}, ") };
        (items[i].start_byte(), t)
    } else {
        let last = items[items.len() - 1];
        match (comma_after(last), multiline) {
            (Some(c), true) => (c.end_byte(), format!("{nl}{indent}{element},")),
            (None, true) => (last.end_byte(), format!(",{nl}{indent}{element}")),
            (Some(c), false) => (c.end_byte(), format!(" {element},")),
            (None, false) => (last.end_byte(), format!(", {element}")),
        }
    };
    Ok(Splice::new(&m.uri, at, at, t, &element))
}

/// Takes runtime control `node` out of the code: the element with its separator, when it is an
/// item of a list of children.
pub fn remove_element(files: &[(String, String)], tree: &RuntimeTree, node: NodeId) -> Result<CodeEdit, Refusal> {
    let rn = tree.nodes.iter().find(|n| n.id == node).ok_or_else(|| Refusal::new("src-not-in-tree", &[]))?;
    if rn.internal {
        return Err(Refusal::new("src-library-part", &[]));
    }
    if rn.template_index.is_some() {
        return Err(Refusal::new("src-insert-templated", &[]));
    }
    let m = match_runtime(files, tree).into_iter().find(|m| m.node == node).ok_or_else(|| Refusal::new("src-no-code", &[]))?;
    if crate::tsx::is_tsx(&m.uri) {
        return crate::tsx::remove(files, &m);
    }
    let text = files.iter().find(|(u, _)| *u == m.uri).expect("matched file").1.as_str();
    let src = read_csharp(&m.uri, text);
    let answer = edit_csharp(text, &src, &m.key, &SourceEdit::Remove);
    if answer.refused.is_some() {
        return Err(Refusal::new("src-remove-not-in-list", &[("key", &m.key)]));
    }
    let [e] = answer.edits.as_slice() else { return Err(Refusal::new("src-not-single", &[])) };
    let (a, b) = (byte_of(text, e.range.start), byte_of(text, e.range.end));
    Ok(finish(&m.uri, text, a, b, e.new_text.clone(), ""))
}

/// `new T { A = 1, B = "x" }` (or `new T()`), each value spelled for its property's type.
fn new_element(type_name: &str, defaults: &[(String, String)], types: &[(String, String)]) -> Result<String, Refusal> {
    let mut parts = vec![];
    for (name, value) in defaults {
        let t = types.iter().find(|(n, _)| n == name).map(|(_, t)| t.as_str()).ok_or_else(|| Refusal::new("src-insert-type-unknown", &[("prop", name)]))?;
        parts.push(format!("{name} = {}", spell_value(t, value)?));
    }
    Ok(creation(type_name, parts))
}

/// A copy's element laid out by `style`: on one line when short (under 100 characters, its indent
/// not counted) and without children, or where the list keeps its items on one line; else one
/// property per line, then `Children = <list> {` with each child on its own line one level deeper.
fn spec_element(spec: &ControlSpec, root_types: &[(String, String)], style: &Style, path: &mut Vec<usize>, forms: &HashMap<Vec<usize>, String>) -> Result<String, Refusal> {
    // The list's form: the original code's, else the collection initializer.
    let list = forms.get(path.as_slice()).cloned().unwrap_or_default();
    let head = if list.is_empty() { "Children =".to_owned() } else { format!("Children = {list}") };
    let mut kid = |i: usize, c: &ControlSpec, st: &Style| {
        path.push(i);
        let r = spec_element(c, &[], st, path, forms);
        path.pop();
        r
    };
    let mut parts = vec![];
    for p in &spec.props {
        let t = if p.type_name.is_empty() { root_types.iter().find(|(n, _)| *n == p.name).map(|(_, t)| t.as_str()) } else { Some(p.type_name.as_str()) };
        let t = t.ok_or_else(|| Refusal::new("src-insert-type-unknown", &[("prop", &p.name)]))?;
        parts.push(format!("{} = {}", p.name, spell_value(t, &p.value)?));
    }
    if spec.children.is_empty() {
        let line = creation(&spec.type_name, parts.clone());
        if style.one_line || line.chars().count() < 100 {
            return Ok(line);
        }
    } else if style.one_line {
        let kids: Vec<String> = spec.children.iter().enumerate().map(|(i, c)| kid(i, c, style)).collect::<Result<_, _>>()?;
        parts.push(format!("{head} {{ {} }}", kids.join(", ")));
        return Ok(creation(&spec.type_name, parts));
    }
    let (nl, base) = (style.nl, &style.indent);
    let inner = format!("{base}{}", style.unit);
    let deeper = Style { indent: format!("{inner}{}", style.unit), ..style.clone() };
    let short = spec.type_name.split(['`', '<']).next().unwrap_or(&spec.type_name).rsplit(['.', '+']).next().unwrap_or(&spec.type_name);
    let mut out = format!("new {short}{nl}{base}{{");
    for p in parts {
        out.push_str(&format!("{nl}{inner}{p},"));
    }
    if !spec.children.is_empty() {
        out.push_str(&format!("{nl}{inner}{head}{nl}{inner}{{"));
        for (i, c) in spec.children.iter().enumerate() {
            out.push_str(&format!("{nl}{}{},", deeper.indent, kid(i, c, &deeper)?));
        }
        out.push_str(&format!("{nl}{inner}}},"));
    }
    out.push_str(&format!("{nl}{base}}}"));
    Ok(out)
}

/// `new T { a, b }`, or `new T()` with nothing to set.
fn creation(type_name: &str, parts: Vec<String>) -> String {
    let short = type_name.split(['`', '<']).next().unwrap_or(type_name).rsplit(['.', '+']).next().unwrap_or(type_name);
    if parts.is_empty() { format!("new {short}()") } else { format!("new {short} {{ {} }}", parts.join(", ")) }
}

/// A value as C# writes it for a property of type `t`.
fn spell_value(t: &str, value: &str) -> Result<String, Refusal> {
    Ok(match value_from_runtime(t, value, value)? {
        PropValue::Plain(v) => spell(&v).map_err(|_| Refusal::new("src-type-unsupported", &[("type", &t)]))?,
        PropValue::Color(c) => color_parse_call(c),
        PropValue::Thickness(th) => format!("new Thickness({})", thickness_args(th)),
    })
}

/// `.WithChildren(..)` in the fluent chain on an element.
fn fluent_children<'t>(text: &str, creation: Node<'t>) -> Option<Node<'t>> {
    if !is_creation(creation) {
        return None;
    }
    let top = chain_top(creation);
    let mut n = creation;
    while n != top {
        let p = n.parent()?;
        if p.kind() == "invocation_expression" && p.child_by_field_name("function").and_then(|f| f.child_by_field_name("name")).is_some_and(|x| &text[x.byte_range()] == "WithChildren") {
            return Some(p);
        }
        n = p;
    }
    None
}

/// The file with `new_text` put in place of bytes `a..b`; the range is `element` inside it (the
/// inserted control, without its separator), or an empty range at `a` for a removal.
fn finish(uri: &str, text: &str, a: usize, b: usize, new_text: String, element: &str) -> CodeEdit {
    let mut new = text.to_string();
    new.replace_range(a..b, &new_text);
    let lines = Lines::new(&new);
    let range = match new_text.find(element).filter(|_| !element.is_empty()) {
        Some(skip) => lines.range(a + skip, a + skip + element.len()),
        None => lines.range(a, a),
    };
    CodeEdit { uri: uri.into(), text: new, range }
}

/// One replacement in one file: bytes `a..b` become `text`, with the written control `element` in it.
pub(crate) struct Splice {
    uri: String,
    a: usize,
    b: usize,
    text: String,
    element: String,
}

impl Splice {
    pub(crate) fn new(uri: &str, a: usize, b: usize, text: String, element: &str) -> Splice {
        Splice { uri: uri.into(), a, b, text, element: element.into() }
    }

    fn finish(self, files: &[(String, String)]) -> CodeEdit {
        let text = files.iter().find(|(u, _)| *u == self.uri).expect("matched file").1.as_str();
        finish(&self.uri, text, self.a, self.b, self.text, &self.element)
    }
}

/// Moves runtime control `node` in the code to child `index` of `parent` (`index` counted with the
/// control taken out). Its own text moves, comments and layout inside it kept, its continuation lines
/// re-indented to the new place; it leaves its list the way [`remove_element`] takes a control out
/// and lands the way [`insert_element`] writes one (separators, one per line, a new list for a parent
/// without children). Both lists must be in the same file. The answer is one edit: the whole new
/// file, its range the moved control.
pub fn move_element(files: &[(String, String)], tree: &RuntimeTree, node: NodeId, parent: NodeId, index: u32) -> Result<CodeEdit, Refusal> {
    let index = index as usize;
    let rn = tree.nodes.iter().find(|n| n.id == node).ok_or_else(|| Refusal::new("src-not-in-tree", &[]))?;
    if rn.internal {
        return Err(Refusal::new("src-library-part", &[]));
    }
    if rn.template_index.is_some() {
        return Err(Refusal::new("src-insert-templated", &[]));
    }
    let mut up = Some(parent);
    while let Some(x) = up {
        if x == node {
            return Err(Refusal::new("src-move-into-itself", &[]));
        }
        up = tree.nodes.iter().find(|n| n.id == x).and_then(|n| n.parent);
    }
    let matches = match_runtime(files, tree);
    let m = matches.iter().find(|m| m.node == node).ok_or_else(|| Refusal::new("src-no-code", &[]))?;
    let pm = matches.iter().find(|m| m.node == parent).ok_or_else(|| Refusal::new("src-no-code", &[]))?;
    if pm.uri != m.uri {
        return Err(Refusal::new("src-move-other-file", &[]));
    }
    let text = files.iter().find(|(u, _)| *u == m.uri).expect("matched file").1.as_str();
    // The list item that makes the control: its own element, or the nearest use that makes it
    // (`<Card ..>`; `via` runs from the outermost use in).
    let (ra, rb, fill, moved) = std::iter::once(&m.key).chain(m.via.iter().rev()).find_map(|k| cut(&m.uri, text, k)).ok_or_else(|| Refusal::new("src-move-not-in-list", &[("key", &m.key)]))?;

    // Where it goes, as an index among the parent's children with the control still there.
    let lines = Lines::new(text);
    let mut at = index;
    if rn.parent == Some(parent) {
        let kids: Vec<NodeId> = tree.nodes.iter().filter(|n| n.parent == Some(parent) && !n.internal).map(|n| n.id).collect();
        let from = kids.iter().position(|&k| k == node).expect("a child of its parent");
        let to = index.min(kids.len() - 1);
        if to == from {
            return Ok(CodeEdit { uri: m.uri.clone(), text: text.to_string(), range: lines.range(moved.start, moved.end) });
        }
        at = if to > from { to + 1 } else { to };
    }
    const MARK: &str = "\u{1}moved\u{1}";
    let ins = if crate::tsx::is_tsx(&m.uri) {
        tsx_insert(files, tree, parent, at, crate::tsx::NewControl::Text(MARK)).unwrap_or_else(|| Err(Refusal::new("src-no-code", &[])))?
    } else {
        insert_with(files, tree, parent, at, |_| Ok(MARK.to_owned()))?
    };
    // ponytail: a list item's cut and an insert among other items never overlap; refuse if they do.
    if ins.a < rb && (ins.a > ra || ins.b > ra) {
        return Err(Refusal::new("src-insert-unmapped", &[]));
    }

    // The moved text re-indented: its first line lands where the insert puts it, the others follow.
    let mark = ins.text.find(MARK).expect("the mark");
    let before = &ins.text[..mark];
    let indent = match before.rfind('\n') {
        Some(k) if before[k + 1..].trim().is_empty() => &before[k + 1..],
        _ => line_indent(text, &lines, ins.a),
    };
    let old = line_indent(text, &lines, moved.start);
    let body = text[moved.clone()]
        .split('\n')
        .enumerate()
        .map(|(k, l)| match l.strip_prefix(old) {
            Some(rest) if k > 0 => format!("{indent}{rest}"),
            _ => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let put = ins.text.replacen(MARK, &body, 1);

    let mut new = text.to_string();
    let start = if ins.a >= rb {
        new.replace_range(ins.a..ins.b, &put);
        new.replace_range(ra..rb, &fill);
        ins.a - (rb - ra) + fill.len() + mark
    } else {
        new.replace_range(ra..rb, &fill);
        new.replace_range(ins.a..ins.b, &put);
        ins.a + mark
    };
    let range = Lines::new(&new).range(start, start + body.len());
    Ok(CodeEdit { uri: m.uri.clone(), text: new, range })
}

/// The cut that takes list item `key` out of its list (bytes `a..b`, what stays there) and the
/// item's own bytes; `None` when `key` is no item of a list of children.
fn cut(uri: &str, text: &str, key: &str) -> Option<(usize, usize, String, std::ops::Range<usize>)> {
    if crate::tsx::is_tsx(uri) {
        return crate::tsx::cut(uri, text, key);
    }
    let ts = parse(text);
    let (src, handles) = walk(&ts, text, uri);
    let e = src.nodes.iter().position(|n| n.key == key)?;
    if src.nodes[e].role != SourceRole::Element {
        return None;
    }
    let (container, item) = collection_slot(chain_top(handles[e]))?;
    let (a, b, fill) = remove_item(container, item);
    Some((a, b, fill, item.byte_range()))
}
