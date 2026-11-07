//! The Rust side: DrawnUI for Rust apps. Controls are fluent chains on a constructor
//! (`SkiaLabel::new("Hi").font_size(18)`), their children the argument of `.children(..)` (a tuple,
//! `vec![..]` or one control) or `.content(..)`. Runtime controls are matched by structure from the
//! function the app builds its window with (`Ui::new(state, build)`, a DrawFiddle snippet's `fn build`),
//! through calls into the app's own functions. Writes are byte edits of the file: a property is the
//! argument of its builder call (`.font_size(18)`), or a new call at the end of the chain; inserts,
//! moves and removals edit the children list. Values are spelled as the engine's builders take them:
//! numbers, `true`, `"text"`, `LayoutOptions::Center`, `Color::from_rgb(..)`, a thickness as `8` or
//! `(8, 4)`.

use std::collections::{HashMap, HashSet};

use designer_protocol::{ControlSpec, NodeId, Refusal, Tree as RuntimeTree};
use tree_sitter::{Node, Parser, Tree};

use super::*;
use crate::setcode::{CodeEdit, PropValue, value_from_runtime};

/// Whether a file is read as Rust: `.rs`.
pub fn is_rust(uri: &str) -> bool {
    uri.to_ascii_lowercase().ends_with(".rs")
}

/// Macro names blanked, so that their arguments parse as expressions: `vec![a, b]` reads as
/// `[a, b]`, `attach!(Ui::new(..))` as `(Ui::new(..))`. Byte offsets do not move.
fn blank_macros(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = b.to_vec();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    for i in 1..b.len().saturating_sub(1) {
        if b[i] == b'!' && matches!(b[i + 1], b'(' | b'[') && ident(b[i - 1]) {
            let mut s = i;
            while s > 0 && (ident(b[s - 1]) || b[s - 1] == b':') {
                s -= 1;
            }
            out[s..=i].fill(b' ');
        }
    }
    String::from_utf8(out).expect("only ASCII bytes replaced")
}

fn parse_rs(text: &str) -> Tree {
    let mut p = Parser::new();
    p.set_language(&tree_sitter_rust::LANGUAGE.into()).expect("tree-sitter-rust ABI");
    p.parse(blank_macros(text), None).expect("parser without timeout always returns a tree")
}

// ---------------------------------------------------------------- syntax

/// The controls the engine makes, and the layouts' other names (`SkiaStack::new()` is a SkiaLayout).
const CONTROLS: &[&str] = &[
    "SkiaLabel", "SkiaLabelFps", "SkiaRichLabel", "SkiaLayout", "DrawnGame", "SkiaShape", "SkiaButton", "SkiaImage", "SkiaImageTiles", "SkiaSvg",
    "SkiaScroll", "SkiaScrollBar", "SkiaEditor", "SkiaCarousel", "SkiaShaderCarousel", "SkiaDrawer", "SkiaDecoratedGrid", "RescalingLayout",
    "SkiaCheckbox", "SkiaRadioButton", "SkiaSwitch", "SkiaToggle", "SkiaSlider", "SkiaProgress", "SkiaLottie", "SkiaGif", "SkiaSprite",
    "SkiaSpriteSet", "SkiaShell",
];
const LAYOUT_NAMES: &[&str] = &["SkiaStack", "SkiaRow", "SkiaLayer", "SkiaWrap", "SkiaGrid"];

/// The property a control's constructor argument sets: `SkiaLabel::new("Hi")` sets `text`.
fn ctor_prop(type_name: &str) -> Option<&'static str> {
    match type_name {
        "SkiaLabel" | "SkiaRichLabel" | "SkiaButton" | "SkiaRadioButton" => Some("text"),
        "SkiaImage" | "SkiaImageTiles" | "SkiaSvg" | "SkiaLottie" | "SkiaGif" | "SkiaSprite" => Some("source"),
        _ => None,
    }
}

/// A new control as the engine's own constructor makes it (`drawnui::designer::create`), its
/// constructor argument `arg` when given.
fn constructor(type_name: &str, arg: Option<String>) -> String {
    let default = match type_name {
        "SkiaLabel" => "\"Label\"",
        "SkiaRichLabel" => "\"Rich text\"",
        "SkiaButton" => "\"Button\"",
        "SkiaRadioButton" => "\"Option\"",
        "RescalingLayout" => "320.0, 240.0",
        t if ctor_prop(t) == Some("source") => "\"\"",
        _ => "",
    };
    format!("{type_name}::new({})", arg.as_deref().unwrap_or(default))
}

fn is_comment(n: Node) -> bool {
    n.kind().ends_with("comment") || n.kind() == "attribute_item"
}

fn named(n: Node) -> Vec<Node> {
    let mut c = n.walk();
    n.named_children(&mut c).filter(|x| !is_comment(*x)).collect()
}

/// `recv.name(args)` (also `recv.name::<T>(args)`): the receiver, the method's name node, the arguments.
fn method_call(n: Node) -> Option<(Node, Node, Node)> {
    if n.kind() != "call_expression" {
        return None;
    }
    let mut f = n.child_by_field_name("function")?;
    if f.kind() == "generic_function" {
        f = f.child_by_field_name("function")?;
    }
    if f.kind() != "field_expression" {
        return None;
    }
    Some((f.child_by_field_name("value")?, f.child_by_field_name("field")?, n.child_by_field_name("arguments")?))
}

/// `Type::name(args)`: the type's last segment, the function's name, the arguments.
fn path_call<'t>(n: Node<'t>, text: &'t str) -> Option<(&'t str, &'t str, Node<'t>)> {
    if n.kind() != "call_expression" {
        return None;
    }
    let mut f = n.child_by_field_name("function")?;
    if f.kind() == "generic_function" {
        f = f.child_by_field_name("function")?;
    }
    if f.kind() != "scoped_identifier" {
        return None;
    }
    let path = f.child_by_field_name("path")?;
    let name = f.child_by_field_name("name")?;
    let ty = text[path.byte_range()].rsplit("::").next().unwrap_or_default().split('<').next().unwrap_or_default().trim();
    Some((ty, &text[name.byte_range()], n.child_by_field_name("arguments")?))
}

/// `name(args)`: the function's name and the arguments.
fn fn_call<'t>(n: Node<'t>, text: &'t str) -> Option<(&'t str, Node<'t>)> {
    if n.kind() != "call_expression" {
        return None;
    }
    let f = n.child_by_field_name("function").filter(|f| f.kind() == "identifier")?;
    Some((&text[f.byte_range()], n.child_by_field_name("arguments")?))
}

/// The control type a constructor call makes: `SkiaLabel::new(..)`, `SkiaLayout::column()`,
/// `SkiaStack::new()` (a SkiaLayout), `Build::new(MyControl::default())`.
fn element_type(n: Node, text: &str) -> Option<String> {
    let (ty, func, args) = path_call(n, text)?;
    if ty == "Build" && func == "new" {
        let a = *named(args).first()?;
        return match a.kind() {
            "struct_expression" => a.child_by_field_name("name").map(|x| text[x.byte_range()].rsplit("::").next().unwrap_or_default().to_owned()),
            _ => path_call(a, text).map(|(t, _, _)| t.to_owned()),
        };
    }
    if LAYOUT_NAMES.contains(&ty) {
        return Some("SkiaLayout".into());
    }
    CONTROLS.contains(&ty).then(|| ty.to_owned())
}

/// The bottom of a fluent chain: `SkiaLabel::new(..)` under `SkiaLabel::new(..).font_size(9)`.
fn chain_base(mut n: Node) -> Node {
    while let Some((recv, _, _)) = method_call(n) {
        n = recv;
    }
    n
}

/// The calls on `base`, innermost first: (call node, method name node, arguments).
fn chain_calls(base: Node) -> Vec<(Node, Node, Node)> {
    let mut out = vec![];
    let mut cur = base;
    while let Some(field) = cur.parent().filter(|p| p.kind() == "field_expression" && p.child_by_field_name("value") == Some(cur)) {
        let mut call = field.parent();
        if call.is_some_and(|c| c.kind() == "generic_function") {
            call = call.and_then(|g| g.parent());
        }
        let Some(call) = call.filter(|c| c.kind() == "call_expression") else { break };
        let (Some(name), Some(args)) = (field.child_by_field_name("field"), call.child_by_field_name("arguments")) else { break };
        out.push((call, name, args));
        cur = call;
    }
    out
}

/// The whole chain on `base`: its last call, or `base` alone.
fn chain_top(base: Node) -> Node {
    chain_calls(base).last().map_or(base, |c| c.0)
}

fn strip_parens(mut n: Node) -> Node {
    while n.kind() == "parenthesized_expression" {
        match named(n).first() {
            Some(x) => n = *x,
            None => break,
        }
    }
    n
}

/// What a function gives back: its block's tail expression, or its one `return`.
fn returned(f: Node) -> Option<Node> {
    let body = f.child_by_field_name("body")?;
    if body.kind() != "block" {
        return Some(body);
    }
    if let Some(last) = named(body).last().filter(|l| !matches!(l.kind(), "expression_statement" | "let_declaration" | "empty_statement")) {
        return Some(*last);
    }
    let mut returns = vec![];
    let mut stack = vec![body];
    while let Some(n) = stack.pop() {
        match n.kind() {
            "return_expression" => returns.push(n),
            "closure_expression" | "function_item" => {}
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

/// A block's value: its tail expression.
fn block_tail(b: Node) -> Option<Node> {
    named(b).last().copied().filter(|l| !matches!(l.kind(), "expression_statement" | "let_declaration" | "empty_statement"))
}

fn param_names(f: Node, text: &str) -> Vec<String> {
    let Some(ps) = f.child_by_field_name("parameters") else { return vec![] };
    named(ps)
        .into_iter()
        .filter(|p| p.kind() == "parameter")
        .map(|p| p.child_by_field_name("pattern").map(|x| text[x.byte_range()].trim_start_matches("mut ").trim().to_owned()).unwrap_or_default())
        .collect()
}

/// The value of a `let name = ..;` in a block around `from`, before it.
fn local_let<'t>(from: Node<'t>, name: &str, text: &str) -> Option<Node<'t>> {
    let mut n = from;
    while let Some(p) = n.parent() {
        if p.kind() == "block" {
            for st in named(p).into_iter().rev().filter(|s| s.end_byte() <= from.start_byte()) {
                if st.kind() == "let_declaration" && st.child_by_field_name("pattern").is_some_and(|x| text[x.byte_range()].trim_start_matches("mut ").trim() == name) {
                    return st.child_by_field_name("value");
                }
            }
        }
        if p.kind() == "function_item" {
            break;
        }
        n = p;
    }
    None
}

// ---------------------------------------------------------------- keys

/// What a file declares: its functions and keys for its controls and calls. An element's key is
/// `scope/i` (the i-th control constructor of function `scope`, `Type.method` in an `impl`), a call
/// of a function by name `scope/ci`.
struct Index<'t> {
    text: &'t str,
    /// Node id -> key.
    keys: HashMap<usize, String>,
    /// Key -> node.
    nodes: HashMap<String, Node<'t>>,
    /// Free functions by name.
    fns: HashMap<String, Vec<Node<'t>>>,
    /// Element node ids -> control type.
    types: HashMap<usize, String>,
}

impl<'t> Index<'t> {
    fn new(tree: &'t Tree, text: &'t str) -> Self {
        let mut ix = Index { text, keys: HashMap::new(), nodes: HashMap::new(), fns: HashMap::new(), types: HashMap::new() };
        let mut counters: HashMap<String, (usize, usize)> = HashMap::new();
        ix.scan(tree.root_node(), "<file>", &mut counters, None);
        ix
    }

    fn scan(&mut self, n: Node<'t>, scope: &str, counters: &mut HashMap<String, (usize, usize)>, impl_type: Option<&str>) {
        let text = self.text;
        let mut scope = scope.to_owned();
        let mut impl_type = impl_type.map(str::to_owned);
        match n.kind() {
            "function_item" => {
                if let Some(name) = n.child_by_field_name("name").map(|x| &text[x.byte_range()]) {
                    if impl_type.is_none() {
                        self.fns.entry(name.to_owned()).or_default().push(n);
                    }
                    scope = match &impl_type {
                        Some(t) => format!("{t}.{name}"),
                        None => name.to_owned(),
                    };
                }
                impl_type = None;
            }
            "impl_item" => impl_type = n.child_by_field_name("type").map(|t| text[t.byte_range()].split('<').next().unwrap_or_default().trim().to_owned()),
            "call_expression" => {
                let (e, c) = counters.entry(scope.clone()).or_default();
                let key = if let Some(t) = element_type(n, text) {
                    self.types.insert(n.id(), t);
                    *e += 1;
                    Some(format!("{scope}/{}", *e - 1))
                } else if fn_call(n, text).is_some() {
                    *c += 1;
                    Some(format!("{scope}/c{}", *c - 1))
                } else {
                    None
                };
                if let Some(k) = key {
                    self.keys.insert(n.id(), k.clone());
                    self.nodes.insert(k, n);
                }
            }
            _ => {}
        }
        let mut c = n.walk();
        for k in n.children(&mut c).collect::<Vec<_>>() {
            self.scan(k, &scope, counters, impl_type.as_deref());
        }
    }
}

/// Reads a Rust file into its source tree: its controls (Element, named by type) with their
/// builder calls (Attribute, named by method), and the calls of the app's functions (Reference).
pub fn read_rust(uri: &str, text: &str) -> SourceTree {
    let ts = parse_rs(text);
    let ix = Index::new(&ts, text);
    let lines = Lines::new(text);
    let mut list: Vec<(&String, &Node)> = ix.nodes.iter().collect();
    list.sort_by_key(|(_, n)| n.start_byte());
    let mut nodes = vec![];
    for (key, n) in list {
        let is_el = ix.types.contains_key(&n.id());
        nodes.push(SourceNode {
            role: if is_el { SourceRole::Element } else { SourceRole::Reference },
            range: lines.range(n.start_byte(), if is_el { chain_top(*n).end_byte() } else { n.end_byte() }),
            parent: None,
            key: key.clone(),
            name: if is_el { ix.types.get(&n.id()).cloned() } else { fn_call(*n, text).map(|(f, _)| f.to_owned()) },
            value: None,
        });
        if is_el {
            let at = nodes.len() - 1;
            let mut seen: HashMap<String, usize> = HashMap::new();
            for (call, name, _) in chain_calls(*n) {
                let name = text[name.byte_range()].to_owned();
                let k = seen.entry(name.clone()).or_default();
                *k += 1;
                let akey = if *k == 1 { format!("{key}/{name}") } else { format!("{key}/{name}#{k}") };
                nodes.push(SourceNode { role: SourceRole::Attribute, range: lines.range(call.start_byte(), call.end_byte()), parent: Some(at), key: akey, name: Some(name), value: None });
            }
        }
    }
    SourceTree { uri: uri.into(), nodes }
}

/// The properties builder helpers set (`fill_x()` sets horizontal_options).
fn helper_props(method: &str) -> &'static [&'static str] {
    match method {
        "fill" | "center" => &["horizontal_options", "vertical_options"],
        "fill_x" | "center_x" | "start_x" | "end_x" => &["horizontal_options"],
        "fill_y" | "center_y" | "start_y" | "end_y" => &["vertical_options"],
        _ => &[],
    }
}

/// Methods of a chain that set no property.
fn not_a_prop(method: &str) -> bool {
    matches!(method, "children" | "child" | "content" | "assign" | "observe" | "into" | "with_gestures") || method.starts_with("on_") || method.starts_with("observe")
}

/// [`crate::element_code`] for Rust: each control's builder calls and its constructor's property.
pub(crate) fn element_code(uri: &str, text: &str) -> HashMap<String, crate::codeset::ElementCode> {
    let _ = uri;
    let ts = parse_rs(text);
    let ix = Index::new(&ts, text);
    let mut out = HashMap::new();
    for (id, ty) in &ix.types {
        let Some(key) = ix.keys.get(id) else { continue };
        let n = ix.nodes[key];
        let mut names: Vec<String> = vec![];
        let mut add = |s: &str| {
            if !names.iter().any(|x| x == s) {
                names.push(s.to_owned());
            }
        };
        if let (Some(p), Some((_, "new", args))) = (ctor_prop(ty), path_call(n, text))
            && named(args).len() == 1
        {
            add(p);
        }
        for (_, name, _) in chain_calls(n) {
            let m = &text[name.byte_range()];
            let helped = helper_props(m);
            if !helped.is_empty() {
                helped.iter().for_each(|p| add(p));
            } else if !not_a_prop(m) {
                add(m);
            }
        }
        out.insert(key.clone(), crate::codeset::ElementCode { type_name: Some(ty.clone()), names });
    }
    out
}

/// The functions an app builds its window with: the second argument of `Ui::new(state, build)` (a
/// function's name or a closure), else a `fn build` (a DrawFiddle snippet).
fn roots<'t>(files: &[RsFile<'t>]) -> Vec<(usize, Node<'t>)> {
    let mut out = vec![];
    for (f, file) in files.iter().enumerate() {
        let mut stack = vec![file.root];
        while let Some(n) = stack.pop() {
            if let Some(("Ui", "new", args)) = path_call(n, file.text)
                && let Some(b) = named(args).get(1).copied()
            {
                match b.kind() {
                    "closure_expression" => out.extend(b.child_by_field_name("body").map(|x| (f, x))),
                    _ => {
                        // That file's own function of that name first.
                        let name = file.text[b.byte_range()].rsplit("::").next().unwrap_or_default().trim().to_owned();
                        out.extend(find_fn(files, f, &name).and_then(|(ff, func)| returned(func).map(|r| (ff, r))));
                    }
                }
            }
            let mut c = n.walk();
            stack.extend(n.children(&mut c));
        }
    }
    if out.is_empty() {
        let found: Vec<(usize, Node<'t>)> = files.iter().enumerate().flat_map(|(f, file)| file.ix.fns.get("build").into_iter().flatten().map(move |n| (f, *n))).collect();
        if let [(f, func)] = found.as_slice() {
            out.extend(returned(*func).map(|r| (*f, r)));
        }
    }
    out
}

/// The function `name` called from file `from`: that file's own, else the one of that name in all.
fn find_fn<'t>(files: &[RsFile<'t>], from: usize, name: &str) -> Option<(usize, Node<'t>)> {
    if let Some([one]) = files[from].ix.fns.get(name).map(Vec::as_slice) {
        return Some((from, *one));
    }
    let found: Vec<(usize, Node<'t>)> = files.iter().enumerate().flat_map(|(f, file)| file.ix.fns.get(name).into_iter().flatten().map(move |n| (f, *n))).collect();
    match found.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

/// The names helpers give the controls they return: for each function (not the window's own build
/// function) whose value is a control it makes, that control's key and the function's name.
pub(crate) fn helper_names(uri: &str, text: &str) -> HashMap<String, String> {
    let ts = parse_rs(text);
    let file = RsFile { uri: uri.into(), text, root: ts.root_node(), ix: Index::new(&ts, text) };
    let files = [file];
    let root_fns: HashSet<usize> = roots(&files).iter().filter_map(|(_, r)| enclosing_fn(*r).map(|f| f.id())).collect();
    let file = &files[0];
    let mut out = HashMap::new();
    for (name, fs) in &file.ix.fns {
        for f in fs.iter().filter(|f| !root_fns.contains(&f.id())) {
            if let Some(ret) = returned(*f)
                && let Some(key) = file.ix.keys.get(&chain_base(strip_parens(ret)).id())
                && file.ix.types.contains_key(&chain_base(strip_parens(ret)).id())
            {
                out.insert(key.clone(), name.clone());
            }
        }
    }
    out
}

fn enclosing_fn(n: Node) -> Option<Node> {
    let mut p = n.parent();
    while let Some(x) = p {
        if x.kind() == "function_item" {
            return Some(x);
        }
        p = x.parent();
    }
    None
}

// ---------------------------------------------------------------- matching

struct RsFile<'t> {
    uri: String,
    text: &'t str,
    root: Node<'t>,
    ix: Index<'t>,
}

/// A use of one of the app's functions: where it was called (`file`, `call`, in frame `parent`),
/// for its parameters.
struct Frame<'t> {
    file: usize,
    call: Node<'t>,
    func_file: usize,
    func: Node<'t>,
    parent: Option<usize>,
}

/// A call chain that goes on a control from outside the function that makes it
/// (`swatch(ACCENT).tag("blue")`, the call `swatch(..)`), with the frame its arguments read in.
#[derive(Clone, Copy)]
struct Outer<'t> {
    file: usize,
    call: Node<'t>,
    frame: Option<usize>,
}

#[derive(Clone)]
enum Item<'t> {
    One { file: usize, node: Node<'t>, via: Vec<String>, frame: Option<usize>, outer: Vec<Outer<'t>> },
    Stop,
}

struct RsMatcher<'t, 'r> {
    files: Vec<RsFile<'t>>,
    rt: &'r RuntimeTree,
    kids: HashMap<NodeId, Vec<usize>>,
    matched: HashMap<NodeId, RuntimeMatch>,
    frames: Vec<Frame<'t>>,
}

/// A runtime type by its short name.
fn short_type(t: &str) -> &str {
    let t = t.split(['`', '<']).next().unwrap_or(t);
    t.rsplit(['.', '+', ':']).next().unwrap_or(t)
}

impl<'t> RsMatcher<'t, '_> {
    /// The controls an expression makes, in order: constructors, the app's functions, tuples,
    /// `vec![..]`, `Some(..)`, locals and parameters. Anything else (a loop, an iterator, a
    /// condition) ends the list: [`Item::Stop`].
    fn eval(&mut self, f: usize, e: Node<'t>, frame: Option<usize>, via: &[String], outer: &[Outer<'t>]) -> Vec<Item<'t>> {
        let text = self.files[f].text;
        let e = strip_parens(e);
        match e.kind() {
            "tuple_expression" | "array_expression" => {
                let mut out = vec![];
                for x in named(e) {
                    out.extend(self.eval(f, x, frame, via, &[]));
                    if matches!(out.last(), Some(Item::Stop)) {
                        break;
                    }
                }
                return out;
            }
            "unit_expression" => return vec![],
            "block" => return block_tail(e).map_or(vec![], |t| self.eval(f, t, frame, via, outer)),
            "identifier" => {
                let name = &text[e.byte_range()];
                if name == "None" {
                    return vec![];
                }
                if let Some(init) = local_let(e, name, text) {
                    return self.eval(f, init, frame, via, outer);
                }
                if let Some(k) = frame {
                    let fr = &self.frames[k];
                    let ps = param_names(fr.func, self.files[fr.func_file].text);
                    if let Some(i) = ps.iter().position(|p| p == name) {
                        let (cf, call, parent) = (fr.file, fr.call, fr.parent);
                        let arg = fn_call(call, self.files[cf].text).and_then(|(_, args)| named(args).get(i).copied());
                        return match arg {
                            Some(a) => self.eval(cf, a, parent, via, outer),
                            None => vec![Item::Stop],
                        };
                    }
                }
                return vec![Item::Stop];
            }
            _ => {}
        }
        let base = chain_base(e);
        // Only builder calls above it: an iterator's `map` / `collect` or the like makes controls at run time.
        let chain: Vec<String> = chain_calls(base).iter().map(|(_, n, _)| text[n.byte_range()].to_owned()).collect();
        if self.files[f].ix.types.contains_key(&base.id()) {
            return vec![Item::One { file: f, node: base, via: via.to_vec(), frame, outer: outer.to_vec() }];
        }
        if chain.iter().any(|m| matches!(m.as_str(), "map" | "collect" | "iter" | "into_iter" | "filter" | "flat_map" | "filter_map" | "chain" | "rev" | "take" | "skip" | "zip" | "enumerate" | "then" | "then_some" | "unwrap_or" | "unwrap_or_else")) {
            return vec![Item::Stop];
        }
        if let Some((name, args)) = fn_call(base, text) {
            if name == "Some" {
                return named(args).first().map_or(vec![Item::Stop], |a| self.eval(f, *a, frame, via, outer));
            }
            if let Some((cf, func)) = find_fn(&self.files, f, name)
                && let Some(ret) = returned(func)
            {
                if self.frames.len() > 64 {
                    return vec![Item::Stop];
                }
                self.frames.push(Frame { file: f, call: base, func_file: cf, func, parent: frame });
                let k = self.frames.len() - 1;
                let mut via = via.to_vec();
                via.extend(self.files[f].ix.keys.get(&base.id()).cloned());
                let mut outer = outer.to_vec();
                outer.push(Outer { file: f, call: base, frame });
                return self.eval(cf, ret, Some(k), &via, &outer);
            }
        }
        // `content.into()`: the value itself.
        if base != e && chain.iter().all(|m| m == "into") {
            return self.eval(f, base, frame, via, outer);
        }
        vec![Item::Stop]
    }

    /// The children an item's chains give it (`.children(..)`, `.child(..)`, `.content(..)`): its own
    /// chain first, then the chains on the calls that return it.
    fn children_items(&mut self, file: usize, node: Node<'t>, frame: Option<usize>, via: &[String], outer: &[Outer<'t>]) -> Vec<Item<'t>> {
        let mut lists: Vec<(usize, Option<usize>, Node<'t>)> = vec![];
        let mut add = |f: usize, fr: Option<usize>, base: Node<'t>, text: &str| {
            for (_, name, args) in chain_calls(base) {
                if matches!(&text[name.byte_range()], "children" | "child" | "content") {
                    lists.extend(named(args).first().map(|a| (f, fr, *a)));
                }
            }
        };
        add(file, frame, node, self.files[file].text);
        for o in outer {
            add(o.file, o.frame, o.call, self.files[o.file].text);
        }
        let mut out = vec![];
        for (f, fr, a) in lists {
            out.extend(self.eval(f, a, fr, via, &[]));
            if matches!(out.last(), Some(Item::Stop)) {
                break;
            }
        }
        out
    }

    fn fits(&self, r: usize, ty: &str) -> bool {
        let n = &self.rt.nodes[r];
        !self.matched.contains_key(&n.id) && short_type(&n.type_name) == ty
    }

    fn match_list(&mut self, list: &[usize], items: Vec<Item<'t>>) {
        let mut ptr = 0;
        for item in items {
            let Item::One { file, node, via, frame, outer } = item else { break };
            let ty = self.files[file].ix.types.get(&node.id()).cloned().unwrap_or_default();
            if let Some(j) = (ptr..list.len()).find(|&j| self.fits(list[j], &ty)) {
                self.assign(list[j], file, node, via, frame, outer);
                ptr = j + 1;
            }
        }
    }

    fn assign(&mut self, r: usize, file: usize, node: Node<'t>, via: Vec<String>, frame: Option<usize>, outer: Vec<Outer<'t>>) {
        let Some(key) = self.files[file].ix.keys.get(&node.id()).cloned() else { return };
        let id = self.rt.nodes[r].id;
        self.matched.insert(id, RuntimeMatch { node: id, uri: self.files[file].uri.clone(), key, via: via.clone() });
        let items = self.children_items(file, node, frame, &via, &outer);
        let kids = self.kids.get(&id).cloned().unwrap_or_default();
        self.match_list(&kids, items);
    }
}

fn parse_all<'t>(files: &'t [(String, String)], trees: &'t [Tree]) -> Vec<RsFile<'t>> {
    files.iter().zip(trees).map(|((uri, text), ts)| RsFile { uri: uri.clone(), text, root: ts.root_node(), ix: Index::new(ts, text) }).collect()
}

/// Matches runtime controls to the Rust of `files` (Rust only), by structure from the window's
/// build function (see [`roots`]).
pub(crate) fn match_rust(files: &[(String, String)], tree: &RuntimeTree, skip: &HashSet<NodeId>) -> Vec<RuntimeMatch> {
    let trees: Vec<Tree> = files.iter().map(|(_, t)| parse_rs(t)).collect();
    let mut m = RsMatcher { files: parse_all(files, &trees), rt: tree, kids: HashMap::new(), matched: HashMap::new(), frames: vec![] };
    for (i, n) in tree.nodes.iter().enumerate() {
        if let Some(p) = n.parent.filter(|_| !n.internal) {
            m.kids.entry(p).or_default().push(i);
        }
    }
    let runtime_roots: Vec<usize> = (0..tree.nodes.len()).filter(|&i| tree.nodes[i].parent.is_none() && !tree.nodes[i].internal && !tree.nodes[i].host.as_deref().is_some_and(|h| h.starts_with("popup"))).collect();
    for (f, ret) in roots(&m.files) {
        let items = m.eval(f, ret, None, &[], &[]);
        m.match_list(&runtime_roots, items);
    }
    m.matched.into_values().filter(|r| !skip.contains(&r.node)).collect()
}

// ---------------------------------------------------------------- spelling

/// A Rust string literal.
fn rs_string(s: &str) -> String {
    let mut out = String::from('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A number: whole numbers as integers (the builders of `f32` properties take them too), unless
/// `float`.
fn rs_number(v: f64, float: bool) -> String {
    if !float && v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{v}");
    if s.contains(['.', 'e', 'E', 'N', 'i']) { s } else { format!("{s}.0") }
}

/// `Color::from_rgb(..)`, `Color::from_argb(..)` when not opaque; hex bytes when `hex`.
fn rs_color(path: &str, argb: u32, hex: bool) -> String {
    let [a, r, g, b] = argb.to_be_bytes();
    let f = |x: u8| if hex { format!("0x{x:02X}") } else { x.to_string() };
    if a == 0xFF { format!("{path}::from_rgb({}, {}, {})", f(r), f(g), f(b)) } else { format!("{path}::from_argb({}, {}, {}, {})", f(a), f(r), f(g), f(b)) }
}

/// The colors named on skia's `Color`.
const NAMED: &[(&str, u32)] = &[
    ("TRANSPARENT", 0x0000_0000), ("BLACK", 0xFF00_0000), ("DARK_GRAY", 0xFF44_4444), ("GRAY", 0xFF88_8888), ("LIGHT_GRAY", 0xFFCC_CCCC),
    ("WHITE", 0xFFFF_FFFF), ("RED", 0xFFFF_0000), ("GREEN", 0xFF00_FF00), ("BLUE", 0xFF00_00FF), ("YELLOW", 0xFFFF_FF00), ("CYAN", 0xFF00_FFFF),
    ("MAGENTA", 0xFFFF_00FF),
];

/// A thickness as the builders take it: `8`, `(8, 4)` or `(1, 2, 3, 4)` (decimals when any has them).
fn rs_thickness([l, t, r, b]: [f64; 4]) -> String {
    let float = [l, t, r, b].iter().any(|v| v.fract() != 0.0);
    let n = |v: f64| rs_number(v, float);
    if l == t && t == r && r == b {
        n(l)
    } else if l == r && t == b {
        format!("({}, {})", n(l), n(t))
    } else {
        format!("({}, {}, {}, {})", n(l), n(t), n(r), n(b))
    }
}

/// How a file writes colors: hex bytes (`from_rgb(0x94, ..)`) or decimal.
fn hex_colors(text: &str) -> bool {
    text.contains("from_rgb(0x") || text.contains("from_argb(0x")
}

/// A value as a new argument.
fn spell(value: &PropValue, text: &str) -> Option<String> {
    Some(match value {
        PropValue::Plain(SourceEditValue::String { value }) => rs_string(value),
        PropValue::Plain(SourceEditValue::Number { value }) if value.is_finite() => rs_number(*value, false),
        PropValue::Plain(SourceEditValue::Number { .. }) => return None,
        PropValue::Plain(SourceEditValue::Boolean { value }) => value.to_string(),
        PropValue::Plain(SourceEditValue::Case { name }) => {
            let (ty, case) = name.rsplit_once('.')?;
            format!("{}::{case}", short_type(ty))
        }
        PropValue::Color(c) => rs_color("Color", *c, hex_colors(text)),
        PropValue::Thickness(t) => rs_thickness(*t),
    })
}

fn is_number(n: Node) -> bool {
    matches!(n.kind(), "integer_literal" | "float_literal") || (n.kind() == "unary_expression" && n.named_child(0).is_some_and(|x| matches!(x.kind(), "integer_literal" | "float_literal")))
}

fn is_float(n: Node) -> bool {
    n.kind() == "float_literal" || (n.kind() == "unary_expression" && n.named_child(0).is_some_and(|x| x.kind() == "float_literal"))
}

/// The value replacing argument `old`, in its form: a number keeps being a decimal, a color its
/// constructor and digits, a case its path. `None` when `old` is not a literal of that kind (a
/// constant, an expression).
fn respell(old: Node, text: &str, value: &PropValue) -> Option<String> {
    let old_text = &text[old.byte_range()];
    match value {
        PropValue::Plain(SourceEditValue::Number { value }) if value.is_finite() && is_number(old) => Some(rs_number(*value, is_float(old))),
        PropValue::Plain(SourceEditValue::String { .. }) if matches!(old.kind(), "string_literal" | "raw_string_literal") => spell(value, text),
        PropValue::Plain(SourceEditValue::Boolean { .. }) if old.kind() == "boolean_literal" => spell(value, text),
        PropValue::Plain(SourceEditValue::Case { name }) if old.kind() == "scoped_identifier" => {
            let path = old.child_by_field_name("path").map(|p| &text[p.byte_range()])?;
            let case = name.rsplit('.').next()?;
            Some(format!("{path}::{case}"))
        }
        PropValue::Color(c) => {
            if old.kind() == "scoped_identifier" {
                let path = old.child_by_field_name("path").map(|p| &text[p.byte_range()])?;
                if short_type(path) != "Color" {
                    return None;
                }
                if let Some((name, _)) = NAMED.iter().find(|(_, v)| v == c) {
                    return Some(format!("{path}::{name}"));
                }
                return Some(rs_color(path, *c, hex_colors(text)));
            }
            let (ty, func, args) = path_call(old, text)?;
            if ty != "Color" {
                return None;
            }
            let path = &old_text[..old_text.rfind("::")?];
            let hex = text[args.byte_range()].contains("0x");
            match func {
                "new" => Some(format!("{path}::new(0x{c:08X})")),
                "from_rgb" | "from_argb" => Some(rs_color(path, *c, hex)),
                _ => None,
            }
        }
        PropValue::Thickness(t) => {
            let numbers = |n: Node| named(n).iter().all(|x| is_number(*x));
            if is_number(old) || (old.kind() == "tuple_expression" && numbers(old)) {
                return Some(rs_thickness(*t));
            }
            None
        }
        _ => None,
    }
}

// ---------------------------------------------------------------- writing

/// One parsed file set and the match's control in it.
struct Located<'t> {
    files: Vec<RsFile<'t>>,
    file: usize,
    node: Node<'t>,
    /// The calls that return the control, innermost first, each with its file: their chains go on it.
    outer: Vec<(usize, Node<'t>)>,
}

fn locate<'t>(files: &'t [(String, String)], trees: &'t [Tree], m: &RuntimeMatch) -> Result<Located<'t>, Refusal> {
    let parsed = parse_all(files, trees);
    let file = parsed.iter().position(|f| f.uri == m.uri).ok_or_else(|| Refusal::new("src-no-code", &[]))?;
    let node = *parsed[file].ix.nodes.get(&m.key).ok_or_else(|| Refusal::new("src-key-missing", &[("key", &m.key)]))?;
    // From the innermost call out, the ones whose function gives back (a chain on) what the call
    // before it made.
    let mut outer = vec![];
    let mut target = node;
    for key in m.via.iter().rev() {
        let found = std::iter::once(file).chain((0..parsed.len()).filter(|&f| f != file)).find_map(|f| parsed[f].ix.nodes.get(key).map(|n| (f, *n)));
        let Some((cf, call)) = found else { break };
        let Some((name, _)) = fn_call(call, parsed[cf].text) else { break };
        let Some((_, func)) = find_fn(&parsed, cf, name) else { break };
        if returned(func).map(|r| chain_base(strip_parens(r))) != Some(target) {
            break;
        }
        outer.push((cf, call));
        target = call;
    }
    Ok(Located { files: parsed, file, node, outer })
}

impl<'t> Located<'t> {
    /// Every chain on the control in the order they apply: its own, then each returning call's.
    fn chains(&self) -> Vec<(usize, Node<'t>)> {
        std::iter::once((self.file, self.node)).chain(self.outer.iter().copied()).collect()
    }

    /// The chain only this use of the control has: the outermost call that returns it, else its own.
    fn own_chain(&self) -> (usize, Node<'t>) {
        self.outer.last().copied().unwrap_or((self.file, self.node))
    }
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

/// A call added at the end of the chain on `base`: on its own line when the chain's calls are.
fn append_call(text: &str, base: Node, call: &str) -> (usize, usize, String) {
    let calls = chain_calls(base);
    let top = chain_top(base);
    let lines = Lines::new(text);
    if let Some((_, name, _)) = calls.last() {
        // The `.` before the last call's name.
        let dot = text[..name.start_byte()].rfind('.').unwrap_or(name.start_byte());
        let before = &text[..dot];
        if before.trim_end_matches([' ', '\t']).ends_with('\n') {
            let indent = line_indent(text, &lines, dot);
            return (top.end_byte(), top.end_byte(), format!("{}{indent}.{call}", newline(text)));
        }
    }
    (top.end_byte(), top.end_byte(), format!(".{call}"))
}

/// [`crate::set_property`] for a control made by Rust. Only the edited control changes, never another
/// made by the same code: for a control a function returns (`tag("Two")`), the value goes on that
/// call (its `.prop(..)` there, else a new `.prop(value)` after it), or into the argument of that call
/// that feeds it; never into the function's body. A control written in place takes the argument of
/// its last `.prop(..)`, its constructor's argument for its text or source, or a new call at the end of
/// its chain; inside a function used more than once, only through that function's arguments.
pub(crate) fn set_property(files: &[(String, String)], m: &RuntimeMatch, prop: &str, value: &PropValue) -> Result<CodeEdit, Refusal> {
    let trees: Vec<Tree> = files.iter().map(|(_, t)| parse_rs(t)).collect();
    let at = locate(files, &trees, m)?;
    let attr_key = format!("{}/{prop}", m.key);
    let not_literal = || Refusal::new("src-not-literal", &[("key", &attr_key)]);
    // The functions this use goes through that make other controls with the same code: those called
    // more than once, outside the calls that return the control.
    let enclosing = &m.via[..m.via.len() - at.outer.len()];
    let shared = shared_helper(&at, enclosing);
    let write_at = |f: usize, arg: Node<'_>| -> Result<CodeEdit, Refusal> {
        let text = at.files[f].text;
        let new_text = respell(arg, text, value).ok_or_else(not_literal)?;
        Ok(finish(&at.files[f].uri, text, arg.start_byte(), arg.end_byte(), new_text.clone(), &new_text))
    };
    // 1. The calls that return it, the outermost (applied last) first.
    for (f, base) in at.outer.iter().rev() {
        let text = at.files[*f].text;
        if let Some((_, _, args)) = chain_calls(*base).into_iter().rev().find(|(_, n, _)| &text[n.byte_range()] == prop) {
            if let Some(h) = &shared {
                return Err(h.clone());
            }
            let arg = single(args).ok_or_else(not_literal)?;
            let (f, arg) = through_params(&at, *f, arg, m).ok_or_else(not_literal)?;
            return write_at(f, arg);
        }
    }
    // 2. Its own chain, or its constructor's text or source.
    let text = at.files[at.file].text;
    let ty = at.files[at.file].ix.types.get(&at.node.id()).cloned().unwrap_or_default();
    let mut own = chain_calls(at.node).into_iter().rev().find(|(_, n, _)| &text[n.byte_range()] == prop).map(|(_, _, a)| a);
    if own.is_none()
        && ctor_prop(&ty) == Some(prop)
        && let Some((_, "new", args)) = path_call(at.node, text)
        && named(args).len() == 1
    {
        own = Some(args);
    }
    if let Some(args) = own {
        // A value the designer cannot follow: for a control a call returns, the value goes on that
        // call (below); written in place, it is refused.
        let found = single(args).and_then(|arg| through_params(&at, at.file, arg, m).map(|t| (arg, t)));
        let Some((arg, (f, through))) = found.or_else(|| (!at.outer.is_empty()).then_some((at.node, (at.file, at.node)))) else { return Err(not_literal()) };
        // A parameter: the argument of this use's call; one that is no literal (a constant passed
        // in) gets the value after the call instead, on this use only.
        if through.id() != arg.id() {
            match write_at(f, through) {
                Err(_) if !at.outer.is_empty() => {}
                done => return done,
            }
        }
        // A literal in place, when no other control is made by it.
        if at.outer.is_empty() && through.id() == arg.id() {
            if let Some(h) = shared {
                return Err(h);
            }
            return write_at(f, through);
        }
    }
    // 3. A new call on the chain only this use has (after the call that returns it).
    if let Some(h) = shared {
        return Err(h);
    }
    let (f, base) = at.own_chain();
    let text = at.files[f].text;
    let spelled = spell(value, text).ok_or_else(|| Refusal::new("src-value-misfit", &[("key", &attr_key)]))?;
    let call = format!("{prop}({spelled})");
    let (a, b, t) = append_call(text, base, &call);
    Ok(finish(&at.files[f].uri, text, a, b, t, &call))
}

/// The one argument of a call.
fn single(args: Node) -> Option<Node> {
    match named(args).as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

/// The refusal when one of the functions `calls` go through is called more than once: its code
/// makes a control for each use, so a value written there would change them all.
fn shared_helper(at: &Located, calls: &[String]) -> Option<Refusal> {
    for key in calls {
        let Some((f, call)) = at.files.iter().enumerate().find_map(|(f, file)| file.ix.nodes.get(key).map(|n| (f, *n))) else { continue };
        let Some((name, _)) = fn_call(call, at.files[f].text) else { continue };
        let uses = at.files.iter().map(|file| file.ix.nodes.values().filter(|n| fn_call(**n, file.text).is_some_and(|(x, _)| x == name)).count()).sum::<usize>();
        if uses > 1 {
            return Some(Refusal::new("src-shared-helper", &[("helper", &name), ("count", &uses.to_string())]));
        }
    }
    None
}

/// `arg` itself, or, while it is a local, its value, and while it is a parameter of the function it
/// is in, the argument the matching call of `m.via` passes for it.
fn through_params<'t>(at: &Located<'t>, mut f: usize, mut arg: Node<'t>, m: &RuntimeMatch) -> Option<(usize, Node<'t>)> {
    let mut calls: Vec<(usize, Node<'t>)> = m.via.iter().rev().filter_map(|k| at.files.iter().enumerate().find_map(|(cf, file)| file.ix.nodes.get(k).map(|n| (cf, *n)))).collect();
    loop {
        let text = at.files[f].text;
        if arg.kind() != "identifier" {
            return Some((f, arg));
        }
        let name = &text[arg.byte_range()];
        if let Some(v) = local_let(arg, name, text) {
            arg = strip_parens(v);
            continue;
        }
        let func = enclosing_fn(arg)?;
        let i = param_names(func, text).iter().position(|p| p == name)?;
        let fname = func.child_by_field_name("name").map(|n| &text[n.byte_range()])?;
        let k = calls.iter().position(|(cf, c)| fn_call(*c, at.files[*cf].text).is_some_and(|(n, _)| n == fname))?;
        let (cf, call) = calls.remove(k);
        let (_, args) = fn_call(call, at.files[cf].text)?;
        arg = strip_parens(*named(args).get(i)?);
        f = cf;
    }
}

/// A new control as Rust: its constructor (with its text or source as the argument), its values as
/// builder calls, its children in `.children(..)`, laid out over lines below `indent`.
pub(crate) enum NewControl<'a> {
    Spec { spec: &'a ControlSpec, root_types: &'a [(String, String)] },
    /// Written as it is (a moved control's own text).
    Text(&'a str),
}

fn spec_text(spec: &ControlSpec, root_types: &[(String, String)], indent: &str, unit: &str, nl: &str, text: &str) -> Result<String, Refusal> {
    let ty = short_type(&spec.type_name).to_owned();
    let mut arg = None;
    let mut calls = vec![];
    for p in &spec.props {
        let t = if p.type_name.is_empty() { root_types.iter().find(|(n, _)| *n == p.name).map(|(_, t)| t.as_str()) } else { Some(p.type_name.as_str()) };
        let t = t.ok_or_else(|| Refusal::new("src-insert-type-unknown", &[("prop", &p.name)]))?;
        let v = value_from_runtime(t, &p.value, &p.value)?;
        let spelled = spell(&v, text).ok_or_else(|| Refusal::new("src-type-unsupported", &[("type", &t)]))?;
        if ctor_prop(&ty) == Some(p.name.as_str()) && arg.is_none() {
            arg = Some(spelled);
        } else {
            calls.push(format!(".{}({spelled})", p.name));
        }
    }
    let head = constructor(&ty, arg);
    if spec.children.is_empty() {
        let line = format!("{head}{}", calls.concat());
        if line.chars().count() < 100 {
            return Ok(line);
        }
    }
    let inner = format!("{indent}{unit}");
    let mut out = head;
    for c in calls {
        out.push_str(&format!("{nl}{inner}{c}"));
    }
    if !spec.children.is_empty() {
        let deeper = format!("{inner}{unit}");
        out.push_str(&format!("{nl}{inner}.children(("));
        for c in &spec.children {
            out.push_str(&format!("{nl}{deeper}{},", spec_text(c, &[], &deeper, unit, nl, text)?));
        }
        out.push_str(&format!("{nl}{inner}))"));
    }
    Ok(out)
}

fn new_control(new: &NewControl, indent: &str, unit: &str, nl: &str, text: &str) -> Result<String, Refusal> {
    match new {
        NewControl::Spec { spec, root_types } => spec_text(spec, root_types, indent, unit, nl, text),
        NewControl::Text(t) => Ok(t.to_string()),
    }
}

/// The type a new control is of: a spec's, `None` for moved text.
fn new_type(new: &NewControl) -> Option<String> {
    match new {
        NewControl::Spec { spec, .. } => Some(short_type(&spec.type_name).to_owned()),
        NewControl::Text(_) => None,
    }
}

/// The most children a tuple takes (`IntoChildren` is implemented up to 12).
const TUPLE_MAX: usize = 12;

/// [`crate::insert_spec`] for a parent made by Rust: the new control among the items of its
/// `.children(..)`, at the siblings' layout; a parent without children gets `.children(..)`.
pub(crate) fn insert(files: &[(String, String)], tree: &RuntimeTree, matches: &HashMap<NodeId, RuntimeMatch>, parent: NodeId, index: usize, new: NewControl) -> Result<crate::insert::Splice, Refusal> {
    let m = matches.get(&parent).ok_or_else(|| Refusal::new("src-no-code", &[]))?;
    let kids: Vec<NodeId> = tree.nodes.iter().filter(|n| n.parent == Some(parent) && !n.internal).map(|n| n.id).collect();
    let trees: Vec<Tree> = files.iter().map(|(_, t)| parse_rs(t)).collect();
    let at = locate(files, &trees, m)?;
    // Its children lists, in every chain on it.
    let mut lists = vec![];
    for (f, base) in at.chains() {
        let text = at.files[f].text;
        for (call, name, args) in chain_calls(base) {
            if matches!(&text[name.byte_range()], "children" | "child" | "content") {
                lists.push((f, call, args));
            }
        }
        if chain_calls(base).iter().any(|(_, n, _)| matches!(&text[n.byte_range()], "items" | "item_template")) {
            return Err(Refusal::new("src-insert-templated", &[]));
        }
    }
    let (f, args) = match lists.as_slice() {
        [] => {
            if !kids.is_empty() {
                return Err(Refusal::new("src-insert-unmapped", &[]));
            }
            let (f, base) = at.own_chain();
            let text = at.files[f].text;
            let lines = Lines::new(text);
            let indent = line_indent(text, &lines, base.start_byte()).to_string();
            let u = if indent.contains('\t') { "\t" } else { "    " };
            let el = new_control(&new, &format!("{indent}{u}"), u, newline(text), text)?;
            let ty = at.files[at.file].ix.types.get(&at.node.id()).cloned().unwrap_or_default();
            let method = if ty == "SkiaScroll" { "content" } else { "children" };
            let call = format!("{method}({el})");
            let (a, b, t) = append_call(text, base, &call);
            return Ok(crate::insert::Splice::new(&at.files[f].uri, a, b, t, &el));
        }
        [(f, _, args)] => (*f, *args),
        _ => return Err(Refusal::new("src-insert-unmapped", &[])),
    };
    let Some(&arg) = named(args).first() else { return Err(Refusal::new("src-insert-no-list", &[])) };
    // `.children(children)` in a helper: what its caller passes.
    let (f, arg) = through_params(&at, f, strip_parens(arg), m).ok_or_else(|| Refusal::new("src-insert-no-list", &[]))?;
    let file = &at.files[f];
    let text = file.text;
    let (container, items) = match arg.kind() {
        "tuple_expression" | "array_expression" => (Some(arg), named(arg)),
        _ => (None, vec![arg]),
    };
    // Each runtime child must be one item, in order.
    let item_key = |it: Node| file.ix.keys.get(&chain_base(strip_parens(it)).id()).cloned();
    let lined_up = kids.len() == items.len()
        && kids.iter().zip(&items).all(|(kid, it)| {
            let (Some(km), Some(key)) = (matches.get(kid), item_key(*it)) else { return false };
            km.uri == file.uri && (km.key == key || km.via.contains(&key))
        });
    if !lined_up {
        return Err(Refusal::new("src-insert-unmapped", &[]));
    }
    let is_vec = arg.kind() == "array_expression";
    if is_vec
        && let Some(t) = new_type(&new)
    {
        // `vec![..]` holds one type.
        let mismatch = items.iter().any(|it| file.ix.types.get(&chain_base(strip_parens(*it)).id()) != Some(&t));
        if mismatch {
            let had = items.first().and_then(|it| file.ix.types.get(&chain_base(strip_parens(*it)).id())).cloned().unwrap_or_default();
            return Err(Refusal::new("src-insert-vec-type", &[("type", &had), ("new", &t)]));
        }
    }
    if !is_vec && items.len() >= TUPLE_MAX {
        return Err(Refusal::new("src-insert-tuple-full", &[("count", &TUPLE_MAX.to_string())]));
    }
    let lines = Lines::new(text);
    let nl = newline(text);
    let i = index.min(items.len());
    let span_start = container.unwrap_or(arg).start_byte();
    let multiline = items.iter().any(|it| lines.line_of(it.start_byte()) != lines.line_of(span_start));
    let indent = line_indent(text, &lines, items[i.min(items.len() - 1)].start_byte()).to_string();
    let unit = if indent.contains('\t') { "\t" } else { "    " };
    let el = new_control(&new, if multiline { &indent } else { line_indent(text, &lines, span_start) }, unit, nl, text)?;
    let Some(container) = container else {
        // One child: it and the new one become a tuple.
        let one = &text[arg.byte_range()];
        let t = if i == 0 { format!("({el}, {one})") } else { format!("({one}, {el})") };
        return Ok(crate::insert::Splice::new(&file.uri, arg.start_byte(), arg.end_byte(), t, &el));
    };
    if items.is_empty() {
        let open = container.child(0).expect("open token");
        let close = container.child(container.child_count() - 1).expect("close token");
        let t = if is_vec { el.clone() } else { format!("{el},") };
        return Ok(crate::insert::Splice::new(&file.uri, open.end_byte(), close.start_byte(), t, &el));
    }
    let (a, t) = if i < items.len() {
        (items[i].start_byte(), if multiline { format!("{el},{nl}{indent}") } else { format!("{el}, ") })
    } else {
        let last = items[items.len() - 1];
        match (comma_after(last), multiline) {
            (Some(c), true) => (c.end_byte(), format!("{nl}{indent}{el},")),
            (None, true) => (last.end_byte(), format!(",{nl}{indent}{el}")),
            // `(a,)`: a one-item tuple's comma stays after the last item.
            (Some(c), false) => (c.end_byte(), format!(" {el},")),
            (None, false) => (last.end_byte(), format!(", {el}")),
        }
    };
    Ok(crate::insert::Splice::new(&file.uri, a, a, t, &el))
}

/// `x` with the parentheses around it.
fn up(mut x: Node) -> Node {
    while let Some(p) = x.parent().filter(|p| p.kind() == "parenthesized_expression") {
        x = p;
    }
    x
}

/// For a copy of control `source` of `tree`: when a call of the app's own function makes it
/// (`swatch(CORAL, "coral")`, Rust), that call with its chain, as the code has it; the copy is written
/// as the same call. `None` for a control written in place.
pub fn copy_call(files: &[(String, String)], tree: &RuntimeTree, source: NodeId) -> Option<String> {
    let rs: Vec<(String, String)> = files.iter().filter(|(u, _)| is_rust(u)).cloned().collect();
    let m = crate::match_runtime(&rs, tree).into_iter().find(|m| m.node == source && is_rust(&m.uri))?;
    let trees: Vec<Tree> = rs.iter().map(|(_, t)| parse_rs(t)).collect();
    let at = locate(&rs, &trees, &m).ok()?;
    let (f, call) = *at.outer.last()?;
    let top = chain_top(call);
    Some(at.files[f].text[call.start_byte()..top.end_byte()].to_owned())
}

/// The cut that takes the item `key` makes (a control, or a call of the app's function that returns
/// one) out of its children list: bytes `a..b` and what stays there, and the item's own bytes. The
/// last item takes its whole `.children(..)` call with it (an empty tuple or `vec![]` is no list
/// of children).
pub(crate) fn cut(text: &str, key: &str) -> Option<(usize, usize, String, std::ops::Range<usize>)> {
    let ts = parse_rs(text);
    let ix = Index::new(&ts, text);
    let n = *ix.nodes.get(key)?;
    let top = chain_top(n);
    let item = up(top);
    let p = item.parent()?;
    let list = matches!(p.kind(), "tuple_expression" | "array_expression").then_some(p);
    if let Some(p) = list {
        let its = named(p);
        if its.len() > 1 {
            let k = its.iter().position(|&x| x == item)?;
            let (a, b) = if k + 1 < its.len() {
                (item.start_byte(), its[k + 1].start_byte())
            } else {
                let prev = its[k - 1];
                match (comma_after(item), comma_after(prev)) {
                    (Some(own), Some(pc)) => (pc.end_byte(), own.end_byte()),
                    _ => (prev.end_byte(), item.end_byte()),
                }
            };
            return Some((a, b, String::new(), top.byte_range()));
        }
    }
    // The only child: the whole `.children(..)` call goes.
    let args = up(list.unwrap_or(item)).parent().filter(|a| a.kind() == "arguments")?;
    let call = args.parent()?;
    let (_, name, _) = method_call(call)?;
    if !matches!(&text[name.byte_range()], "children" | "child" | "content") {
        return None;
    }
    let dot = text[..name.start_byte()].rfind('.')?;
    let start = text[..dot].trim_end_matches([' ', '\t', '\r', '\n']).len();
    Some((start, call.end_byte(), String::new(), top.byte_range()))
}

/// [`crate::remove_element`] for a control made by Rust: its item in the children list.
pub(crate) fn remove(files: &[(String, String)], m: &RuntimeMatch) -> Result<CodeEdit, Refusal> {
    let text = files.iter().find(|(u, _)| *u == m.uri).expect("matched file").1.as_str();
    let (a, b, fill, _) = std::iter::once(&m.key).chain(m.via.iter().rev()).find_map(|k| cut(text, k)).ok_or_else(|| Refusal::new("src-remove-not-in-list", &[("key", &m.key)]))?;
    let mut new = text.to_string();
    new.replace_range(a..b, &fill);
    let range = Lines::new(&new).range(a, a);
    Ok(CodeEdit { uri: m.uri.clone(), text: new, range })
}

#[cfg(test)]
#[path = "rs_tests.rs"]
mod tests;
