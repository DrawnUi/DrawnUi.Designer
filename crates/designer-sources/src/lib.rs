//! designer-sources: reads fluent DrawnUI C# into a [`SourceTree`] with tree-sitter, and answers a
//! [`SourceEdit`] with the [`TextEdit`]s that make it, touching nothing outside the edited span.
//!
//! What becomes a node:
//! - every `new T(...) { ... }` / `new() { ... }` is an Element, except a pure collection
//!   (`new List<T> { a, b }`), whose items count as items of the enclosing property;
//! - every `Name = value` in an element's object initializer is an Attribute;
//! - under an attribute, a literal value is a Literal (numbers, strings, chars, bools, negative
//!   numbers, `Colors.White` / `LayoutType.Column`), an element value is that Element, a
//!   collection value (`{ ... }`, `[ ... ]`, `new List<T> { ... }`) gives one node per item
//!   (Element, or Reference for anything else), and any other value is a Reference.
//! - Elements outside a property (a statement in a constructor, a lambda in a fluent chain) are
//!   top-level, keyed by the enclosing type and member.
//!
//! Keys: `Type.Member/i` for the i-th top-level element of a member, `<element>/Prop` for an
//! attribute, `<attribute>/i` for the i-th item of its value (`/0` for a single element) and
//! `<attribute>/=` for its literal or reference value.

use std::collections::{HashMap, HashSet};

use designer_protocol::source::*;
use tree_sitter::{Node, Parser, Tree};

mod codeset;
mod insert;
mod matcher;
mod rs;
mod setcode;
mod tsx;
pub use rs::{copy_call, is_rust, read_rust};
pub use tsx::{edit_tsx, is_tsx, read_tsx};
pub use insert::{children_forms, insert_element, insert_spec, insert_text, move_element, remove_element};
pub use codeset::{ElementCode, code_set_names, element_code};
pub use matcher::{RuntimeMatch, match_runtime};
pub use setcode::{CodeEdit, PropValue, set_property, value_from_runtime};

/// Reads a C# file into its source tree.
pub fn read_csharp(uri: &str, text: &str) -> SourceTree {
    let tree = parse(text);
    walk(&tree, text, uri).0
}

/// Answers one edit aimed at the node `key` of `tree`, read earlier from `text`.
pub fn edit_csharp(text: &str, tree: &SourceTree, key: &str, edit: &SourceEdit) -> SourceEditAnswer {
    if !tree.nodes.iter().any(|n| n.key == key) {
        return refuse(SourceEditRefusal::UnknownNode);
    }
    let ts = parse(text);
    let (fresh, handles) = walk(&ts, text, &tree.uri);
    let Some(i) = fresh.nodes.iter().position(|n| n.key == key) else {
        return refuse(SourceEditRefusal::UnknownNode);
    };
    let lines = Lines::new(text);
    match plan(text, &fresh, &handles, i, edit) {
        Ok(edits) => SourceEditAnswer {
            edits: edits
                .into_iter()
                .map(|(a, b, new_text)| TextEdit { range: lines.range(a, b), new_text })
                .collect(),
            refused: None,
        },
        Err(r) => refuse(r),
    }
}

fn refuse(r: SourceEditRefusal) -> SourceEditAnswer {
    SourceEditAnswer { edits: vec![], refused: Some(r) }
}

fn parse(text: &str) -> Tree {
    let mut p = Parser::new();
    p.set_language(&tree_sitter_c_sharp::LANGUAGE.into()).expect("tree-sitter-c-sharp ABI");
    p.parse(text, None).expect("parser without timeout always returns a tree")
}

// ---------------------------------------------------------------- positions

struct Lines<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Self {
        let starts = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
        Self { text, starts }
    }
    fn line_of(&self, byte: usize) -> usize {
        self.starts.partition_point(|&s| s <= byte) - 1
    }
    fn pos(&self, byte: usize) -> TextPosition {
        let line = self.line_of(byte);
        let character = self.text[self.starts[line]..byte].encode_utf16().count() as u32;
        TextPosition { line: line as u32, character }
    }
    fn range(&self, a: usize, b: usize) -> EditorRange {
        EditorRange { start: self.pos(a), end: self.pos(b) }
    }
}

// ---------------------------------------------------------------- syntax helpers

fn is_creation(n: Node) -> bool {
    matches!(n.kind(), "object_creation_expression" | "implicit_object_creation_expression")
}

fn child_of_kind<'t>(n: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut c = n.walk();
    n.named_children(&mut c).find(|x| x.kind() == kind)
}

fn initializer(creation: Node) -> Option<Node> {
    child_of_kind(creation, "initializer_expression")
}

/// The items of an `{ ... }` initializer or a `[ ... ]` collection expression, comments skipped.
fn items(container: Node) -> Vec<Node> {
    let mut c = container.walk();
    container.named_children(&mut c).filter(|x| x.kind() != "comment").collect()
}

/// `new List<T> { a, b }`: an initializer of items, none of them `Name = value`.
fn is_collection_creation(n: Node) -> bool {
    is_creation(n)
        && initializer(n).is_some_and(|i| {
            let it = items(i);
            !it.is_empty() && it.iter().all(|x| x.kind() != "assignment_expression")
        })
}

fn is_element(n: Node) -> bool {
    is_creation(n) && !is_collection_creation(n)
}

/// The creation at the bottom of `new X { }.Assign(out _x).OnTapped(...)`, or `n` itself.
fn chain_base(mut n: Node) -> Node {
    loop {
        match n.kind() {
            "invocation_expression" => match n.child_by_field_name("function") {
                Some(f) if f.kind() == "member_access_expression" => {
                    n = f.child_by_field_name("expression").unwrap_or(f)
                }
                _ => return n,
            },
            "parenthesized_expression" => match n.named_child(0) {
                Some(x) => n = x,
                None => return n,
            },
            _ => return n,
        }
    }
}

/// The whole fluent chain built on `n`: the inverse of [`chain_base`].
fn chain_top(mut n: Node) -> Node {
    while let Some(p) = n.parent() {
        let up = match p.kind() {
            "member_access_expression" if p.child_by_field_name("expression") == Some(n) => {
                p.parent().filter(|pp| {
                    pp.kind() == "invocation_expression" && pp.child_by_field_name("function") == Some(p)
                })
            }
            "parenthesized_expression" => Some(p),
            _ => None,
        };
        match up {
            Some(u) => n = u,
            None => break,
        }
    }
    n
}

fn call_args(call: Node) -> Vec<Node> {
    let mut c = call.walk();
    call.child_by_field_name("arguments")
        .map(|a| a.named_children(&mut c).filter(|x| x.kind() == "argument").collect())
        .unwrap_or_default()
}

/// The expression of a collection item or an argument.
fn expr_of(item: Node) -> Node {
    match item.kind() {
        "collection_element" => child_of_kind(item, "expression_element").and_then(|e| e.named_child(0)).unwrap_or(item),
        "argument" => item.named_child((item.named_child_count() as u32).saturating_sub(1)).unwrap_or(item),
        _ => item,
    }
}

/// `Helpers.Card`, `DrawnUi.Draw.Helpers`: identifiers joined by dots.
fn is_dotted(n: Node) -> bool {
    match n.kind() {
        "identifier" | "this" => true,
        "member_access_expression" | "qualified_name" => {
            n.child_by_field_name("expression").or_else(|| n.child_by_field_name("qualifier")).is_none_or(is_dotted)
        }
        _ => false,
    }
}

/// The method call a value is: `Card(..)`, `Helpers.Card(..)`, `this.Card(..)`, also at the
/// bottom of a fluent chain (`Card(..).Assign(out _c)`).
fn call_of(mut n: Node) -> Option<Node> {
    while n.kind() == "parenthesized_expression" {
        n = n.named_child(0)?;
    }
    while n.kind() == "invocation_expression" {
        let f = n.child_by_field_name("function")?;
        match f.child_by_field_name("expression") {
            Some(inner) if f.kind() == "member_access_expression" && inner.kind() == "invocation_expression" => n = inner,
            _ => return Some(n),
        }
    }
    None
}

/// `T M() => new(..)` / `return new(..);`: the declared return type `T`.
fn returned_type(expr: Node) -> Option<Node> {
    let p = expr.parent().filter(|p| matches!(p.kind(), "arrow_expression_clause" | "return_statement"))?;
    let mut n = p.parent();
    while let Some(x) = n {
        match x.kind() {
            "method_declaration" => return x.child_by_field_name("returns"),
            "local_function_statement" | "property_declaration" => {
                return x.child_by_field_name("type").or_else(|| x.child_by_field_name("returns"));
            }
            "lambda_expression" | "anonymous_method_expression" => return None,
            _ => n = x.parent(),
        }
    }
    None
}

/// The element an item of a collection is, if it is one.
fn item_element(item: Node) -> Option<Node> {
    let expr = if item.kind() == "collection_element" {
        child_of_kind(item, "expression_element")?.named_child(0)?
    } else {
        item
    };
    let base = chain_base(expr);
    is_element(base).then_some(base)
}

/// A property value that is a collection: its container and items.
fn value_items(value: Node) -> Option<(Node, Vec<Node>)> {
    match value.kind() {
        "initializer_expression" | "collection_expression" => Some((value, items(value))),
        "array_creation_expression" | "implicit_array_creation_expression" => {
            let i = initializer(value)?;
            Some((i, items(i)))
        }
        _ if is_collection_creation(value) => {
            let i = initializer(value)?;
            Some((i, items(i)))
        }
        _ => None,
    }
}

/// The collection holding the element whose chain is `top`, and the item it is.
fn collection_slot(top: Node) -> Option<(Node, Node)> {
    let p = top.parent()?;
    match p.kind() {
        "initializer_expression" => Some((p, top)),
        "expression_element" => {
            let item = p.parent().filter(|x| x.kind() == "collection_element")?;
            Some((item.parent()?, item))
        }
        _ => None,
    }
}

fn is_upper_ident(n: Node, text: &str) -> bool {
    n.kind() == "identifier" && n.utf8_text(text.as_bytes()).is_ok_and(|s| s.starts_with(char::is_uppercase))
}

/// `Colors.White`, `LayoutType.Column`, `DrawnUi.Draw.LayoutType.Column`: a dotted name of
/// capitalized identifiers. A heuristic: a capitalized field of a capitalized type also matches.
fn is_case_access(n: Node, text: &str) -> bool {
    if n.kind() != "member_access_expression" {
        return false;
    }
    let (Some(e), Some(name)) = (n.child_by_field_name("expression"), n.child_by_field_name("name")) else {
        return false;
    };
    is_upper_ident(name, text) && (is_upper_ident(e, text) || is_case_access(e, text))
}

fn is_number(n: Node) -> bool {
    matches!(n.kind(), "integer_literal" | "real_literal")
}

fn is_literal(n: Node, text: &str) -> bool {
    match n.kind() {
        "integer_literal" | "real_literal" | "string_literal" | "verbatim_string_literal" | "raw_string_literal"
        | "character_literal" | "boolean_literal" => true,
        "prefix_unary_expression" => {
            n.child(0).is_some_and(|op| op.kind() == "-") && n.named_child(0).is_some_and(is_number)
        }
        _ => is_case_access(n, text),
    }
}

// ---------------------------------------------------------------- reading

struct Walker<'t> {
    text: &'t str,
    lines: Lines<'t>,
    nodes: Vec<SourceNode>,
    handles: Vec<Node<'t>>,
    keys: HashSet<String>,
    types: Vec<String>,
    member: String,
    members_seen: HashMap<String, usize>,
    loose: HashMap<String, usize>,
    /// The implicit `this` element of the constructor being read.
    this_elem: Option<usize>,
}

fn walk<'t>(tree: &'t Tree, text: &'t str, uri: &str) -> (SourceTree, Vec<Node<'t>>) {
    let mut w = Walker {
        text,
        lines: Lines::new(text),
        nodes: vec![],
        handles: vec![],
        keys: HashSet::new(),
        types: vec![],
        member: String::new(),
        members_seen: HashMap::new(),
        loose: HashMap::new(),
        this_elem: None,
    };
    w.scan(tree.root_node());
    // A constructor's `Children.Add(...)` calls form one attribute spanning other statements, so
    // emission order is not start order: sort, keeping parents first on equal starts.
    let mut order: Vec<usize> = (0..w.nodes.len()).collect();
    order.sort_by_key(|&i| (w.nodes[i].range.start.line, w.nodes[i].range.start.character, i));
    let mut new_of = vec![0; order.len()];
    for (new, &old) in order.iter().enumerate() {
        new_of[old] = new;
    }
    let nodes = order
        .iter()
        .map(|&old| SourceNode { parent: w.nodes[old].parent.map(|p| new_of[p]), ..w.nodes[old].clone() })
        .collect();
    let handles = order.iter().map(|&old| w.handles[old]).collect();
    (SourceTree { uri: uri.into(), nodes }, handles)
}

/// The property a constructor or initializer assigns: `X` or `this.X`.
fn prop_name<'t>(left: Node, text: &'t str) -> Option<&'t str> {
    match left.kind() {
        "identifier" => Some(&text[left.byte_range()]),
        "member_access_expression" if left.child(0)?.kind() == "this" => {
            Some(&text[left.child_by_field_name("name")?.byte_range()])
        }
        _ => None,
    }
}

/// `X = value;` / `this.X = value;` in a constructor body. Capitalized names only: a heuristic
/// that keeps fields (`_x`, `count`) out.
fn this_assignment<'t>(s: Node<'t>, text: &str) -> Option<Node<'t>> {
    let a = s.named_child(0).filter(|a| s.kind() == "expression_statement" && a.kind() == "assignment_expression")?;
    if a.child(1)?.kind() != "=" {
        return None;
    }
    prop_name(a.child_by_field_name("left")?, text).filter(|n| n.starts_with(char::is_uppercase))?;
    Some(a)
}

/// `Children.Add(x);` / `this.Children.Add(x);` / `AddSubView(x);` / `this.AddSubView(x);`.
fn this_add<'t>(s: Node<'t>, text: &str) -> Option<Node<'t>> {
    let inv = s.named_child(0).filter(|a| s.kind() == "expression_statement" && a.kind() == "invocation_expression")?;
    let f = inv.child_by_field_name("function")?;
    let ok = match f.kind() {
        "identifier" => &text[f.byte_range()] == "AddSubView",
        "member_access_expression" => {
            let name = &text[f.child_by_field_name("name")?.byte_range()];
            (name == "AddSubView" && f.child(0)?.kind() == "this")
                || (name == "Add" && f.child_by_field_name("expression").and_then(|t| prop_name(t, text)) == Some("Children"))
        }
        _ => false,
    };
    ok.then_some(inv)
}

impl<'t> Walker<'t> {
    fn txt(&self, n: Node) -> &'t str {
        &self.text[n.byte_range()]
    }

    fn push(&mut self, role: SourceRole, n: Node<'t>, parent: Option<usize>, key: String, name: Option<String>, value: Option<String>) -> (usize, String) {
        let mut key = key;
        if self.keys.contains(&key) {
            // Only reachable for code that does not compile (a property set twice).
            let mut k = 2;
            while self.keys.contains(&format!("{key}#{k}")) {
                k += 1;
            }
            key = format!("{key}#{k}");
        }
        self.keys.insert(key.clone());
        self.nodes.push(SourceNode {
            role,
            range: self.lines.range(n.start_byte(), n.end_byte()),
            parent,
            key: key.clone(),
            name,
            value,
        });
        self.handles.push(n);
        (self.nodes.len() - 1, key)
    }

    fn member_key(&self) -> String {
        let t = self.types.join(".");
        match (t.is_empty(), self.member.is_empty()) {
            (true, true) => "<file>".into(),
            (true, false) => self.member.clone(),
            (false, true) => t,
            (false, false) => format!("{t}.{}", self.member),
        }
    }

    fn in_member(&mut self, name: &str, body: impl FnOnce(&mut Self)) {
        let saved = std::mem::replace(&mut self.member, name.into());
        let full = self.member_key();
        let seen = self.members_seen.entry(full).or_default();
        *seen += 1;
        if *seen > 1 {
            self.member = format!("{name}#{seen}");
        }
        body(self);
        self.member = saved;
    }

    fn scan_children(&mut self, n: Node<'t>) {
        let mut c = n.walk();
        let kids: Vec<_> = n.children(&mut c).collect();
        for k in kids {
            self.scan(k);
        }
    }

    /// Code outside any property: finds top-level elements.
    fn scan(&mut self, n: Node<'t>) {
        let text = self.text;
        let name_of = move |n: Node<'t>| n.child_by_field_name("name").map(|x| text[x.byte_range()].to_string());
        match n.kind() {
            "class_declaration" | "struct_declaration" | "record_declaration" | "record_struct_declaration"
            | "interface_declaration" | "enum_declaration" => {
                self.types.push(name_of(n).unwrap_or_default());
                let saved = std::mem::take(&mut self.member);
                self.scan_children(n);
                self.member = saved;
                self.types.pop();
            }
            "method_declaration" | "property_declaration" | "event_declaration" => {
                let name = name_of(n).unwrap_or_default();
                self.in_member(&name, |w| w.scan_children(n))
            }
            "constructor_declaration" => self.in_member("ctor", |w| w.constructor(n)),
            "destructor_declaration" => self.in_member("dtor", |w| w.scan_children(n)),
            "indexer_declaration" => self.in_member("this[]", |w| w.scan_children(n)),
            "operator_declaration" | "conversion_operator_declaration" => {
                self.in_member("operator", |w| w.scan_children(n))
            }
            "variable_declarator" if self.member.is_empty() && !self.types.is_empty() => {
                let name = name_of(n).unwrap_or_default();
                self.in_member(&name, |w| w.scan_children(n))
            }
            _ if is_element(n) => {
                let mk = self.member_key();
                let i = self.loose.entry(mk.clone()).or_default();
                let key = format!("{mk}/{i}");
                *i += 1;
                self.element(n, None, key);
            }
            _ => self.scan_children(n),
        }
    }

    /// A constructor that sets properties of `this` or adds children to it: one Element for
    /// `this`, its assignment statements as attributes, its `Add` calls as a `Children` attribute.
    fn constructor(&mut self, n: Node<'t>) {
        let text = self.text;
        let body = n.child_by_field_name("body").filter(|b| b.kind() == "block");
        let stmts = body.map(items).unwrap_or_default();
        let adds: Vec<_> = stmts.iter().copied().filter(|&s| this_add(s, text).is_some()).collect();
        if adds.is_empty() && !stmts.iter().any(|&s| this_assignment(s, text).is_some()) {
            return self.scan_children(n);
        }
        let class = self.types.last().cloned();
        let (this_idx, this_key) = self.push(SourceRole::Element, n, None, format!("{}/this", self.types.join(".")), class, None);
        let saved = self.this_elem.replace(this_idx);
        let mut kids: Option<(usize, String)> = None;
        let mut kid_i = 0;
        let mut c = n.walk();
        for child in n.children(&mut c).collect::<Vec<_>>() {
            if Some(child) != body {
                self.scan(child);
                continue;
            }
            let mut bc = child.walk();
            for s in child.children(&mut bc).collect::<Vec<_>>() {
                if let Some(a) = this_assignment(s, text) {
                    self.attribute(a, this_idx, &this_key);
                } else if let Some(inv) = this_add(s, text) {
                    let (aidx, akey) = match &kids {
                        Some(k) => k.clone(),
                        None => {
                            let k = self.push(SourceRole::Attribute, adds[0], Some(this_idx), format!("{this_key}/Children"), Some("Children".into()), None);
                            let last = adds[adds.len() - 1];
                            self.nodes[k.0].range = self.lines.range(adds[0].start_byte(), last.end_byte());
                            kids = Some(k.clone());
                            k
                        }
                    };
                    let mut ac = inv.walk();
                    let args: Vec<_> = inv.child_by_field_name("arguments").map(|a| a.named_children(&mut ac).collect()).unwrap_or_default();
                    for arg in args {
                        match arg.named_child((arg.named_child_count() as u32).saturating_sub(1)).map(chain_base).filter(|&b| is_element(b)) {
                            Some(el) => {
                                self.absorb(this_idx);
                                self.chained_element(el, aidx, format!("{akey}/{kid_i}"));
                            }
                            None => {
                                self.reference(arg, aidx, format!("{akey}/{kid_i}"));
                            }
                        }
                        kid_i += 1;
                    }
                } else {
                    self.scan(s);
                }
            }
        }
        self.this_elem = saved;
    }

    /// An element placed under `this` used to be a top-level element of the constructor; it
    /// keeps its slot in the counter so the top-level elements after it keep their keys.
    fn absorb(&mut self, parent: usize) {
        if self.this_elem == Some(parent) {
            *self.loose.entry(self.member_key()).or_default() += 1;
        }
    }

    fn element(&mut self, creation: Node<'t>, parent: Option<usize>, key: String) {
        let name = match creation.child_by_field_name("type") {
            Some(t) => Some(self.txt(t).to_string()),
            // `T x = new();`: the declared type.
            None => creation
                .parent()
                .filter(|p| p.kind() == "variable_declarator")
                .and_then(|p| p.parent())
                .and_then(|d| d.child_by_field_name("type"))
                .or_else(|| returned_type(chain_top(creation)))
                .filter(|t| !matches!(t.kind(), "implicit_type" | "predefined_type"))
                .map(|t| self.txt(t).to_string()),
        };
        let (idx, key) = self.push(SourceRole::Element, creation, parent, key, name, None);
        if let Some(args) = child_of_kind(creation, "argument_list") {
            self.scan(args);
        }
        if let Some(init) = initializer(creation) {
            for item in items(init) {
                match item.child_by_field_name("left") {
                    Some(l) if item.kind() == "assignment_expression" && l.kind() == "identifier" => {
                        self.attribute(item, idx, &key)
                    }
                    _ => self.scan(item),
                }
            }
        }
    }

    fn attribute(&mut self, assign: Node<'t>, parent: usize, elem_key: &str) {
        let name = prop_name(assign.child_by_field_name("left").unwrap(), self.text).unwrap_or_default().to_string();
        let (aidx, akey) = self.push(SourceRole::Attribute, assign, Some(parent), format!("{elem_key}/{name}"), Some(name), None);
        let Some(value) = assign.child_by_field_name("right") else { return };
        if is_literal(value, self.text) {
            let v = self.txt(value).to_string();
            self.push(SourceRole::Literal, value, Some(aidx), format!("{akey}/="), None, Some(v));
        } else if let Some((_, its)) = value_items(value) {
            for (i, item) in its.into_iter().enumerate() {
                match item_element(item) {
                    Some(el) => {
                        self.absorb(parent);
                        self.chained_element(el, aidx, format!("{akey}/{i}"))
                    }
                    None => {
                        self.reference(item, aidx, format!("{akey}/{i}"));
                    }
                }
            }
        } else if is_element(chain_base(value)) {
            self.absorb(parent);
            self.chained_element(chain_base(value), aidx, format!("{akey}/0"));
        } else {
            self.reference(value, aidx, format!("{akey}/="));
        }
    }

    /// A value that is not a literal or an element. A call (`Card("..", new SkiaLayer { })`) is
    /// named after the method as written, and the elements and calls among its arguments become
    /// its children, keyed `<call>/arg<i>`.
    fn reference(&mut self, item: Node<'t>, parent: usize, key: String) {
        let call = call_of(expr_of(item));
        let name = call.and_then(|c| c.child_by_field_name("function")).map(|f| {
            let plain = |n: Node| n.kind() == "identifier" || n.kind() == "generic_name" || n.kind() == "this" || is_dotted(n);
            match (f.child_by_field_name("expression"), f.child_by_field_name("name")) {
                (Some(q), Some(m)) if f.kind() == "member_access_expression" && !plain(q) => self.txt(m).to_string(),
                _ => self.txt(f).to_string(),
            }
        });
        let (idx, key) = self.push(SourceRole::Reference, item, Some(parent), key, name, None);
        let Some(call) = call else { return self.scan(item) };
        let Some(arg_list) = call.child_by_field_name("arguments") else { return self.scan(item) };
        // The receiver and the rest of the chain are scanned; the arguments are the call's children.
        self.scan_except(item, arg_list);
        let mut c = call.walk();
        let args: Vec<_> = call.child_by_field_name("arguments").map(|a| a.named_children(&mut c).collect()).unwrap_or_default();
        for (i, arg) in args.into_iter().filter(|a| a.kind() == "argument").enumerate() {
            let e = expr_of(arg);
            if is_element(chain_base(e)) {
                // It used to be a top-level element: keep its slot so the ones after keep their keys.
                *self.loose.entry(self.member_key()).or_default() += 1;
                self.chained_element(chain_base(e), idx, format!("{key}/arg{i}"));
            } else if call_of(e).is_some() {
                self.reference(arg, idx, format!("{key}/arg{i}"));
            } else if is_literal(e, self.text) {
                let v = self.txt(e).to_string();
                self.push(SourceRole::Literal, e, Some(idx), format!("{key}/arg{i}"), None, Some(v));
            } else {
                self.scan(arg);
            }
        }
    }

    /// Scans `n` except the subtree `skip`.
    fn scan_except(&mut self, n: Node<'t>, skip: Node<'t>) {
        if n == skip {
            return;
        }
        if n.byte_range().contains(&skip.start_byte()) && !is_element(n) {
            let mut c = n.walk();
            for k in n.children(&mut c).collect::<Vec<_>>() {
                self.scan_except(k, skip);
            }
        } else {
            self.scan(n);
        }
    }

    /// An element under a property, then the arguments of its fluent chain (`.OnTapped(...)`).
    fn chained_element(&mut self, el: Node<'t>, parent: usize, key: String) {
        self.element(el, Some(parent), key);
        let top = chain_top(el);
        let mut n = el;
        while n != top {
            let Some(p) = n.parent() else { break };
            if p.kind() == "invocation_expression"
                && let Some(args) = p.child_by_field_name("arguments")
            {
                self.scan(args);
            }
            n = p;
        }
    }
}

// ---------------------------------------------------------------- helpers

/// What the code calls the controls its helpers make: for each method or local function that returns
/// an element it creates itself (`SkiaControl Circle(..) => new SkiaShape { .. }`), the element's key
/// and the helper's name. Through casts, parentheses and fluent calls on the element.
pub fn helper_names(uri: &str, text: &str) -> HashMap<String, String> {
    if crate::rs::is_rust(uri) {
        return crate::rs::helper_names(uri, text);
    }
    let ts = parse(text);
    let (fresh, handles) = walk(&ts, text, uri);
    let elements: HashMap<usize, usize> = (0..fresh.nodes.len()).filter(|&i| fresh.nodes[i].role == SourceRole::Element).map(|i| (handles[i].id(), i)).collect();
    let mut helpers = vec![];
    collect_kind(ts.root_node(), "method_declaration", &mut helpers);
    collect_kind(ts.root_node(), "local_function_statement", &mut helpers);
    let mut out = HashMap::new();
    for f in helpers {
        let (Some(name), Some(mut e)) = (f.child_by_field_name("name"), returned_expr(f)) else { continue };
        loop {
            e = match e.kind() {
                "parenthesized_expression" => match e.named_child(0) {
                    Some(x) => x,
                    None => break,
                },
                "cast_expression" => match e.child_by_field_name("value") {
                    Some(x) => x,
                    None => break,
                },
                // `new X { .. }.Assign(out _x)`: the element the chain starts from.
                "invocation_expression" => match e.child_by_field_name("function").filter(|f| f.kind() == "member_access_expression").and_then(|f| f.child_by_field_name("expression")) {
                    Some(x) => x,
                    None => break,
                },
                _ => break,
            };
        }
        if let Some(&i) = elements.get(&e.id()) {
            out.insert(fresh.nodes[i].key.clone(), text[name.byte_range()].to_owned());
        }
    }
    out
}

/// The name the code gives each matched control, when a helper made it: a C# method or local
/// function that returns it ([`helper_names`]), or a React component whose top-level element it is
/// (`Card/0`). Controls written in place have none (their type is their name).
pub fn made_by(files: &[(String, String)], matches: &[RuntimeMatch]) -> HashMap<designer_protocol::NodeId, String> {
    let helpers: HashMap<String, HashMap<String, String>> = files.iter().filter(|(u, _)| !crate::tsx::is_tsx(u)).map(|(u, t)| (u.clone(), helper_names(u, t))).collect();
    made_by_named(&helpers, matches)
}

/// [`made_by`] with each C# file's [`helper_names`] read already (file uri -> names).
pub fn made_by_named(helpers: &HashMap<String, HashMap<String, String>>, matches: &[RuntimeMatch]) -> HashMap<designer_protocol::NodeId, String> {
    let mut out = HashMap::new();
    for m in matches {
        let name = if crate::tsx::is_tsx(&m.uri) {
            m.key.split_once('/').filter(|(c, i)| c.chars().next().is_some_and(char::is_uppercase) && i.chars().all(|d| d.is_ascii_digit())).map(|(c, _)| c.to_owned())
        } else {
            helpers.get(m.uri.as_str()).and_then(|n| n.get(&m.key)).cloned()
        };
        if let Some(name) = name {
            out.insert(m.node, name);
        }
    }
    out
}

/// A call to a helper method of the same file, and the element that helper returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperLink {
    /// The key of the call (a Reference node named after the method).
    pub call: String,
    /// The key of the element the helper returns.
    pub returns: String,
    /// The arguments the returned element uses directly as a value or a child.
    pub params: Vec<ParamFlow>,
}

/// An argument of a helper call that lands in the returned element as is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamFlow {
    /// The argument's index in the call.
    pub arg: usize,
    /// The parameter it binds to.
    pub param: String,
    /// The key of the node inside the returned element that is the parameter.
    pub used_at: String,
    /// The attribute of the returned element whose value is the parameter (`Text = title`).
    pub attribute: Option<String>,
    /// The call's node for the argument (`<call>/arg<i>`: a literal, an element or a call), when
    /// it has one: editing `attribute` means editing this node.
    pub arg_key: Option<String>,
}

/// Links helper calls to the elements their helpers return: syntax only, methods of the same file
/// (the calling class and its outer classes first, then static methods anywhere in the file, or
/// the class named by a qualified call), overloads told apart by argument count. A helper returns
/// an element when its single returned expression (`=> ..` or one `return ..;`) is, through fluent
/// chains and casts, an element, a local initialized with one, a parameter (the caller's argument),
/// or a call to another such helper (up to 8 deep).
pub fn resolve_helpers(tree: &SourceTree, text: &str) -> Vec<HelperLink> {
    let ts = parse(text);
    let (fresh, handles) = walk(&ts, text, &tree.uri);
    let elements: HashMap<usize, usize> = (0..fresh.nodes.len())
        .filter(|&i| fresh.nodes[i].role == SourceRole::Element)
        .map(|i| (handles[i].id(), i))
        .collect();
    let mut methods = vec![];
    collect_kind(ts.root_node(), "method_declaration", &mut methods);
    let under = |mut j: usize, e: usize| loop {
        match fresh.nodes[j].parent {
            Some(p) if p == e => break true,
            Some(p) => j = p,
            None => break false,
        }
    };
    let mut out = vec![];
    for (i, n) in fresh.nodes.iter().enumerate() {
        if n.role != SourceRole::Reference || n.name.is_none() {
            continue;
        }
        let Some(call) = call_of(expr_of(handles[i])) else { continue };
        let mut r = Resolver { text, methods: &methods, frames: vec![] };
        let Some((creation, ctx)) = r.call(call, None) else { continue };
        let Some(&e) = elements.get(&creation.id()) else { continue };
        let mut flows = vec![];
        for j in (e + 1..fresh.nodes.len()).filter(|&j| fresh.nodes[j].role == SourceRole::Reference && under(j, e)) {
            let x = expr_of(handles[j]);
            if x.kind() == "identifier"
                && let Some((arg, param)) = r.origin(ctx, &text[x.byte_range()])
            {
                let attribute = fresh.nodes[j]
                    .parent
                    .filter(|&a| fresh.nodes[a].role == SourceRole::Attribute && fresh.nodes[j].key.ends_with("/="))
                    .map(|a| fresh.nodes[a].key.clone());
                let arg_key = Some(format!("{}/arg{arg}", n.key)).filter(|k| fresh.nodes.iter().any(|x| &x.key == k));
                flows.push(ParamFlow { arg, param, used_at: fresh.nodes[j].key.clone(), attribute, arg_key });
            }
        }
        flows.sort_by_key(|f| f.arg);
        out.push(HelperLink { call: n.key.clone(), returns: fresh.nodes[e].key.clone(), params: flows });
    }
    out
}

/// One helper call being followed: the method, the call's arguments, the frame of the caller
/// (`None` for the call being resolved).
struct Frame<'t> {
    method: Node<'t>,
    args: Vec<Node<'t>>,
    params: Vec<String>,
    /// The index of a `params T[]` parameter, the last one.
    params_at: Option<usize>,
    parent: Option<usize>,
}

struct Resolver<'t, 'a> {
    text: &'a str,
    methods: &'a [Node<'t>],
    frames: Vec<Frame<'t>>,
}

impl<'t> Resolver<'t, '_> {
    /// The element a call returns, and the frame its expression belongs to.
    fn call(&mut self, call: Node<'t>, parent: Option<usize>) -> Option<(Node<'t>, Option<usize>)> {
        let k = self.enter_call(call, parent)?;
        self.expr(returned_expr(self.frames[k].method)?, Some(k))
    }

    /// A frame for the method of the file a call targets.
    fn enter_call(&mut self, call: Node<'t>, parent: Option<usize>) -> Option<usize> {
        let args = call_args(call);
        let f = call.child_by_field_name("function")?;
        // A method or local function, else a local `var make = (..) => ..;` called by name.
        let method = find_method(f, call, args.len(), self.methods, self.text)
            .or_else(|| (f.kind() == "identifier").then(|| local_lambda(call, &self.text[f.byte_range()], self.text)).flatten())?;
        self.enter(method, args, parent)
    }

    /// A frame for `method` called with `args` from frame `parent`; `None` past 8 nested calls.
    fn enter(&mut self, method: Node<'t>, args: Vec<Node<'t>>, parent: Option<usize>) -> Option<usize> {
        let (mut depth, mut p) = (0, parent);
        while let Some(k) = p {
            depth += 1;
            p = self.frames[k].parent;
        }
        if depth >= 8 {
            return None;
        }
        let pl = method.child_by_field_name("parameters");
        // `t => ..`: one implicit parameter.
        let implicit = pl.filter(|p| p.kind() == "implicit_parameter").map(|p| vec![self.text[p.byte_range()].to_string()]);
        let pl = pl.filter(|p| p.kind() != "implicit_parameter");
        let mut params: Vec<String> = implicit.unwrap_or_default();
        params.extend(pl
            .map(|pl| {
                let mut c = pl.walk();
                pl.named_children(&mut c)
                    .filter(|p| p.kind() == "parameter")
                    .filter_map(|p| p.child_by_field_name("name"))
                    .map(|x| self.text[x.byte_range()].to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default());
        // `params T[] xs` is not wrapped in a `parameter` node.
        let params_at = pl.and_then(|pl| pl.child_by_field_name("name")).map(|x| {
            params.push(self.text[x.byte_range()].to_string());
            params.len() - 1
        });
        self.frames.push(Frame { method, args, params, params_at, parent });
        Some(self.frames.len() - 1)
    }

    fn expr(&mut self, mut e: Node<'t>, ctx: Option<usize>) -> Option<(Node<'t>, Option<usize>)> {
        while let "cast_expression" | "parenthesized_expression" = e.kind() {
            e = e.child_by_field_name("value").or_else(|| e.named_child(e.named_child_count() as u32 - 1))?;
        }
        let base = chain_base(e);
        if is_element(base) {
            return Some((base, ctx));
        }
        if base.kind() == "identifier"
            && let Some(k) = ctx
        {
            let name = &self.text[base.byte_range()];
            if let Some(arg) = self.bound(k, name) {
                let parent = self.frames[k].parent;
                return self.expr(expr_of(arg), parent);
            }
            if let Some(init) = local_init(self.frames[k].method, name, self.text) {
                return self.expr(init, ctx);
            }
        }
        let c = call_of(e)?;
        self.call(c, ctx)
    }

    /// The argument frame `k`'s call passes for parameter `name`.
    fn bound(&self, k: usize, name: &str) -> Option<Node<'t>> {
        let f = &self.frames[k];
        f.args.iter().enumerate().find_map(|(i, a)| {
            let p = match a.child_by_field_name("name") {
                Some(n) => &self.text[n.byte_range()],
                None => f.params.get(i)?.as_str(),
            };
            (p == name).then_some(*a)
        })
    }

    /// The argument of the resolved call that the identifier `name`, read in frame `ctx`, is, when
    /// it passes through parameters only: its index and the parameter it binds to.
    fn origin(&self, ctx: Option<usize>, name: &str) -> Option<(usize, String)> {
        let (mut k, mut name) = (ctx?, name.to_string());
        loop {
            let arg = self.bound(k, &name)?;
            match self.frames[k].parent {
                None => return Some((self.frames[k].args.iter().position(|&a| a == arg)?, name)),
                Some(pk) => {
                    let x = expr_of(arg);
                    if x.kind() != "identifier" {
                        return None;
                    }
                    name = self.text[x.byte_range()].to_string();
                    k = pk;
                }
            }
        }
    }
}

/// The initializer of local `name` in a method body (`var box = new SkiaShape { .. };`).
fn local_init<'t>(method: Node<'t>, name: &str, text: &str) -> Option<Node<'t>> {
    let mut decls = vec![];
    collect_kind(method.child_by_field_name("body")?, "variable_declarator", &mut decls);
    let d = decls.into_iter().find(|d| d.child_by_field_name("name").is_some_and(|n| &text[n.byte_range()] == name))?;
    let v = d.named_child(d.named_child_count() as u32 - 1)?;
    (Some(v) != d.child_by_field_name("name")).then_some(v)
}

fn collect_kind<'t>(n: Node<'t>, kind: &str, out: &mut Vec<Node<'t>>) {
    if n.kind() == kind {
        out.push(n);
    }
    let mut c = n.walk();
    for k in n.children(&mut c) {
        collect_kind(k, kind, out);
    }
}

fn type_name(t: Node, text: &str) -> String {
    t.child_by_field_name("name").map(|x| text[x.byte_range()].to_string()).unwrap_or_default()
}

fn is_type_decl(n: Node) -> bool {
    matches!(n.kind(), "class_declaration" | "struct_declaration" | "record_declaration" | "record_struct_declaration")
}

/// The method of the file that `f` (a call's function, or a method group) names, seen from
/// `from`, by name and argument count.
fn find_method<'t>(f: Node, from: Node<'t>, argc: usize, methods: &[Node<'t>], text: &str) -> Option<Node<'t>> {
    let (qualifier, name_node) = match f.kind() {
        "identifier" | "generic_name" => (None, f),
        // `this.Card(..)`: same lookup as `Card(..)`.
        "member_access_expression" => (f.child_by_field_name("expression").filter(|q| q.kind() != "this"), f.child_by_field_name("name")?),
        _ => return None,
    };
    let name = match name_node.kind() {
        "generic_name" => &text[name_node.named_child(0)?.byte_range()],
        _ => &text[name_node.byte_range()],
    };
    // A local function in scope (top-level statements, a method body) comes first.
    if qualifier.is_none()
        && let Some(local) = in_scope(from, |st| {
            (st.kind() == "local_function_statement"
                && st.child_by_field_name("name").is_some_and(|n| &text[n.byte_range()] == name)
                && accepts(st, argc))
            .then_some(st)
        })
    {
        return Some(local);
    }
    let owner = |m: &Node<'t>| m.parent().and_then(|d| d.parent()).filter(|t| is_type_decl(*t));
    let fits = |m: &Node<'t>| {
        text[m.child_by_field_name("name").map(|x| x.byte_range()).unwrap_or(0..0)] == *name && accepts(*m, argc)
    };
    let candidates: Vec<Node<'t>> = match qualifier {
        Some(q) => {
            let q = &text[q.byte_range()];
            let q = q.rsplit('.').next().unwrap_or(q);
            methods.iter().copied().filter(|m| fits(m) && owner(m).is_some_and(|t| type_name(t, text) == q)).collect()
        }
        None => {
            let mut enclosing = vec![];
            let mut p = from.parent();
            while let Some(x) = p {
                if is_type_decl(x) {
                    enclosing.push(x);
                }
                p = x.parent();
            }
            let in_class = enclosing.iter().find_map(|t| {
                let found: Vec<_> = methods.iter().copied().filter(|m| fits(m) && owner(m) == Some(*t)).collect();
                (!found.is_empty()).then_some(found)
            });
            in_class.unwrap_or_else(|| {
                methods
                    .iter()
                    .copied()
                    .filter(|m| fits(m) && {
                        let mut c = m.walk();
                        m.children(&mut c).any(|x| x.kind() == "modifier" && &text[x.byte_range()] == "static")
                    })
                    .collect()
            })
        }
    };
    match candidates.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

/// Whether a method takes `argc` arguments: required ≤ argc ≤ all, or more with `params`.
fn accepts(m: Node, argc: usize) -> bool {
    let Some(pl) = m.child_by_field_name("parameters") else { return argc == 0 };
    let mut c = pl.walk();
    let kids: Vec<_> = pl.children(&mut c).collect();
    let has_params = kids.iter().any(|k| k.kind() == "params");
    let ps: Vec<_> = kids.iter().filter(|k| k.kind() == "parameter").collect();
    let required = ps
        .iter()
        .filter(|p| {
            let mut c = p.walk();
            !p.children(&mut c).any(|x| x.kind() == "=")
        })
        .count();
    argc >= required && (argc <= ps.len() || has_params)
}

/// The expression a method returns: its `=> ..` body or its only `return ..;`.
fn returned_expr(m: Node) -> Option<Node> {
    let body = m.child_by_field_name("body")?;
    match body.kind() {
        "arrow_expression_clause" => body.named_child(0),
        "block" => {
            let mut returns = vec![];
            collect_returns(body, &mut returns);
            match returns.as_slice() {
                [r] => r.named_child(0),
                _ => None,
            }
        }
        // A lambda's expression body.
        _ => Some(body),
    }
}

/// The first `f` gives for a statement of a block or of the top-level statements that enclose
/// `from`, innermost scope first.
fn in_scope<'t, T>(from: Node<'t>, mut f: impl FnMut(Node<'t>) -> Option<T>) -> Option<T> {
    let mut n = from;
    while let Some(p) = n.parent() {
        if matches!(p.kind(), "block" | "compilation_unit") {
            let mut c = p.walk();
            for st in p.named_children(&mut c).collect::<Vec<_>>() {
                let st = if st.kind() == "global_statement" { st.named_child(0).unwrap_or(st) } else { st };
                if let Some(found) = f(st) {
                    return Some(found);
                }
            }
        }
        n = p;
    }
    None
}

/// The initializer of local `name` declared in a scope enclosing `from` (`var root = new ..;`).
pub(crate) fn scope_local<'t>(from: Node<'t>, name: &str, text: &str) -> Option<Node<'t>> {
    in_scope(from, |st| {
        if st.kind() != "local_declaration_statement" {
            return None;
        }
        let decl = child_of_kind(st, "variable_declaration")?;
        let mut c = decl.walk();
        let d = decl
            .named_children(&mut c)
            .find(|d| d.kind() == "variable_declarator" && d.child_by_field_name("name").is_some_and(|n| &text[n.byte_range()] == name))?;
        let v = d.named_child(d.named_child_count() as u32 - 1)?;
        (Some(v) != d.child_by_field_name("name")).then_some(v)
    })
}

/// A local lambda `name` in scope of `from`: `var make = (string t) => new SkiaLabel { .. };`.
fn local_lambda<'t>(from: Node<'t>, name: &str, text: &str) -> Option<Node<'t>> {
    let mut v = scope_local(from, name, text)?;
    while let "parenthesized_expression" | "cast_expression" = v.kind() {
        v = v.child_by_field_name("value").or_else(|| v.named_child(v.named_child_count() as u32 - 1))?;
    }
    (v.kind() == "lambda_expression").then_some(v)
}

/// The value of the last top-level `return` of a file of top-level statements (a DrawFiddle
/// snippet), not one inside a local function or lambda.
pub(crate) fn top_level_return(root: Node) -> Option<Node> {
    let mut c = root.walk();
    root.named_children(&mut c)
        .filter(|g| g.kind() == "global_statement")
        .filter_map(|g| g.named_child(0))
        .filter(|st| st.kind() == "return_statement")
        .last()
        .and_then(|r| r.named_child(0))
}

fn collect_returns<'t>(n: Node<'t>, out: &mut Vec<Node<'t>>) {
    match n.kind() {
        "return_statement" => out.push(n),
        "lambda_expression" | "local_function_statement" | "anonymous_method_expression" => {}
        _ => {
            let mut c = n.walk();
            for k in n.children(&mut c) {
                collect_returns(k, out);
            }
        }
    }
}

// ---------------------------------------------------------------- editing

type Edits = Vec<(usize, usize, String)>;

fn plan(text: &str, tree: &SourceTree, handles: &[Node], i: usize, edit: &SourceEdit) -> Result<Edits, SourceEditRefusal> {
    use SourceEditRefusal::*;
    let node = &tree.nodes[i];
    let h = handles[i];
    let child = |role: SourceRole| (i + 1..tree.nodes.len()).find(|&j| tree.nodes[j].parent == Some(i) && tree.nodes[j].role == role);
    match edit {
        SourceEdit::SetValue { value } => {
            let lit = match node.role {
                SourceRole::Literal => h,
                SourceRole::Attribute => handles[child(SourceRole::Literal).ok_or(NotEditable)?],
                _ => return Err(NotEditable),
            };
            Ok(vec![(lit.start_byte(), lit.end_byte(), respell(lit, text, value)?)])
        }
        SourceEdit::AddProperty { name, value } => Ok(vec![add_assignment(text, tree, handles, i, name, &spell(value)?)?]),
        SourceEdit::Remove => {
            let (container, item) = match node.role {
                // `Children.Add(...)` calls of a constructor.
                SourceRole::Attribute if h.kind() == "expression_statement" => return Err(NotEditable),
                SourceRole::Attribute if h.parent().is_some_and(|p| p.kind() == "expression_statement") => {
                    return Ok(vec![remove_statement(text, h.parent().unwrap())]);
                }
                SourceRole::Attribute => (h.parent().ok_or(NotEditable)?, h),
                SourceRole::Element => collection_slot(chain_top(h)).ok_or(NotEditable)?,
                _ => return Err(NotEditable),
            };
            Ok(vec![remove_item(container, item)])
        }
        SourceEdit::Move { to } => {
            if node.role != SourceRole::Element {
                return Err(NotEditable);
            }
            let (container, item) = collection_slot(chain_top(h)).ok_or(NotEditable)?;
            let its = items(container);
            if *to >= its.len() {
                return Err(InvalidValue);
            }
            let from = its.iter().position(|&x| x == item).ok_or(NotEditable)?;
            let mut order: Vec<usize> = (0..its.len()).collect();
            let moved = order.remove(from);
            order.insert(*to, moved);
            Ok(its
                .iter()
                .zip(&order)
                .filter(|&(slot, &src)| slot.id() != its[src].id())
                .map(|(slot, &src)| (slot.start_byte(), slot.end_byte(), text[its[src].byte_range()].to_string()))
                .collect())
        }
        // ponytail: needs a target collection and the control's constructor shape; add with the toolbox drop.
        SourceEdit::InsertElement { .. } => Err(NotEditable),
    }
}

/// `name = value` added to element `i`: into its initializer, or as a statement of its constructor.
fn add_assignment(text: &str, tree: &SourceTree, handles: &[Node], i: usize, name: &str, value: &str) -> Result<(usize, usize, String), SourceEditRefusal> {
    if tree.nodes[i].role != SourceRole::Element {
        return Err(SourceEditRefusal::NotEditable);
    }
    let taken = tree.nodes.iter().any(|n| n.parent == Some(i) && n.name.as_deref() == Some(name));
    if !is_ident(name) || taken {
        return Err(SourceEditRefusal::InvalidValue);
    }
    let assignment = format!("{name} = {value}");
    let h = handles[i];
    if h.kind() == "constructor_declaration" {
        let attrs: Vec<Node> = (i + 1..tree.nodes.len())
            .filter(|&j| tree.nodes[j].parent == Some(i) && tree.nodes[j].role == SourceRole::Attribute)
            .map(|j| handles[j])
            .collect();
        return Ok(add_statement(text, &attrs, &assignment));
    }
    Ok(add_property(text, h, &assignment))
}

fn comma_after(n: Node) -> Option<Node> {
    let mut s = n.next_sibling();
    while let Some(x) = s {
        if x.kind() != "comment" {
            return (x.kind() == ",").then_some(x);
        }
        s = x.next_sibling();
    }
    None
}

/// Deletes one item of an initializer or collection with its separator.
fn remove_item(container: Node, item: Node) -> (usize, usize, String) {
    let its = items(container);
    let k = its.iter().position(|&x| x == item).expect("item of its container");
    if k + 1 < its.len() {
        return (item.start_byte(), its[k + 1].start_byte(), String::new());
    }
    if k > 0 {
        let prev = its[k - 1];
        return match (comma_after(item), comma_after(prev)) {
            (Some(own), Some(pc)) => (pc.end_byte(), own.end_byte(), String::new()),
            _ => (prev.end_byte(), item.end_byte(), String::new()),
        };
    }
    let open = container.child(0).unwrap();
    let close = container.child(container.child_count() - 1).unwrap();
    let fill = if open.kind() == "{" { " " } else { "" };
    (open.end_byte(), close.start_byte(), fill.into())
}

fn line_indent<'a>(text: &'a str, lines: &Lines, byte: usize) -> &'a str {
    let start = lines.starts[lines.line_of(byte)];
    let rest = &text[start..];
    &rest[..rest.len() - rest.trim_start_matches([' ', '\t']).len()]
}

fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// A new assignment statement after the constructor's last one, or before its first `Add` call.
fn add_statement(text: &str, attrs: &[Node], assignment: &str) -> (usize, usize, String) {
    let lines = Lines::new(text);
    let nl = newline(text);
    let last = attrs.iter().filter(|a| a.kind() == "assignment_expression").filter_map(|a| a.parent()).max_by_key(|s| s.start_byte());
    match last {
        Some(st) => (st.end_byte(), st.end_byte(), format!("{nl}{}{assignment};", line_indent(text, &lines, st.start_byte()))),
        None => {
            let at = attrs.iter().map(|a| a.start_byte()).min().expect("a this element has attributes");
            (at, at, format!("{assignment};{nl}{}", line_indent(text, &lines, at)))
        }
    }
}

/// Deletes a statement, and its line when nothing else is on it.
fn remove_statement(text: &str, st: Node) -> (usize, usize, String) {
    let before = &text[..st.start_byte()];
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let after = &text[st.end_byte()..];
    let line_end = after.find('\n').map(|i| st.end_byte() + i + 1);
    match line_end {
        Some(e) if before[line_start..].trim().is_empty() && text[st.end_byte()..e].trim().is_empty() => (line_start, e, String::new()),
        _ => (st.start_byte(), st.end_byte(), String::new()),
    }
}

fn add_property(text: &str, creation: Node, assignment: &str) -> (usize, usize, String) {
    let Some(init) = initializer(creation) else {
        return (creation.end_byte(), creation.end_byte(), format!(" {{ {assignment} }}"));
    };
    let its = items(init);
    let Some(&last) = its.last() else {
        return (init.start_byte(), init.end_byte(), format!("{{ {assignment} }}"));
    };
    let trailing = comma_after(last);
    let at = trailing.map_or(last.end_byte(), |c| c.end_byte());
    let lines = Lines::new(text);
    if lines.line_of(init.start_byte()) == lines.line_of(last.start_byte()) {
        let s = if trailing.is_some() { format!(" {assignment},") } else { format!(", {assignment}") };
        return (at, at, s);
    }
    let indent = line_indent(text, &lines, last.start_byte());
    let nl = newline(text);
    let s = if trailing.is_some() { format!("{nl}{indent}{assignment},") } else { format!(",{nl}{indent}{assignment}") };
    (at, at, s)
}

fn string_literal(s: &str, quote: char) -> String {
    let mut out = String::from(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if c == quote => {
                out.push('\\');
                out.push(c)
            }
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

fn number(v: f64) -> Result<String, SourceEditRefusal> {
    if v.is_finite() { Ok(format!("{v}")) } else { Err(SourceEditRefusal::InvalidValue) }
}

fn is_ident(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// A value for a new property.
fn spell(v: &SourceEditValue) -> Result<String, SourceEditRefusal> {
    Ok(match v {
        SourceEditValue::String { value } => string_literal(value, '"'),
        SourceEditValue::Number { value } => number(*value)?,
        SourceEditValue::Boolean { value } => value.to_string(),
        // A new property has no prefix to keep: `Fill`, or `LayoutOptions.Fill` when given.
        SourceEditValue::Case { name } if name.split('.').all(is_ident) => name.clone(),
        SourceEditValue::Case { .. } => return Err(SourceEditRefusal::InvalidValue),
    })
}

/// A value replacing the literal `old`, in the old literal's kind and spelling.
fn respell(old: Node, text: &str, v: &SourceEditValue) -> Result<String, SourceEditRefusal> {
    use SourceEditRefusal::InvalidValue;
    let kind = old.kind();
    match v {
        SourceEditValue::String { value } if kind.contains("string_literal") => Ok(string_literal(value, '"')),
        SourceEditValue::String { value } if kind == "character_literal" && value.chars().count() == 1 => {
            Ok(string_literal(value, '\''))
        }
        SourceEditValue::Boolean { value } if kind == "boolean_literal" => Ok(value.to_string()),
        // The old prefix stays (`LayoutType.Row` -> `LayoutType.Column`); a qualified name gives its case.
        SourceEditValue::Case { name } if kind == "member_access_expression" && name.split('.').all(is_ident) => {
            let prefix = &text[old.child_by_field_name("expression").unwrap().byte_range()];
            Ok(format!("{prefix}.{}", name.rsplit('.').next().unwrap()))
        }
        SourceEditValue::Number { value } if is_number(old) || kind == "prefix_unary_expression" => {
            let lit = if is_number(old) { old } else { old.named_child(0).unwrap() };
            let s = &text[lit.byte_range()];
            let hex = s.len() > 1 && matches!(&s[..2], "0x" | "0X" | "0b" | "0B");
            let suffix_chars: &[char] = if hex { &['u', 'U', 'l', 'L'] } else { &['f', 'F', 'd', 'D', 'm', 'M', 'u', 'U', 'l', 'L'] };
            let suffix = &s[s.trim_end_matches(suffix_chars).len()..];
            let integral_only = suffix.chars().any(|c| "uUlL".contains(c));
            if integral_only && (value.fract() != 0.0 || *value < 0.0) {
                return Err(InvalidValue);
            }
            Ok(format!("{}{suffix}", number(*value)?))
        }
        _ => Err(InvalidValue),
    }
}

#[cfg(test)]
mod tests;
