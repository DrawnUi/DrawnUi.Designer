//! Ties the controls of a running app ([`designer_protocol::Tree`]) to the source keys that create
//! them, by walking runtime children against what the code puts in each control.

use std::collections::HashMap;

use designer_protocol::{NodeId, Tree as RuntimeTree};
use tree_sitter::{Node, Tree};

use super::*;

/// One runtime control and the source node that creates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeMatch {
    pub node: NodeId,
    pub uri: String,
    /// An element key, or `Class/this` for an instance of a class built in its constructor.
    pub key: String,
    /// The helper calls passed through, outermost first (keys of call Reference nodes).
    pub via: Vec<String>,
}

/// Matches a runtime tree against C# files.
///
/// Anchors are runtime instances of classes whose constructor builds them (`Class/this`). From an
/// anchor, the runtime children are walked in order against the source children (`Children`,
/// `Content`, element-valued properties, `Add` / `AddSubView` calls), through helper calls,
/// parameters, locals, `params` arrays, `Concat`, `ToList`, `Select(Helper)` (every produced
/// item maps to the helper's element) and `x == null ? .. : ..` when `x` is a literal argument.
/// Types compare by short name. Runtime nodes the code does not create are skipped; a source value
/// that cannot be followed (a loop, other LINQ, a field) ends matching for the rest of that list.
pub fn match_runtime(files: &[(String, String)], tree: &RuntimeTree) -> Vec<RuntimeMatch> {
    // C# files by the C# reader, TSX (React) files by the TSX one, Rust by the Rust one; a control
    // any of them can place.
    let (rs, files): (Vec<_>, Vec<_>) = files.iter().cloned().partition(|(u, _)| crate::rs::is_rust(u));
    let (tsx, cs): (Vec<_>, Vec<_>) = files.into_iter().partition(|(u, _)| crate::tsx::is_tsx(u));
    let mut out = if cs.is_empty() { vec![] } else { run(&cs, tree).0 };
    if !tsx.is_empty() || !rs.is_empty() {
        let mut done: std::collections::HashSet<NodeId> = out.iter().map(|m| m.node).collect();
        if !tsx.is_empty() {
            out.extend(crate::tsx::match_tsx(&tsx, tree, &done));
            done = out.iter().map(|m| m.node).collect();
        }
        if !rs.is_empty() {
            out.extend(crate::rs::match_rust(&rs, tree, &done));
        }
        let order: HashMap<NodeId, usize> = tree.nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
        out.sort_by_key(|r| order.get(&r.node).copied().unwrap_or(usize::MAX));
    }
    out
}

/// The matches and the source values that stopped a list (as written, shortened).
pub(crate) fn run(files: &[(String, String)], tree: &RuntimeTree) -> (Vec<RuntimeMatch>, Vec<String>) {
    let parsed: Vec<Tree> = files.iter().map(|(_, text)| parse(text)).collect();
    let methods: Vec<Vec<Node>> = parsed
        .iter()
        .map(|ts| {
            let mut v = vec![];
            collect_kind(ts.root_node(), "method_declaration", &mut v);
            v
        })
        .collect();
    let mut m = Matcher {
        files: files
            .iter()
            .zip(&parsed)
            .zip(&methods)
            .map(|(((uri, text), ts), methods)| FileIndex::new(uri, text, ts, methods))
            .collect(),
        rt: tree,
        kids: HashMap::new(),
        matched: HashMap::new(),
        stops: vec![],
        classes: HashMap::new(),
    };
    // Library-made parts (a button's label, shell chrome) are not in any code: never candidates.
    for (i, n) in tree.nodes.iter().enumerate() {
        if let Some(p) = n.parent.filter(|_| !n.internal) {
            m.kids.entry(p).or_default().push(i);
        }
    }
    for (f, file) in m.files.iter().enumerate() {
        for (i, n) in file.source.nodes.iter().enumerate() {
            if let Some(class) = n.key.strip_suffix("/this") {
                let short = class.rsplit('.').next().unwrap_or(class).to_string();
                m.classes.entry(short).or_default().push((f, i));
            }
        }
    }
    for i in 0..tree.nodes.len() {
        let n = &tree.nodes[i];
        if m.matched.contains_key(&n.id) {
            continue;
        }
        if n.internal {
            continue;
        }
        let Some((f, s)) = m.class_of(&n.type_name) else { continue };
        let ctor = m.files[f].handles[s];
        m.record(i, f, s, vec![]);
        let items = m.this_items(f, ctor);
        m.match_children(i, items);
    }
    // A control held by a field (`.Assign(out _field)`) the walk did not reach: anchor it there.
    for i in 0..tree.nodes.len() {
        let n = &tree.nodes[i];
        if n.internal || m.matched.contains_key(&n.id) {
            continue;
        }
        let Some((f, node)) = n.field.as_deref().and_then(|fl| m.field_site(fl)) else { continue };
        m.assign(i, Made { file: f, node, ctx: None, via: vec![], template: false });
    }
    m.climb();
    // A root the code puts in a host view (`new Canvas { Content = .. }`): anchor it there.
    for i in 0..tree.nodes.len() {
        let n = &tree.nodes[i];
        if n.internal || n.parent.is_some() || m.matched.contains_key(&n.id) {
            continue;
        }
        if let Some(made) = m.canvas_content(i) {
            m.assign(i, made);
        }
    }
    // A DrawFiddle snippet (top-level statements): the Fiddle puts the value of its last top-level
    // `return` into its own root layout, so that layout's children are what the return makes.
    for f in 0..m.files.len() {
        let Some(ret) = top_level_return(m.files[f].root) else { continue };
        let items = m.eval(f, ret, None, &[]);
        for i in 0..tree.nodes.len() {
            if tree.nodes[i].parent.is_none() && !tree.nodes[i].internal {
                m.match_children(i, items.clone());
            }
        }
    }
    let mut out: Vec<RuntimeMatch> = m.matched.into_values().collect();
    let order: HashMap<NodeId, usize> = tree.nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
    out.sort_by_key(|r| order[&r.node]);
    (out, m.stops)
}

struct FileIndex<'t> {
    uri: String,
    root: Node<'t>,
    text: &'t str,
    source: SourceTree,
    handles: Vec<Node<'t>>,
    /// Element creation node id -> source node index.
    elements: HashMap<usize, usize>,
    /// Call node id -> key of its Reference node.
    calls: HashMap<usize, String>,
    namespace: String,
    /// `new X { }.Assign(out _field)`: creation node id -> (class, field), and the reverse.
    assigned: HashMap<usize, (String, String)>,
    assign_sites: HashMap<(String, String), Vec<Node<'t>>>,
    resolver: Resolver<'t, 't>,
}

impl<'t> FileIndex<'t> {
    fn new(uri: &str, text: &'t str, ts: &'t Tree, methods: &'t [Node<'t>]) -> Self {
        let (source, handles) = walk(ts, text, uri);
        let mut elements = HashMap::new();
        let mut calls = HashMap::new();
        for (i, n) in source.nodes.iter().enumerate() {
            match n.role {
                SourceRole::Element => {
                    elements.insert(handles[i].id(), i);
                }
                SourceRole::Reference if n.name.is_some() => {
                    if let Some(c) = call_of(expr_of(handles[i])) {
                        calls.insert(c.id(), n.key.clone());
                    }
                }
                _ => {}
            }
        }
        let mut ns = vec![];
        collect_kind(ts.root_node(), "file_scoped_namespace_declaration", &mut ns);
        collect_kind(ts.root_node(), "namespace_declaration", &mut ns);
        let namespace = ns
            .first()
            .and_then(|n| n.child_by_field_name("name"))
            .map(|n| text[n.byte_range()].to_string())
            .unwrap_or_default();
        let mut assigned = HashMap::new();
        let mut assign_sites: HashMap<(String, String), Vec<Node<'t>>> = HashMap::new();
        let mut invs = vec![];
        collect_kind(ts.root_node(), "invocation_expression", &mut invs);
        for inv in invs {
            let Some(func) = inv.child_by_field_name("function").filter(|f| f.kind() == "member_access_expression") else { continue };
            let args = call_args(inv);
            let is_assign = func.child_by_field_name("name").is_some_and(|n| &text[n.byte_range()] == "Assign");
            let (true, [arg]) = (is_assign, args.as_slice()) else { continue };
            let base = chain_base(inv);
            let field = expr_of(*arg);
            if !is_element(base) || field.kind() != "identifier" || !text[arg.byte_range()].starts_with("out") {
                continue;
            }
            let mut p = inv.parent();
            while let Some(x) = p.filter(|x| !is_type_decl(*x)) {
                p = x.parent();
            }
            let Some(class) = p.map(|t| type_name(t, text)) else { continue };
            let key = (class, text[field.byte_range()].to_string());
            assigned.insert(base.id(), key.clone());
            assign_sites.entry(key).or_default().push(base);
        }
        Self {
            uri: uri.into(),
            root: ts.root_node(),
            text,
            source,
            handles,
            elements,
            calls,
            namespace,
            assigned,
            assign_sites,
            resolver: Resolver { text, methods, frames: vec![] },
        }
    }
}

/// What a source value puts in a control: elements in order, or a point where following stops.
#[derive(Clone)]
enum Item<'t> {
    One(Made<'t>),
    /// Every item a `Select` produces: matches a run of runtime siblings, of this length when the
    /// source says it (`new[] { .. }`, `Enumerable.Range(0, 24)`).
    Many(Made<'t>, Option<usize>),
    /// A value that cannot be followed, in file `.0`.
    Stop(usize, Node<'t>),
}

#[derive(Clone)]
struct Made<'t> {
    file: usize,
    node: Node<'t>,
    ctx: Option<usize>,
    via: Vec<String>,
    /// Made by an `ItemTemplate`: matches recycled cells (`template_index`) only.
    template: bool,
}

struct Matcher<'t, 'r> {
    files: Vec<FileIndex<'t>>,
    rt: &'r RuntimeTree,
    kids: HashMap<NodeId, Vec<usize>>,
    matched: HashMap<NodeId, RuntimeMatch>,
    stops: Vec<String>,
    /// Short class name -> (file, index of its `this` element).
    classes: HashMap<String, Vec<(usize, usize)>>,
}

/// How many items `recv.Select(..)` makes, when the source says it.
fn select_count(mut recv: Node, text: &str) -> Option<usize> {
    while recv.kind() == "parenthesized_expression" {
        recv = recv.named_child(0)?;
    }
    if let Some((_, its)) = value_items(recv) {
        return Some(its.len());
    }
    // `Enumerable.Range(start, count)` with a literal count.
    let f = recv.child_by_field_name("function")?;
    if recv.kind() == "invocation_expression" && text[f.byte_range()] == *"Enumerable.Range" {
        let args = call_args(recv);
        let n = expr_of(*args.get(1)?);
        return (n.kind() == "integer_literal").then(|| text[n.byte_range()].parse().ok()).flatten();
    }
    None
}

/// The short name of the class a node is declared in.
fn enclosing_class(mut n: Node, text: &str) -> String {
    while let Some(p) = n.parent() {
        if is_type_decl(p) {
            return type_name(p, text);
        }
        n = p;
    }
    String::new()
}

/// `DrawnUi.Draw.SkiaButton+ButtonLabel` -> (`ButtonLabel`, `DrawnUi.Draw`); generics dropped.
fn split_type(t: &str) -> (&str, &str) {
    let t = t.split(['`', '<']).next().unwrap_or(t);
    let (ns, rest) = t.rsplit_once('.').unwrap_or(("", t));
    (rest.rsplit('+').next().unwrap_or(rest), ns)
}

impl<'t> Matcher<'t, '_> {
    fn class_of(&self, runtime_type: &str) -> Option<(usize, usize)> {
        let (short, ns) = split_type(runtime_type);
        let all = self.classes.get(short)?;
        let same_ns: Vec<_> = all.iter().filter(|(f, _)| self.files[*f].namespace == ns).collect();
        match (all.as_slice(), same_ns.as_slice()) {
            ([one], _) => Some(*one),
            (_, [one]) => Some(**one),
            _ => None,
        }
    }

    /// The one `.Assign(out _field)` site of a runtime `Namespace.Class._field`.
    fn field_site(&self, field: &str) -> Option<(usize, Node<'t>)> {
        let (class_path, name) = field.rsplit_once('.')?;
        let (class, ns) = split_type(class_path);
        let sites: Vec<(usize, Node<'t>)> = self
            .files
            .iter()
            .enumerate()
            .filter(|(_, fi)| ns.is_empty() || fi.namespace.is_empty() || fi.namespace == ns)
            .flat_map(|(f, fi)| fi.assign_sites.get(&(class.to_string(), name.to_string())).into_iter().flatten().map(move |n| (f, *n)))
            .collect();
        match sites.as_slice() {
            [one] => Some(*one),
            _ => None,
        }
    }

    /// Whether runtime node `r` can be what `made` creates, by name, template-ness and field.
    fn fits(&self, r: usize, made: &Made<'t>, name: &str) -> bool {
        let n = &self.rt.nodes[r];
        if self.matched.contains_key(&n.id) || split_type(&n.type_name).0 != name || n.template_index.is_some() != made.template {
            return false;
        }
        // Both name a field: they must agree.
        match (n.field.as_deref(), self.files[made.file].assigned.get(&made.node.id())) {
            (Some(rf), Some((class, field))) => rf.rsplit_once('.').is_some_and(|(cp, fl)| fl == field && split_type(cp).0 == class),
            _ => true,
        }
    }

    /// Whether runtime node `r` is matched to the element `made` creates.
    fn is_match_of(&self, r: usize, made: &Made<'t>) -> bool {
        let fi = &self.files[made.file];
        let Some(&s) = fi.elements.get(&made.node.id()) else { return false };
        self.matched.get(&self.rt.nodes[r].id).is_some_and(|m| m.uri == fi.uri && m.key == fi.source.nodes[s].key)
    }

    /// From matched controls up: while the source element sits in a parent element's
    /// `Children` / `Content` (or another property) and the runtime parent has that type, the
    /// parent is that element, and its other children follow from the walk.
    fn climb(&mut self) {
        loop {
            let mut found = None;
            for n in &self.rt.nodes {
                let (Some(m), Some(pid)) = (self.matched.get(&n.id), n.parent) else { continue };
                if self.matched.contains_key(&pid) {
                    continue;
                }
                let Some(p) = self.rt.nodes.iter().position(|x| x.id == pid).filter(|&p| !self.rt.nodes[p].internal) else { continue };
                let Some(f) = self.files.iter().position(|fi| fi.uri == m.uri) else { continue };
                let fi = &self.files[f];
                let Some(s) = fi.source.nodes.iter().position(|x| x.key == m.key) else { continue };
                // element <- attribute <- element: structural parents only, not helper call arguments.
                let parent = fi.source.nodes[s]
                    .parent
                    .filter(|&a| fi.source.nodes[a].role == SourceRole::Attribute)
                    .and_then(|a| fi.source.nodes[a].parent)
                    .filter(|&e| fi.source.nodes[e].role == SourceRole::Element && fi.handles[e].kind() != "constructor_declaration");
                let Some(e) = parent else { continue };
                let made = Made { file: f, node: fi.handles[e], ctx: None, via: m.via.clone(), template: false };
                if self.short_name(&made).is_some_and(|name| self.fits(p, &made, &name)) {
                    found = Some((p, made));
                    break;
                }
            }
            match found {
                Some((p, made)) => self.assign(p, made),
                None => break,
            }
        }
    }

    /// The element a root control is, when exactly one host view in the code takes an element of
    /// its type as `Content` (`new Canvas { Content = CreateMainLayout() }`). Narrowed to the class
    /// named by the root's `host` hint, else to classes holding a field reported under the root.
    fn canvas_content(&mut self, root: usize) -> Option<Made<'t>> {
        const HOSTS: [&str; 2] = ["Canvas", "DrawnView"];
        let rn = &self.rt.nodes[root];
        let short = split_type(&rn.type_name).0.to_string();
        let host = rn.host.clone();
        let mut candidates: Vec<(String, Made<'t>)> = vec![];
        for f in 0..self.files.len() {
            let fi = &self.files[f];
            let hosts: Vec<Node<'t>> = (0..fi.source.nodes.len())
                .filter(|&i| fi.source.nodes[i].role == SourceRole::Element && fi.source.nodes[i].name.as_deref().is_some_and(|n| HOSTS.contains(&split_type(n).0)))
                .map(|i| fi.handles[i])
                .collect();
            for h in hosts {
                let class = enclosing_class(h, self.files[f].text);
                let Some(init) = initializer(h) else { continue };
                for a in items(init).into_iter().filter(|a| a.kind() == "assignment_expression") {
                    let text = self.files[f].text;
                    if a.child_by_field_name("left").and_then(|l| prop_name(l, text)) != Some("Content") {
                        continue;
                    }
                    let Some(value) = a.child_by_field_name("right") else { continue };
                    for item in self.eval(f, value, None, &[]) {
                        if let Item::One(made) = item
                            && self.short_name(&made).as_deref() == Some(short.as_str())
                        {
                            candidates.push((class.clone(), made));
                        }
                    }
                }
            }
        }
        if let Some(host) = &host {
            let (class, _) = split_type(host);
            candidates.retain(|(c, _)| c == class);
        } else if candidates.len() > 1 {
            // Fields reported under the root name the class that built it.
            let mut fields = vec![];
            let mut stack = vec![rn.id];
            while let Some(id) = stack.pop() {
                for n in self.rt.nodes.iter().filter(|n| n.parent == Some(id)) {
                    if let Some(f) = n.field.as_deref().and_then(|f| f.rsplit_once('.')) {
                        fields.push(split_type(f.0).0.to_string());
                    }
                    stack.push(n.id);
                }
            }
            candidates.retain(|(c, _)| fields.contains(c));
        }
        match candidates.len() {
            1 => candidates.pop().map(|(_, m)| m),
            _ => None,
        }
    }

    fn record(&mut self, r: usize, f: usize, s: usize, via: Vec<String>) {
        let id = self.rt.nodes[r].id;
        self.matched.insert(id, RuntimeMatch { node: id, uri: self.files[f].uri.clone(), key: self.files[f].source.nodes[s].key.clone(), via });
    }

    fn short_name(&self, made: &Made<'t>) -> Option<String> {
        let s = *self.files[made.file].elements.get(&made.node.id())?;
        let name = self.files[made.file].source.nodes[s].name.as_deref()?;
        Some(split_type(name).0.to_string())
    }

    fn match_children(&mut self, r: usize, items: Vec<Item<'t>>) {
        let kids = self.kids.get(&self.rt.nodes[r].id).cloned().unwrap_or_default();
        let mut ptr = 0;
        for (idx, item) in items.iter().cloned().enumerate() {
            match item {
                Item::Stop(f, n) => {
                    let text = self.files[f].text[n.byte_range()].split_whitespace().collect::<Vec<_>>().join(" ");
                    self.stops.push(text.chars().take(60).collect());
                    break;
                }
                Item::One(made) => {
                    let Some(name) = self.short_name(&made) else { continue };
                    // Matched already (an anchor below this control): keep the alignment past it.
                    if let Some(j) = (ptr..kids.len()).find(|&j| self.is_match_of(kids[j], &made)) {
                        ptr = j + 1;
                        continue;
                    }
                    if let Some(j) = (ptr..kids.len()).find(|&j| self.fits(kids[j], &made, &name)) {
                        self.assign(kids[j], made);
                        ptr = j + 1;
                    }
                }
                Item::Many(made, count) => {
                    let Some(name) = self.short_name(&made) else { continue };
                    let Some(start) = (ptr..kids.len()).find(|&j| self.fits(kids[j], &made, &name)) else { continue };
                    let run = (start..kids.len()).take_while(|&j| self.fits(kids[j], &made, &name)).count();
                    // Without a count, leave one sibling for each later item of the same type.
                    let later = items[idx + 1..]
                        .iter()
                        .take_while(|i| !matches!(i, Item::Stop(..)))
                        .filter(|i| matches!(i, Item::One(m) if self.short_name(m).as_deref() == Some(&name)))
                        .count();
                    let take = count.unwrap_or(run.saturating_sub(later)).min(run);
                    for &k in &kids[start..start + take] {
                        self.assign(k, made.clone());
                    }
                    ptr = start + take;
                }
            }
        }
    }

    fn assign(&mut self, r: usize, made: Made<'t>) {
        let f = made.file;
        let s = self.files[f].elements[&made.node.id()];
        self.record(r, f, s, made.via.clone());
        let mut items = vec![];
        if let Some(name) = self.short_name(&made)
            && let Some(&(cf, cs)) = self.classes.get(&name).and_then(|v| (v.len() == 1).then(|| &v[0]))
        {
            let ctor = self.files[cf].handles[cs];
            items = self.this_items(cf, ctor);
        }
        items.extend(self.element_items(&made));
        self.match_children(r, items);
    }

    /// What a constructor puts in `this`.
    fn this_items(&mut self, f: usize, ctor: Node<'t>) -> Vec<Item<'t>> {
        let text = self.files[f].text;
        let mut out = vec![];
        let Some(body) = ctor.child_by_field_name("body").filter(|b| b.kind() == "block") else { return out };
        for s in items(body) {
            if let Some(a) = this_assignment(s, text) {
                out.extend(self.property_items(f, a, None, &[]));
            } else if let Some(inv) = this_add(s, text) {
                for arg in call_args(inv) {
                    out.extend(self.eval(f, expr_of(arg), None, &[]));
                }
            }
        }
        out
    }

    /// What an element's initializer puts in it.
    fn element_items(&mut self, made: &Made<'t>) -> Vec<Item<'t>> {
        let Some(init) = initializer(made.node) else { return vec![] };
        let mut out = vec![];
        for a in items(init).into_iter().filter(|a| a.kind() == "assignment_expression") {
            out.extend(self.property_items(made.file, a, made.ctx, &made.via));
        }
        out
    }

    /// `Children` and `Content` are followed fully; another property only when its value is an
    /// element.
    fn property_items(&mut self, f: usize, assign: Node<'t>, ctx: Option<usize>, via: &[String]) -> Vec<Item<'t>> {
        let text = self.files[f].text;
        let (Some(left), Some(value)) = (assign.child_by_field_name("left"), assign.child_by_field_name("right")) else { return vec![] };
        match prop_name(left, text) {
            Some("Children" | "Content") => self.eval(f, value, ctx, via),
            // `ItemTemplate = new DataTemplate(() => new Cell())`: every recycled cell.
            Some("ItemTemplate") if is_creation(value) => {
                let Some(&lambda) = call_args(value).first() else { return vec![] };
                self.eval_fn(f, expr_of(lambda), ctx, via)
                    .into_iter()
                    .map(|i| match i {
                        Item::One(m) => Item::Many(Made { template: true, ..m }, None),
                        other => other,
                    })
                    .collect()
            }
            _ if is_element(chain_base(value)) => vec![Item::One(Made { file: f, node: chain_base(value), ctx, via: via.to_vec(), template: false })],
            _ => vec![],
        }
    }

    /// The elements an expression evaluates to, in order.
    fn eval(&mut self, f: usize, mut e: Node<'t>, ctx: Option<usize>, via: &[String]) -> Vec<Item<'t>> {
        let text = self.files[f].text;
        while let "cast_expression" | "parenthesized_expression" = e.kind() {
            match e.child_by_field_name("value").or_else(|| e.named_child(e.named_child_count() as u32 - 1)) {
                Some(x) => e = x,
                None => return vec![Item::Stop(f, e)],
            }
        }
        if e.kind() == "null_literal" {
            return vec![];
        }
        // LINQ shapes that keep the items known.
        if e.kind() == "invocation_expression"
            && let Some(func) = e.child_by_field_name("function").filter(|x| x.kind() == "member_access_expression")
            && let (Some(recv), Some(name)) = (func.child_by_field_name("expression"), func.child_by_field_name("name"))
        {
            let args = call_args(e);
            let name = if name.kind() == "generic_name" { name.named_child(0).unwrap_or(name) } else { name };
            match (&text[name.byte_range()], args.len()) {
                ("ToList" | "ToArray" | "AsEnumerable", 0) => return self.eval(f, recv, ctx, via),
                ("Prepend", 1) => {
                    let mut out = self.eval(f, expr_of(args[0]), ctx, via);
                    if !matches!(out.last(), Some(Item::Stop(..))) {
                        out.extend(self.eval(f, recv, ctx, via));
                    }
                    return out;
                }
                ("Concat" | "Append", 1) => {
                    let mut out = self.eval(f, recv, ctx, via);
                    if !matches!(out.last(), Some(Item::Stop(..))) {
                        out.extend(self.eval(f, expr_of(args[0]), ctx, via));
                    }
                    return out;
                }
                ("Select", 1) => {
                    let count = select_count(recv, text);
                    return self
                        .eval_fn(f, expr_of(args[0]), ctx, via)
                        .into_iter()
                        .map(|i| match i {
                            Item::One(m) => Item::Many(m, count),
                            other => other,
                        })
                        .collect();
                }
                ("Empty", 0) if matches!(&text[recv.byte_range()], "Enumerable" | "Array") => return vec![],
                _ => {}
            }
        }
        let base = chain_base(e);
        if is_element(base) {
            return vec![Item::One(Made { file: f, node: base, ctx, via: via.to_vec(), template: false })];
        }
        if let Some((_, its)) = value_items(base) {
            let mut out = vec![];
            for item in its {
                let spread = child_of_kind(item, "spread_element").and_then(|s| s.named_child(0));
                out.extend(self.eval(f, spread.unwrap_or_else(|| expr_of(item)), ctx, via));
                if matches!(out.last(), Some(Item::Stop(..))) {
                    break;
                }
            }
            return out;
        }
        if e.kind() == "conditional_expression" {
            return match self.decide(f, e.child_by_field_name("condition"), ctx) {
                Some(true) => self.eval(f, e.child_by_field_name("consequence").unwrap_or(e), ctx, via),
                Some(false) => self.eval(f, e.child_by_field_name("alternative").unwrap_or(e), ctx, via),
                None => vec![Item::Stop(f, e)],
            };
        }
        if base.kind() == "identifier"
            && let Some(k) = ctx
        {
            let name = &text[base.byte_range()];
            let args = self.bound_all(f, k, name);
            if !args.is_empty() {
                let parent = self.files[f].resolver.frames[k].parent;
                let mut out = vec![];
                for a in args {
                    out.extend(self.eval(f, expr_of(a), parent, via));
                }
                return out;
            }
            if let Some(init) = local_init(self.files[f].resolver.frames[k].method, name, text) {
                return self.eval(f, init, ctx, via);
            }
        }
        // A local of the top-level statements (`var footer = new SkiaLabel { .. };`).
        if base.kind() == "identifier"
            && ctx.is_none()
            && let Some(init) = scope_local(base, &text[base.byte_range()], text)
        {
            return self.eval(f, init, None, via);
        }
        if let Some(call) = call_of(e)
            && let Some(k) = self.files[f].resolver.enter_call(call, ctx)
            && let Some(ret) = returned_expr(self.files[f].resolver.frames[k].method)
        {
            let mut via = via.to_vec();
            via.extend(self.files[f].calls.get(&call.id()).cloned());
            return self.eval(f, ret, Some(k), &via);
        }
        vec![Item::Stop(f, e)]
    }

    /// The items one call of a `Select` function produces: a method group or a lambda.
    fn eval_fn(&mut self, f: usize, func: Node<'t>, ctx: Option<usize>, via: &[String]) -> Vec<Item<'t>> {
        let r = &self.files[f].resolver;
        if matches!(func.kind(), "identifier" | "member_access_expression") {
            let method = [1, 2].into_iter().find_map(|argc| find_method(func, func, argc, r.methods, r.text));
            if let Some(method) = method
                && let Some(k) = self.files[f].resolver.enter(method, vec![], ctx)
                && let Some(ret) = returned_expr(method)
            {
                return self.eval(f, ret, Some(k), via);
            }
        }
        if func.kind() == "lambda_expression"
            && let Some(body) = func.child_by_field_name("body")
        {
            let ret = if body.kind() == "block" {
                let mut rs = vec![];
                collect_returns(body, &mut rs);
                match rs.as_slice() {
                    [r] => r.named_child(0),
                    _ => None,
                }
            } else {
                Some(body)
            };
            if let Some(ret) = ret {
                return self.eval(f, ret, ctx, via);
            }
        }
        vec![Item::Stop(f, func)]
    }

    /// Every argument parameter `name` of frame `k` receives (all trailing ones for `params`).
    fn bound_all(&self, f: usize, k: usize, name: &str) -> Vec<Node<'t>> {
        let r = &self.files[f].resolver;
        let fr = &r.frames[k];
        if let Some(p) = fr.params_at.filter(|&p| fr.params[p] == name) {
            return fr.args.iter().skip(p).filter(|a| a.child_by_field_name("name").is_none()).copied().collect();
        }
        r.bound(k, name).into_iter().collect()
    }

    /// `x == null` / `x != null` where `x` is a parameter given a literal, `null` or an element.
    fn decide(&self, f: usize, cond: Option<Node<'t>>, ctx: Option<usize>) -> Option<bool> {
        let text = self.files[f].text;
        let c = cond.filter(|c| c.kind() == "binary_expression")?;
        let (l, r) = (c.child_by_field_name("left")?, c.child_by_field_name("right")?);
        let op = &text[c.child_by_field_name("operator")?.byte_range()];
        let id = if r.kind() == "null_literal" { l } else if l.kind() == "null_literal" { r } else { return None };
        if id.kind() != "identifier" {
            return None;
        }
        let arg = expr_of(self.files[f].resolver.bound(ctx?, &text[id.byte_range()])?);
        let is_null = match arg.kind() {
            "null_literal" => true,
            _ if is_literal(arg, text) || is_creation(chain_base(arg)) => false,
            _ => return None,
        };
        match op {
            "==" => Some(is_null),
            "!=" => Some(!is_null),
            _ => None,
        }
    }
}
