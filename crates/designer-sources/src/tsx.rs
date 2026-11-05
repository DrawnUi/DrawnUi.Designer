//! The TSX side: DrawnUi.React apps. Reads JSX into the same [`SourceTree`] shape as C# (elements,
//! their attributes as properties, their JSX children under a `Children` attribute), answers
//! [`SourceEdit`]s with text edits, matches runtime controls to JSX, and writes values, inserted
//! and removed controls the TSX way: strings in quotes, numbers and booleans in braces, enum cases
//! as strings (`HorizontalOptions="Center"`), colors as `"#RRGGBB"`, thickness as a number or
//! `"12,6"`.

use std::collections::{HashMap, HashSet};

use designer_protocol::{ControlSpec, NodeId, Refusal, Tree as RuntimeTree};
use tree_sitter::{Node, Parser, Tree};

use super::*;
use crate::setcode::{CodeEdit, PropValue, color_hex, value_from_runtime};

/// Whether a file is read as TSX (React): `.tsx`, `.jsx`.
pub fn is_tsx(uri: &str) -> bool {
    let u = uri.to_ascii_lowercase();
    u.ends_with(".tsx") || u.ends_with(".jsx")
}

fn parse_tsx(text: &str) -> Tree {
    let mut p = Parser::new();
    p.set_language(&tree_sitter_typescript::LANGUAGE_TSX.into()).expect("tree-sitter-typescript ABI");
    p.parse(text, None).expect("parser without timeout always returns a tree")
}

/// Reads a TSX file into its source tree. Keys: `Scope/i` for the i-th JSX element of a function
/// (`RootPage/0`, `<module>/0` at the top level) that is not inside another one, `<element>/Prop`
/// for an attribute, `<attribute>/=` for its value, `<element>/Children/i` for its i-th JSX child.
pub fn read_tsx(uri: &str, text: &str) -> SourceTree {
    let ts = parse_tsx(text);
    walk_tsx(&ts, text, uri).0
}

/// [`edit_csharp`] for TSX: SetValue / AddProperty / Remove on attributes, Remove on a JSX child.
pub fn edit_tsx(text: &str, tree: &SourceTree, key: &str, edit: &SourceEdit) -> SourceEditAnswer {
    if !tree.nodes.iter().any(|n| n.key == key) {
        return refuse(SourceEditRefusal::UnknownNode);
    }
    let ts = parse_tsx(text);
    let (src, handles) = walk_tsx(&ts, text, &tree.uri);
    let Some(i) = src.nodes.iter().position(|n| n.key == key) else { return refuse(SourceEditRefusal::UnknownNode) };
    let lines = Lines::new(text);
    match plan_tsx(text, &src, &handles, i, edit) {
        Ok((a, b, new_text)) => SourceEditAnswer { edits: vec![TextEdit { range: lines.range(a, b), new_text }], refused: None },
        Err(r) => refuse(r),
    }
}

// ---------------------------------------------------------------- reading

struct TsxWalker<'t> {
    text: &'t str,
    lines: Lines<'t>,
    nodes: Vec<SourceNode>,
    handles: Vec<Node<'t>>,
    keys: HashSet<String>,
    scopes: Vec<String>,
    counters: HashMap<String, usize>,
}

fn walk_tsx<'t>(tree: &'t Tree, text: &'t str, uri: &str) -> (SourceTree, Vec<Node<'t>>) {
    let mut w = TsxWalker { text, lines: Lines::new(text), nodes: vec![], handles: vec![], keys: HashSet::new(), scopes: vec![], counters: HashMap::new() };
    w.scan(tree.root_node());
    (SourceTree { uri: uri.into(), nodes: w.nodes }, w.handles)
}

/// A JSX element: `<T ..>..</T>` or `<T .. />`; a fragment `<>..</>` is not one.
fn is_jsx(n: Node) -> bool {
    match n.kind() {
        "jsx_self_closing_element" => true,
        "jsx_element" => open_tag(n).and_then(|o| o.child_by_field_name("name")).is_some(),
        _ => false,
    }
}

fn is_fragment(n: Node) -> bool {
    n.kind() == "jsx_element" && open_tag(n).and_then(|o| o.child_by_field_name("name")).is_none()
}

/// The node that holds a JSX element's name and attributes.
fn open_tag(n: Node) -> Option<Node> {
    match n.kind() {
        "jsx_element" => n.child_by_field_name("open_tag"),
        "jsx_self_closing_element" => Some(n),
        _ => None,
    }
}

fn attributes(n: Node) -> Vec<Node> {
    let Some(o) = open_tag(n) else { return vec![] };
    let mut c = o.walk();
    o.children_by_field_name("attribute", &mut c).collect()
}

/// The JSX children that make controls: elements, fragments and `{ expressions }` (not comments,
/// not text).
fn jsx_children(n: Node) -> Vec<Node> {
    if n.kind() != "jsx_element" {
        return vec![];
    }
    let mut c = n.walk();
    n.named_children(&mut c)
        .filter(|k| match k.kind() {
            "jsx_element" | "jsx_self_closing_element" => true,
            "jsx_expression" => expression_of(*k).is_some(),
            _ => false,
        })
        .collect()
}

/// The expression inside `{ .. }`, none for a comment-only `{/* .. */}`.
fn expression_of(e: Node) -> Option<Node> {
    let mut c = e.walk();
    e.named_children(&mut c).find(|x| x.kind() != "comment")
}

fn tag_name<'a>(n: Node, text: &'a str) -> Option<&'a str> {
    open_tag(n)?.child_by_field_name("name").map(|x| &text[x.byte_range()])
}

/// A literal attribute value: `"x"`, or `{12}`, `{-4}`, `{true}`, `{"x"}`, a template without
/// substitutions, `{Colors.White}` (a capitalized member).
fn is_literal_value(v: Node, text: &str) -> bool {
    if v.kind() == "string" {
        return true;
    }
    let Some(e) = (v.kind() == "jsx_expression").then(|| expression_of(v)).flatten() else { return false };
    match e.kind() {
        "number" | "string" | "true" | "false" => true,
        "template_string" => !text[e.byte_range()].contains("${"),
        "unary_expression" => e.child(0).is_some_and(|op| op.kind() == "-") && e.named_child(0).is_some_and(|x| x.kind() == "number"),
        "member_expression" => {
            let (Some(o), Some(p)) = (e.child_by_field_name("object"), e.child_by_field_name("property")) else { return false };
            o.kind() == "identifier" && [o, p].iter().all(|x| text[x.byte_range()].starts_with(char::is_uppercase))
        }
        _ => false,
    }
}

impl<'t> TsxWalker<'t> {
    fn push(&mut self, role: SourceRole, n: Node<'t>, parent: Option<usize>, key: String, name: Option<String>, value: Option<String>) -> (usize, String) {
        let mut key = key;
        if self.keys.contains(&key) {
            let mut k = 2;
            while self.keys.contains(&format!("{key}#{k}")) {
                k += 1;
            }
            key = format!("{key}#{k}");
        }
        self.keys.insert(key.clone());
        self.nodes.push(SourceNode { role, range: self.lines.range(n.start_byte(), n.end_byte()), parent, key: key.clone(), name, value });
        self.handles.push(n);
        (self.nodes.len() - 1, key)
    }

    fn scope_key(&self) -> String {
        if self.scopes.is_empty() { "<module>".into() } else { self.scopes.join(".") }
    }

    /// The name a function, a component or a class gives to the elements inside it.
    fn scope_name(&self, n: Node) -> Option<String> {
        let name = |x: Node| Some(self.text[x.byte_range()].to_string());
        match n.kind() {
            "function_declaration" | "generator_function_declaration" | "class_declaration" | "method_definition" => name(n.child_by_field_name("name")?),
            "variable_declarator" => {
                let v = n.child_by_field_name("value")?;
                matches!(v.kind(), "arrow_function" | "function_expression" | "function").then(|| name(n.child_by_field_name("name")?)).flatten()
            }
            _ => None,
        }
    }

    fn scan_children(&mut self, n: Node<'t>) {
        let mut c = n.walk();
        for k in n.children(&mut c).collect::<Vec<_>>() {
            self.scan(k);
        }
    }

    fn scan(&mut self, n: Node<'t>) {
        if let Some(name) = self.scope_name(n) {
            self.scopes.push(name);
            self.scan_children(n);
            self.scopes.pop();
        } else if is_jsx(n) {
            let scope = self.scope_key();
            let i = self.counters.entry(scope.clone()).or_default();
            let key = format!("{scope}/{i}");
            *i += 1;
            self.element(n, None, key);
        } else {
            self.scan_children(n);
        }
    }

    fn element(&mut self, n: Node<'t>, parent: Option<usize>, key: String) {
        let name = tag_name(n, self.text).map(String::from);
        let (idx, key) = self.push(SourceRole::Element, n, parent, key, name, None);
        for a in attributes(n) {
            if a.kind() == "jsx_attribute" {
                self.attribute(a, idx, &key);
            } else {
                self.scan(a); // `{...props}`
            }
        }
        let kids = jsx_children(n);
        if let (Some(first), Some(last)) = (kids.first(), kids.last()) {
            let (aidx, akey) = self.push(SourceRole::Attribute, n, Some(idx), format!("{key}/Children"), Some("Children".into()), None);
            self.nodes[aidx].range = self.lines.range(first.start_byte(), last.end_byte());
            for (i, k) in kids.iter().enumerate() {
                if is_jsx(*k) {
                    self.element(*k, Some(aidx), format!("{akey}/{i}"));
                } else {
                    self.push(SourceRole::Reference, *k, Some(aidx), format!("{akey}/{i}"), None, None);
                    self.scan_children(*k);
                }
            }
        }
    }

    fn attribute(&mut self, a: Node<'t>, parent: usize, elem_key: &str) {
        let Some(name_node) = a.named_child(0) else { return };
        let name = self.text[name_node.byte_range()].to_string();
        let (aidx, akey) = self.push(SourceRole::Attribute, a, Some(parent), format!("{elem_key}/{name}"), Some(name), None);
        match a.named_child(1) {
            // `<X IsVisible />`: true.
            None => {
                self.push(SourceRole::Literal, a, Some(aidx), format!("{akey}/="), None, Some("true".into()));
            }
            Some(v) if is_literal_value(v, self.text) => {
                let value = self.text[v.byte_range()].to_string();
                self.push(SourceRole::Literal, v, Some(aidx), format!("{akey}/="), None, Some(value));
            }
            Some(v) if is_jsx(v) => self.element(v, Some(aidx), format!("{akey}/0")),
            Some(v) => {
                self.push(SourceRole::Reference, v, Some(aidx), format!("{akey}/="), None, None);
                self.scan_children(v);
            }
        }
    }
}

// ---------------------------------------------------------------- spelling

/// A JS string literal.
fn js_string(s: &str) -> String {
    let mut out = String::from('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// An attribute value as JSX writes it: `"text"` (`{"..."}` when it needs escapes), `{12}`,
/// `{true}`, `"Center"` for a case.
pub(crate) fn spell_attr(v: &SourceEditValue) -> Option<String> {
    Some(match v {
        SourceEditValue::String { value } if value.chars().any(|c| matches!(c, '"' | '\\' | '\n' | '\r' | '\t') || c.is_control()) => format!("{{{}}}", js_string(value)),
        SourceEditValue::String { value } => format!("\"{value}\""),
        SourceEditValue::Number { value } if value.is_finite() => format!("{{{value}}}"),
        SourceEditValue::Number { .. } => return None,
        SourceEditValue::Boolean { value } => format!("{{{value}}}"),
        SourceEditValue::Case { name } => format!("\"{}\"", name.rsplit('.').next().unwrap_or(name)),
    })
}

fn thickness_text([l, t, r, b]: [f64; 4]) -> String {
    if l == t && t == r && r == b {
        format!("{l}")
    } else if l == r && t == b {
        format!("{l},{t}")
    } else {
        format!("{l},{t},{r},{b}")
    }
}

/// A runtime value as a new attribute value.
fn spell_prop(value: &PropValue) -> Option<String> {
    Some(match value {
        PropValue::Plain(v) => spell_attr(v)?,
        PropValue::Color(c) => format!("\"#{}\"", color_hex(*c, None, true)),
        PropValue::Thickness(t) if t.iter().all(|x| *x == t[0]) => format!("{{{}}}", t[0]),
        PropValue::Thickness(t) => format!("\"{}\"", thickness_text(*t)),
    })
}

/// A runtime value replacing the value node `old` (a `"string"` or a `{ .. }`), in its form when
/// it has one: a color keeps its hex format, `Colors.X` its name, `new Thickness(..)` its call.
fn respell(old: Node, text: &str, value: &PropValue) -> Option<(usize, usize, String)> {
    let whole = |s: String| Some((old.start_byte(), old.end_byte(), s));
    let inner = if old.kind() == "jsx_expression" { expression_of(old) } else { Some(old) };
    match value {
        PropValue::Color(c) => {
            if let Some(s) = inner.filter(|e| e.kind() == "string") {
                let content = &text[s.start_byte() + 1..s.end_byte() - 1];
                if let Some((_, len)) = parse_color_text(content) {
                    let upper = !content.chars().any(|ch| ch.is_ascii_lowercase());
                    return Some((s.start_byte() + 1, s.end_byte() - 1, format!("#{}", color_hex(*c, Some(len), upper))));
                }
            }
            if let Some(e) = inner.filter(|e| e.kind() == "member_expression")
                && let Some(p) = e.child_by_field_name("property")
                && let Some((n, _)) = crate::setcode::NAMED_COLORS.iter().find(|(_, v)| v == c)
            {
                return Some((p.start_byte(), p.end_byte(), n.to_string()));
            }
            whole(spell_prop(value)?)
        }
        PropValue::Thickness(t) => {
            if let Some(e) = inner.filter(|e| e.kind() == "new_expression")
                && let Some(args) = e.child_by_field_name("arguments")
            {
                let mut c = args.walk();
                let numbers = args.named_children(&mut c).all(|a| a.kind() == "number" || (a.kind() == "unary_expression" && a.named_child(0).is_some_and(|x| x.kind() == "number")));
                if numbers {
                    let [l, tp, r, b] = *t;
                    let a = if l == tp && tp == r && r == b { format!("({l})") } else if l == r && tp == b { format!("({l}, {tp})") } else { format!("({l}, {tp}, {r}, {b})") };
                    return Some((args.start_byte(), args.end_byte(), a));
                }
                return None;
            }
            if inner.is_some_and(|e| e.kind() == "number") && t.iter().all(|x| *x == t[0]) {
                return whole(format!("{{{}}}", t[0]));
            }
            whole(format!("\"{}\"", thickness_text(*t)))
        }
        PropValue::Plain(v) => whole(spell_attr(v)?),
    }
}

fn parse_color_text(s: &str) -> Option<(u32, usize)> {
    let h = s.strip_prefix('#')?;
    (h.chars().all(|c| c.is_ascii_hexdigit()) && matches!(h.len(), 3 | 4 | 6 | 8)).then_some((0, h.len()))
}

// ---------------------------------------------------------------- editing

fn plan_tsx(text: &str, src: &SourceTree, handles: &[Node], i: usize, edit: &SourceEdit) -> Result<(usize, usize, String), SourceEditRefusal> {
    use SourceEditRefusal::*;
    let node = &src.nodes[i];
    let h = handles[i];
    let child = |role: SourceRole| (i + 1..src.nodes.len()).find(|&j| src.nodes[j].parent == Some(i) && src.nodes[j].role == role);
    match edit {
        SourceEdit::SetValue { value } => {
            let (lit, name) = match node.role {
                SourceRole::Literal => (h, src.nodes[node.parent.ok_or(NotEditable)?].name.clone()),
                SourceRole::Attribute => (handles[child(SourceRole::Literal).ok_or(NotEditable)?], node.name.clone()),
                _ => return Err(NotEditable),
            };
            let spelled = spell_attr(value).ok_or(InvalidValue)?;
            if lit.kind() == "jsx_attribute" {
                // `<X IsVisible />` gets its value written out.
                return Ok((lit.start_byte(), lit.end_byte(), format!("{}={spelled}", name.unwrap_or_default())));
            }
            Ok((lit.start_byte(), lit.end_byte(), spelled))
        }
        SourceEdit::AddProperty { name, value } => {
            if node.role != SourceRole::Element || !is_ident(name) {
                return Err(NotEditable);
            }
            if src.nodes.iter().any(|n| n.parent == Some(i) && n.name.as_deref() == Some(name)) {
                return Err(InvalidValue);
            }
            Ok(add_attribute(text, h, &format!("{name}={}", spell_attr(value).ok_or(InvalidValue)?)))
        }
        SourceEdit::Remove => match node.role {
            SourceRole::Attribute if h.kind() == "jsx_attribute" => Ok(cut_with_space_before(text, h)),
            SourceRole::Element if node.parent.is_some_and(|p| src.nodes[p].name.as_deref() == Some("Children") && src.nodes[p].role == SourceRole::Attribute) => {
                Ok(cut_line_or_space(text, h))
            }
            _ => Err(NotEditable),
        },
        _ => Err(NotEditable),
    }
}

/// ` Name=value` after an element's last attribute (on its own line when they are one per line).
fn add_attribute(text: &str, element: Node, attr: &str) -> (usize, usize, String) {
    let o = open_tag(element).expect("an element");
    let attrs = attributes(element);
    let lines = Lines::new(text);
    let anchor = attrs.last().copied().or_else(|| o.child_by_field_name("name")).expect("a named element");
    let at = anchor.end_byte();
    let name_line = o.child_by_field_name("name").map(|n| lines.line_of(n.start_byte()));
    if let Some(last) = attrs.last()
        && Some(lines.line_of(last.start_byte())) != name_line
    {
        return (at, at, format!("{}{}{attr}", newline(text), line_indent(text, &lines, last.start_byte())));
    }
    (at, at, format!(" {attr}"))
}

/// Deletes a node with the white space before it (an attribute and its leading space).
fn cut_with_space_before(text: &str, n: Node) -> (usize, usize, String) {
    let start = text[..n.start_byte()].trim_end_matches([' ', '\t', '\r', '\n']).len();
    (start, n.end_byte(), String::new())
}

/// Deletes a JSX child: its whole lines when it stands alone on them, else with the space before it.
fn cut_line_or_space(text: &str, n: Node) -> (usize, usize, String) {
    let before = &text[..n.start_byte()];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let after = &text[n.end_byte()..];
    let line_end = after.find('\n').map(|i| n.end_byte() + i + 1);
    match line_end {
        Some(e) if before[line_start..].trim().is_empty() && text[n.end_byte()..e].trim().is_empty() => (line_start, e, String::new()),
        _ => cut_with_space_before(text, n),
    }
}

// ---------------------------------------------------------------- matching

/// A parsed TSX file: its keys and the components it declares.
struct TsxFile<'t> {
    uri: String,
    text: &'t str,
    root: Node<'t>,
    /// Element node id -> key.
    keys: HashMap<usize, String>,
}

/// What a JSX value makes, in order.
#[derive(Clone)]
enum Item<'t> {
    One { file: usize, node: Node<'t>, via: Vec<String>, frame: Option<usize> },
    Stop,
}

/// A component used as `<Card ..>..</Card>`: where it was used (its children and props).
struct Frame<'t> {
    file: usize,
    usage: Node<'t>,
    parent: Option<usize>,
    /// The component's props parameter: a name (`props`) or the destructured names.
    props: Vec<String>,
}

struct TsxMatcher<'t, 'r> {
    files: Vec<TsxFile<'t>>,
    rt: &'r RuntimeTree,
    kids: HashMap<NodeId, Vec<usize>>,
    matched: HashMap<NodeId, RuntimeMatch>,
    frames: Vec<Frame<'t>>,
}

/// The function a component name is declared as, in any of the files: `function Card(..)`,
/// `const Card = (..) => ..`.
fn find_component<'t>(files: &[TsxFile<'t>], name: &str) -> Option<(usize, Node<'t>)> {
    if !name.starts_with(char::is_uppercase) {
        return None;
    }
    let mut found = vec![];
    for (f, file) in files.iter().enumerate() {
        let mut stack = vec![file.root];
        while let Some(n) = stack.pop() {
            match n.kind() {
                "function_declaration" if n.child_by_field_name("name").is_some_and(|x| &file.text[x.byte_range()] == name) => found.push((f, n)),
                "variable_declarator" if n.child_by_field_name("name").is_some_and(|x| &file.text[x.byte_range()] == name) => {
                    if let Some(v) = n.child_by_field_name("value").filter(|v| matches!(v.kind(), "arrow_function" | "function_expression" | "function")) {
                        found.push((f, v));
                    }
                }
                _ => {
                    let mut c = n.walk();
                    stack.extend(n.children(&mut c));
                }
            }
        }
    }
    match found.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

/// What a component function returns: its arrow body or its one `return`.
fn component_return(f: Node) -> Option<Node> {
    let body = f.child_by_field_name("body")?;
    if body.kind() != "statement_block" {
        return Some(body);
    }
    let mut returns = vec![];
    let mut stack = vec![body];
    while let Some(n) = stack.pop() {
        match n.kind() {
            "return_statement" => returns.push(n),
            "arrow_function" | "function_expression" | "function_declaration" | "function" => {}
            _ => {
                let mut c = n.walk();
                stack.extend(n.children(&mut c));
            }
        }
    }
    match returns.as_slice() {
        [r] => r.named_child(0),
        _ => None,
    }
}

/// The names a component's props arrive as: `props` or the destructured `{ title, children }`.
fn props_names(f: Node, text: &str) -> Vec<String> {
    let first = f
        .child_by_field_name("parameters")
        .and_then(|p| {
            let mut c = p.walk();
            p.named_children(&mut c).next()
        })
        .or_else(|| f.child_by_field_name("parameter"));
    let Some(first) = first else { return vec![] };
    let pattern = first.child_by_field_name("pattern").unwrap_or(first);
    match pattern.kind() {
        "identifier" => vec![text[pattern.byte_range()].to_string()],
        "object_pattern" => {
            let mut out = vec![];
            let mut c = pattern.walk();
            for p in pattern.named_children(&mut c) {
                let n = p.child_by_field_name("key").or_else(|| p.child_by_field_name("left")).unwrap_or(p);
                out.push(format!("{{{}", &text[n.byte_range()]));
            }
            out
        }
        _ => vec![],
    }
}

fn short_type(t: &str) -> &str {
    let t = t.split(['`', '<']).next().unwrap_or(t);
    t.rsplit(['.', '+']).next().unwrap_or(t)
}

impl<'t> TsxMatcher<'t, '_> {
    /// The elements a JSX value makes, following components, their `children` and local consts.
    fn eval(&mut self, f: usize, mut e: Node<'t>, frame: Option<usize>, via: &[String]) -> Vec<Item<'t>> {
        let text = self.files[f].text;
        while e.kind() == "parenthesized_expression" || (e.kind() == "jsx_expression") {
            match expression_of(e) {
                Some(x) => e = x,
                None => return vec![],
            }
        }
        match e.kind() {
            "null" | "undefined" | "false" => return vec![],
            "jsx_element" if is_fragment(e) => return self.children_items(f, e, frame, via),
            "jsx_element" | "jsx_self_closing_element" => {
                let name = tag_name(e, text).unwrap_or_default().to_string();
                if let Some((cf, comp)) = find_component(&self.files, &name)
                    && let Some(ret) = component_return(comp)
                {
                    if self.frames.len() > 32 {
                        return vec![Item::Stop];
                    }
                    let props = props_names(comp, self.files[cf].text);
                    self.frames.push(Frame { file: f, usage: e, parent: frame, props });
                    let k = self.frames.len() - 1;
                    let mut via = via.to_vec();
                    via.extend(self.files[f].keys.get(&e.id()).cloned());
                    return self.eval(cf, ret, Some(k), &via);
                }
                return vec![Item::One { file: f, node: e, via: via.to_vec(), frame }];
            }
            _ => {}
        }
        // `children` / `props.children` of the component being expanded: the usage's children.
        let name = &text[e.byte_range()];
        if let Some(k) = frame {
            let fr = &self.frames[k];
            let is_children = (name == "children" && fr.props.iter().any(|p| p == "{children"))
                || fr.props.first().is_some_and(|p| !p.starts_with('{') && name == format!("{p}.children"));
            if is_children {
                let (uf, usage, parent) = (fr.file, fr.usage, fr.parent);
                return self.children_items(uf, usage, parent, via);
            }
        }
        if e.kind() == "identifier"
            && let Some(init) = local_const(e, name, text)
        {
            return self.eval(f, init, frame, via);
        }
        vec![Item::Stop]
    }

    fn children_items(&mut self, f: usize, e: Node<'t>, frame: Option<usize>, via: &[String]) -> Vec<Item<'t>> {
        let mut out = vec![];
        for k in jsx_children(e) {
            out.extend(self.eval(f, k, frame, via));
            if matches!(out.last(), Some(Item::Stop)) {
                break;
            }
        }
        out
    }

    fn fits(&self, r: usize, name: &str) -> bool {
        let n = &self.rt.nodes[r];
        !self.matched.contains_key(&n.id) && short_type(&n.type_name) == name
    }

    fn match_list(&mut self, list: &[usize], items: Vec<Item<'t>>) {
        let mut ptr = 0;
        for item in items {
            let Item::One { file, node, via, frame } = item else { break };
            let name = tag_name(node, self.files[file].text).map(short_type).unwrap_or_default().to_string();
            if let Some(j) = (ptr..list.len()).find(|&j| self.fits(list[j], &name)) {
                self.assign(list[j], file, node, via, frame);
                ptr = j + 1;
            }
        }
    }

    fn assign(&mut self, r: usize, file: usize, node: Node<'t>, via: Vec<String>, frame: Option<usize>) {
        let Some(key) = self.files[file].keys.get(&node.id()).cloned() else { return };
        let id = self.rt.nodes[r].id;
        self.matched.insert(id, RuntimeMatch { node: id, uri: self.files[file].uri.clone(), key, via: via.clone() });
        let items = self.children_items(file, node, frame, &via);
        let kids = self.kids.get(&id).cloned().unwrap_or_default();
        self.match_list(&kids, items);
    }
}

/// The value of a `const name = ..` declared in a scope around `from`.
fn local_const<'t>(from: Node<'t>, name: &str, text: &str) -> Option<Node<'t>> {
    let mut n = from;
    while let Some(p) = n.parent() {
        if matches!(p.kind(), "statement_block" | "program") {
            let mut c = p.walk();
            for st in p.named_children(&mut c) {
                let st = if st.kind() == "export_statement" { st.child_by_field_name("declaration").unwrap_or(st) } else { st };
                if !matches!(st.kind(), "lexical_declaration" | "variable_declaration") {
                    continue;
                }
                let mut c2 = st.walk();
                for d in st.named_children(&mut c2).filter(|d| d.kind() == "variable_declarator") {
                    if d.child_by_field_name("name").is_some_and(|x| &text[x.byte_range()] == name) {
                        return d.child_by_field_name("value");
                    }
                }
            }
        }
        n = p;
    }
    None
}

/// The location hint of a runtime node (`file:line:column`, 1-based, at the element's `<`): the
/// element there, when one file of `files` ends with that path.
fn element_at<'t>(files: &[TsxFile<'t>], source: &str) -> Option<(usize, Node<'t>)> {
    // `path:line:col` or `path:line`; the path may hold a drive colon or a uri scheme.
    let (rest, last) = source.rsplit_once(':')?;
    let last: usize = last.trim().parse().ok()?;
    let (path, line, col) = match rest.rsplit_once(':').map(|(p, l)| (p, l.trim().parse::<usize>())) {
        Some((p, Ok(line))) => (p, line, Some(last)),
        _ => (rest, last, None),
    };
    let path = path.replace('\\', "/").to_ascii_lowercase();
    let path = path.trim_start_matches("file:///").trim_start_matches("./");
    let f = files.iter().position(|fi| {
        let u = fi.uri.replace('\\', "/").to_ascii_lowercase();
        u.ends_with(path) || path.ends_with(u.trim_start_matches("file:///"))
    })?;
    let file = &files[f];
    let lines = Lines::new(file.text);
    let at = |n: &Node| {
        let p = lines.pos(n.start_byte());
        (p.line as usize + 1, p.character as usize + 1)
    };
    let mut stack = vec![file.root];
    let mut best = None;
    while let Some(n) = stack.pop() {
        if is_jsx(n) {
            let (l, c) = at(&n);
            // Without a column: the first element on that line.
            let first_on_line = col.is_none() && best.is_none_or(|b: Node| b.start_byte() > n.start_byte());
            if l == line && (col.is_some_and(|col| c == col || c + 1 == col || c == col + 1) || first_on_line) {
                best = Some(n);
                if col.is_some() {
                    break;
                }
            }
        }
        let mut cur = n.walk();
        stack.extend(n.children(&mut cur));
    }
    best.map(|n| (f, n))
}

/// Matches runtime controls to the JSX of `files` (TSX only): by each node's location hint when
/// the app sends one, else by structure from `<Canvas>`'s children (the runtime roots), following
/// components like helpers. `.map`, conditionals and spreads end a list.
pub(crate) fn match_tsx(files: &[(String, String)], tree: &RuntimeTree, skip: &HashSet<NodeId>) -> Vec<RuntimeMatch> {
    let parsed: Vec<Tree> = files.iter().map(|(_, t)| parse_tsx(t)).collect();
    let mut m = TsxMatcher { files: vec![], rt: tree, kids: HashMap::new(), matched: HashMap::new(), frames: vec![] };
    for ((uri, text), ts) in files.iter().zip(&parsed) {
        let (src, handles) = walk_tsx(ts, text, uri);
        let keys = (0..src.nodes.len()).filter(|&i| src.nodes[i].role == SourceRole::Element).map(|i| (handles[i].id(), src.nodes[i].key.clone())).collect();
        m.files.push(TsxFile { uri: uri.clone(), text, root: ts.root_node(), keys });
    }
    for (i, n) in tree.nodes.iter().enumerate() {
        if let Some(p) = n.parent.filter(|_| !n.internal) {
            m.kids.entry(p).or_default().push(i);
        }
    }
    // 1. Location hints.
    for n in &tree.nodes {
        if n.internal || skip.contains(&n.id) {
            continue;
        }
        if let Some((f, el)) = n.source.as_deref().and_then(|s| element_at(&m.files, s))
            && let Some(key) = m.files[f].keys.get(&el.id()).cloned()
        {
            m.matched.insert(n.id, RuntimeMatch { node: n.id, uri: m.files[f].uri.clone(), key, via: vec![] });
        }
    }
    // 2. Structure from the one `<Canvas>`: its children are the runtime roots.
    let mut canvases = vec![];
    for (f, file) in m.files.iter().enumerate() {
        let mut stack = vec![file.root];
        while let Some(n) = stack.pop() {
            if is_jsx(n) && tag_name(n, file.text) == Some("Canvas") {
                canvases.push((f, n));
            }
            let mut c = n.walk();
            stack.extend(n.children(&mut c));
        }
    }
    let roots: Vec<usize> = (0..tree.nodes.len()).filter(|&i| tree.nodes[i].parent.is_none() && !tree.nodes[i].internal).collect();
    let roots: Vec<usize> = roots
        .into_iter()
        .flat_map(|r| if matches!(short_type(&tree.nodes[r].type_name), "Canvas" | "DrawnView") { m.kids.get(&tree.nodes[r].id).cloned().unwrap_or_default() } else { vec![r] })
        .collect();
    if let [(f, canvas)] = canvases.as_slice() {
        let items = m.children_items(*f, *canvas, None, &[]);
        m.match_list(&roots, items);
    } else if canvases.is_empty() {
        // 3. A DrawFiddle snippet: the Canvas renders the module's default export (a component or an
        // element) with nothing in between.
        let exports: Vec<(usize, Node)> = m.files.iter().enumerate().filter_map(|(f, file)| default_export(file.root).map(|e| (f, e))).collect();
        if let [(f, export)] = exports.as_slice() {
            let items = match export.kind() {
                "function_declaration" | "arrow_function" | "function_expression" | "function" => match component_return(*export) {
                    Some(ret) => m.eval(*f, ret, None, &[]),
                    None => vec![],
                },
                "identifier" => {
                    let name = m.files[*f].text[export.byte_range()].to_string();
                    match find_component(&m.files, &name).and_then(|(cf, c)| component_return(c).map(|r| (cf, r))) {
                        Some((cf, ret)) => m.eval(cf, ret, None, &[]),
                        None => vec![],
                    }
                }
                _ => m.eval(*f, *export, None, &[]),
            };
            m.match_list(&roots, items);
        }
    }
    m.matched.into_values().filter(|r| !skip.contains(&r.node)).collect()
}

/// What `export default ..` exports: a function, a name or an expression.
fn default_export(program: Node) -> Option<Node> {
    let mut c = program.walk();
    let found = program.named_children(&mut c).find(|st| {
        let mut c2 = st.walk();
        st.kind() == "export_statement" && st.children(&mut c2).any(|k| k.kind() == "default")
    })?;
    found.child_by_field_name("declaration").or_else(|| found.child_by_field_name("value"))
}

// ---------------------------------------------------------------- writing

fn matched_file<'a>(files: &'a [(String, String)], m: &RuntimeMatch) -> &'a str {
    files.iter().find(|(u, _)| *u == m.uri).expect("matched file").1.as_str()
}

fn finish(uri: &str, text: &str, a: usize, b: usize, new_text: String, element: &str) -> CodeEdit {
    let mut new = text.to_string();
    new.replace_range(a..b, &new_text);
    let lines = Lines::new(&new);
    let range = match new_text.find(element).filter(|_| !element.is_empty()) {
        Some(skip) => lines.range(a + skip, a + skip + element.len()),
        None => lines.range(a, a + new_text.len()),
    };
    CodeEdit { uri: uri.into(), text: new, range }
}

/// [`set_property`] for a control made by JSX: the attribute's value, the attribute of the
/// component usage that feeds it (`<Card Title="x">` for `Text={Title}` in `Card`), or a new attribute.
pub(crate) fn set_property(files: &[(String, String)], m: &RuntimeMatch, prop: &str, value: &PropValue) -> Result<CodeEdit, Refusal> {
    let text = matched_file(files, m);
    let ts = parse_tsx(text);
    let (src, handles) = walk_tsx(&ts, text, &m.uri);
    let e = src.nodes.iter().position(|n| n.key == m.key).ok_or_else(|| Refusal::new("src-key-missing", &[("key", &m.key)]))?;
    let attr_key = format!("{}/{prop}", m.key);
    let Some(a) = src.nodes.iter().position(|n| n.key == attr_key) else {
        let spelled = spell_prop(value).ok_or_else(|| Refusal::new("src-value-misfit", &[("key", &m.key)]))?;
        let (at, b, t) = add_attribute(text, handles[e], &format!("{prop}={spelled}"));
        let el = t.trim_start().to_string();
        return Ok(finish(&m.uri, text, at, b, t, &el));
    };
    let v = (a + 1..src.nodes.len()).find(|&j| src.nodes[j].parent == Some(a)).ok_or_else(|| Refusal::new("src-no-value", &[]))?;
    match src.nodes[v].role {
        SourceRole::Literal if handles[v].kind() == "jsx_attribute" => {
            let spelled = spell_prop(value).ok_or_else(|| Refusal::new("src-value-misfit", &[("key", &attr_key)]))?;
            let t = format!("{prop}={spelled}");
            Ok(finish(&m.uri, text, handles[v].start_byte(), handles[v].end_byte(), t.clone(), &t))
        }
        SourceRole::Literal => {
            let (x, y, t) = respell(handles[v], text, value).ok_or_else(|| Refusal::new("src-value-misfit", &[("key", &attr_key)]))?;
            Ok(finish(&m.uri, text, x, y, t.clone(), &t))
        }
        SourceRole::Reference => {
            let expr = expression_of(handles[v]).unwrap_or(handles[v]);
            // `Margin={new Thickness(4, 2)}`: its arguments.
            if expr.kind() == "new_expression" {
                let (x, y, t) = respell(handles[v], text, value).ok_or_else(|| Refusal::new("src-not-literal", &[("key", &attr_key)]))?;
                return Ok(finish(&m.uri, text, x, y, t.clone(), &t));
            }
            // `Text={title}` / `Text={props.title}` inside a component: the usage's attribute.
            let name = text[expr.byte_range()].rsplit('.').next().unwrap_or_default().to_string();
            let usage = m.via.last().ok_or_else(|| Refusal::new("src-not-literal", &[("key", &attr_key)]))?;
            if !matches!(expr.kind(), "identifier" | "member_expression") {
                return Err(Refusal::new("src-not-literal", &[("key", &attr_key)]));
            }
            let usage_attr = format!("{usage}/{name}");
            let u = src.nodes.iter().position(|n| n.key == usage_attr).ok_or_else(|| Refusal::new("src-arg-no-value", &[("prop", &prop), ("param", &name), ("call", usage)]))?;
            let uv = (u + 1..src.nodes.len()).find(|&j| src.nodes[j].parent == Some(u) && src.nodes[j].role == SourceRole::Literal).ok_or_else(|| Refusal::new("src-not-literal", &[("key", &usage_attr)]))?;
            let (x, y, t) = respell(handles[uv], text, value).ok_or_else(|| Refusal::new("src-value-misfit", &[("key", &usage_attr)]))?;
            Ok(finish(&m.uri, text, x, y, t.clone(), &t))
        }
        _ => Err(Refusal::new("src-not-literal", &[("key", &attr_key)])),
    }
}

/// A new control as JSX: `<T A="x" B={1} />`, or laid out over lines with its children inside.
pub(crate) enum NewControl<'a> {
    Defaults { type_name: &'a str, defaults: &'a [(String, String)], types: &'a [(String, String)] },
    Spec { spec: &'a ControlSpec, root_types: &'a [(String, String)] },
    /// Written as it is (a moved element's own text).
    Text(&'a str),
}

fn jsx_value(t: &str, value: &str) -> Result<String, Refusal> {
    let v = value_from_runtime(t, value, value)?;
    spell_prop(&v).ok_or_else(|| Refusal::new("src-type-unsupported", &[("type", &t)]))
}

fn jsx_element(spec: &ControlSpec, root_types: &[(String, String)], indent: &str, unit: &str, nl: &str) -> Result<String, Refusal> {
    let mut attrs = vec![];
    for p in &spec.props {
        let t = if p.type_name.is_empty() { root_types.iter().find(|(n, _)| *n == p.name).map(|(_, t)| t.as_str()) } else { Some(p.type_name.as_str()) };
        let t = t.ok_or_else(|| Refusal::new("src-insert-type-unknown", &[("prop", &p.name)]))?;
        attrs.push(format!("{}={}", p.name, jsx_value(t, &p.value)?));
    }
    let tag = short_type(&spec.type_name);
    let head = if attrs.is_empty() { format!("<{tag}") } else { format!("<{tag} {}", attrs.join(" ")) };
    if spec.children.is_empty() {
        return Ok(format!("{head} />"));
    }
    let inner = format!("{indent}{unit}");
    let mut out = format!("{head}>");
    for c in &spec.children {
        out.push_str(&format!("{nl}{inner}{}", jsx_element(c, &[], &inner, unit, nl)?));
    }
    out.push_str(&format!("{nl}{indent}</{tag}>"));
    Ok(out)
}

fn new_control(new: &NewControl, indent: &str, unit: &str, nl: &str) -> Result<String, Refusal> {
    match new {
        NewControl::Defaults { type_name, defaults, types } => {
            let props = defaults
                .iter()
                .map(|(n, v)| {
                    let t = types.iter().find(|(x, _)| x == n).map(|(_, t)| t.clone()).ok_or_else(|| Refusal::new("src-insert-type-unknown", &[("prop", n)]))?;
                    Ok(designer_protocol::SpecProp { name: n.clone(), value: v.clone(), type_name: t })
                })
                .collect::<Result<Vec<_>, Refusal>>()?;
            jsx_element(&ControlSpec { type_name: type_name.to_string(), props, children: vec![] }, &[], indent, unit, nl)
        }
        NewControl::Spec { spec, root_types } => jsx_element(spec, root_types, indent, unit, nl),
        NewControl::Text(t) => Ok(t.to_string()),
    }
}

/// [`insert_element`] / [`insert_spec`] for a parent made by JSX: the new element among its JSX
/// children, one per line at the siblings' indentation; a parent with no children gets them
/// (`<T ..>` .. `</T>`).
pub(crate) fn insert(files: &[(String, String)], tree: &RuntimeTree, matches: &HashMap<NodeId, RuntimeMatch>, parent: NodeId, index: usize, new: NewControl) -> Result<crate::insert::Splice, Refusal> {
    let m = matches.get(&parent).ok_or_else(|| Refusal::new("src-no-code", &[]))?;
    let kids: Vec<NodeId> = tree.nodes.iter().filter(|n| n.parent == Some(parent) && !n.internal).map(|n| n.id).collect();
    let text = matched_file(files, m);
    let ts = parse_tsx(text);
    let (src, handles) = walk_tsx(&ts, text, &m.uri);
    let e = src.nodes.iter().position(|n| n.key == m.key).ok_or_else(|| Refusal::new("src-key-missing", &[("key", &m.key)]))?;
    let has = |name: &str| src.nodes.iter().any(|n| n.parent == Some(e) && n.name.as_deref() == Some(name));
    if has("ItemTemplate") || has("ItemsSource") {
        return Err(Refusal::new("src-insert-templated", &[]));
    }
    let h = handles[e];
    let lines = Lines::new(text);
    let nl = newline(text);
    let base = line_indent(text, &lines, h.start_byte()).to_string();
    let unit = if base.contains('\t') { "\t".to_string() } else { "  ".to_string() };
    let items = jsx_children(h);
    if items.is_empty() {
        if !kids.is_empty() {
            return Err(Refusal::new("src-insert-unmapped", &[]));
        }
        let inner = format!("{base}{unit}");
        let el = new_control(&new, &inner, &unit, nl)?;
        let tag = tag_name(h, text).unwrap_or_default();
        if h.kind() == "jsx_self_closing_element" {
            // `<T .. />` -> `<T ..>` + the child + `</T>`.
            let close = h.child(h.child_count() - 1).filter(|c| c.kind() == "/>").map_or(h.end_byte() - 2, |c| c.start_byte());
            let head_end = text[..close].trim_end().len();
            let t = format!(">{nl}{inner}{el}{nl}{base}</{tag}>");
            return Ok(crate::insert::Splice::new(&m.uri, head_end, h.end_byte(), t, &el));
        }
        let open = h.child_by_field_name("open_tag").expect("open tag");
        let close = h.child_by_field_name("close_tag").expect("close tag");
        let t = format!("{nl}{inner}{el}{nl}{base}");
        return Ok(crate::insert::Splice::new(&m.uri, open.end_byte(), close.start_byte(), t, &el));
    }
    // Each runtime child must be one JSX child, in order.
    let attr_key = format!("{}/Children", m.key);
    let lined_up = kids.len() == items.len()
        && kids.iter().enumerate().all(|(i, kid)| {
            let key = format!("{attr_key}/{i}");
            matches.get(kid).is_some_and(|km| km.uri == m.uri && (km.key == key || km.via.contains(&key)))
        });
    if !lined_up {
        return Err(Refusal::new("src-insert-unmapped", &[]));
    }
    let i = index.min(items.len());
    let open_line = lines.line_of(h.start_byte());
    let multiline = items.iter().any(|it| lines.line_of(it.start_byte()) != open_line);
    let indent = line_indent(text, &lines, items[i.min(items.len() - 1)].start_byte()).to_string();
    let el = new_control(&new, if multiline { &indent } else { &base }, &unit, nl)?;
    let (at, t) = match (i < items.len(), multiline) {
        (true, true) => (items[i].start_byte(), format!("{el}{nl}{indent}")),
        (true, false) => (items[i].start_byte(), el.clone()),
        (false, true) => (items[items.len() - 1].end_byte(), format!("{nl}{indent}{el}")),
        (false, false) => (items[items.len() - 1].end_byte(), el.clone()),
    };
    Ok(crate::insert::Splice::new(&m.uri, at, at, t, &el))
}

/// [`remove_element`] for a control made by JSX: the element and its line.
pub(crate) fn remove(files: &[(String, String)], m: &RuntimeMatch) -> Result<CodeEdit, Refusal> {
    let text = matched_file(files, m);
    let src = read_tsx(&m.uri, text);
    let answer = edit_tsx(text, &src, &m.key, &SourceEdit::Remove);
    if answer.refused.is_some() {
        return Err(Refusal::new("src-remove-not-in-list", &[("key", &m.key)]));
    }
    let e = &answer.edits[0];
    let (a, b) = (crate::setcode::byte_of(text, e.range.start), crate::setcode::byte_of(text, e.range.end));
    Ok(finish(&m.uri, text, a, b, String::new(), ""))
}

/// The cut that takes JSX child `key` out of its parent's children (bytes `a..b`, what stays there)
/// and the element's own bytes; `None` when `key` is no such child.
pub(crate) fn cut(uri: &str, text: &str, key: &str) -> Option<(usize, usize, String, std::ops::Range<usize>)> {
    let ts = parse_tsx(text);
    let (src, handles) = walk_tsx(&ts, text, uri);
    let e = src.nodes.iter().position(|n| n.key == key)?;
    let node = &src.nodes[e];
    let in_children = node.parent.is_some_and(|p| src.nodes[p].role == SourceRole::Attribute && src.nodes[p].name.as_deref() == Some("Children"));
    if node.role != SourceRole::Element || !in_children {
        return None;
    }
    let (a, b, fill) = cut_line_or_space(text, handles[e]);
    Some((a, b, fill, handles[e].byte_range()))
}

/// [`element_code`] for TSX: each element's attribute names, `Children` when it has JSX children.
pub(crate) fn element_code(uri: &str, text: &str) -> HashMap<String, crate::codeset::ElementCode> {
    let src = read_tsx(uri, text);
    let mut out = HashMap::new();
    for (i, n) in src.nodes.iter().enumerate().filter(|(_, n)| n.role == SourceRole::Element) {
        let names = src.nodes.iter().filter(|a| a.parent == Some(i) && a.role == SourceRole::Attribute).filter_map(|a| a.name.clone()).collect();
        out.insert(n.key.clone(), crate::codeset::ElementCode { type_name: n.name.clone(), names });
    }
    out
}
