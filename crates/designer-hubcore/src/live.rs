//! The live changes of one app that are not in its code yet: controls inserted live and not written,
//! controls taken out live and still in the code, controls moved live and still at their old place
//! in the code. With them the hub turns the app's tree into the
//! tree its code makes (the "code view"), which every write to code is matched against.
//!
//! Updated from what goes to the app (Insert, Remove, Move, PlaceMode, SetProperty, Copy), what the
//! app answers (Inserted, Moved, a new Tree, its Controls, Copied) and from writes to code and their undos.
//! When the app builds its controls again from its code (a hot reload), the changes not written are
//! put back onto the rebuilt controls ([`Live::rebuilt`]): nothing changed live is lost unless its
//! place is gone from the code.

use std::collections::{HashMap, HashSet};

use designer_protocol::{AppMsg, ControlKind, ControlSpec, HubToApp, Node, NodeId, Refusal, SpecProp, Tree};

/// A control taken out live and still in the code.
#[derive(Clone, Debug, PartialEq)]
pub struct Removal {
    /// The control and what it holds, as the app had them (parents first).
    pub nodes: Vec<Node>,
    pub parent: NodeId,
    /// Its parent's children before it, then: it goes back after the last of them still there.
    before: Vec<NodeId>,
}

impl Removal {
    pub fn node(&self) -> NodeId {
        self.nodes[0].id
    }
}

/// A control moved live and not written: where the code has it, its parent and the siblings it had
/// before it there (it goes back after the last of them still there).
#[derive(Clone, Debug, PartialEq)]
pub struct Movement {
    pub node: NodeId,
    pub parent: NodeId,
    before: Vec<NodeId>,
}

/// A live change a write to code makes true in the code (an undo makes it live-only again).
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// The control inserted live, as it was written.
    Insert(NodeId, ControlSpec),
    Remove(Removal),
    Move(Movement),
    /// A property set live, now written.
    Prop(NodeId, String),
}

/// A copy the app read ([`HubToApp::Copy`]): the spec and where it came from, so its code keeps only
/// what the source's code sets (and what was changed live), not the values the library sets itself.
#[derive(Clone, Debug)]
pub struct CopySource {
    pub spec: ControlSpec,
    pub source: NodeId,
    /// The code view of the app's tree when it was read.
    pub tree: Tree,
    /// Properties changed live in the source's controls then, by control.
    pub live_set: HashMap<NodeId, HashSet<String>>,
}

/// An insert on its way to the app.
#[derive(Debug)]
enum Pending {
    /// A new control as Write puts it in code: its type's defaults, the properties sent, its children.
    New(ControlSpec),
    /// A control taken out live, put back (the code still has it).
    Restore(NodeId),
    /// An insert not written, put back after a rebuild: its old id and what it is.
    Replay(NodeId, ControlSpec),
    /// A control the code makes, moved: where the code has it.
    Move(Movement),
}

/// A move put back after a rebuild into a control inserted live and put back too: sent once the app
/// tells that insert's new id.
#[derive(Debug)]
struct WaitingMove {
    /// The insert's id before the rebuild.
    insert: NodeId,
    request: u32,
    node: NodeId,
    index: u32,
}

#[derive(Default, Debug)]
pub struct Live {
    /// Controls inserted live and not in the code, as Write puts them in code (property types
    /// left empty where the app's properties give them).
    pub inserts: HashMap<NodeId, ControlSpec>,
    /// Controls taken out live and still in the code, in the order they were taken out.
    pub removes: Vec<Removal>,
    /// Controls the code makes moved live and not written, in the order they were first moved.
    pub moves: Vec<Movement>,
    requests: HashMap<u32, Pending>,
    /// The type place mode places: its click answers request 0.
    placing: Option<String>,
    /// The controls the app can create, for their defaults.
    controls: Vec<ControlKind>,
    /// Copies asked of the app (request, control) and the last ones it read.
    copy_requests: HashMap<u32, NodeId>,
    pub copies: Vec<CopySource>,
    /// Properties set live and not written (from an editor or the MCP server), by control: their
    /// last values, in the order first set.
    live_set: HashMap<NodeId, Vec<(String, String)>>,
    /// Moves put back after a rebuild that wait for the insert they go into.
    waiting: Vec<WaitingMove>,
}

/// How many read copies are kept.
const COPIES: usize = 16;

/// The control `id` and every control under it in `tree` (parents come before their children).
fn subtree(tree: &Tree, id: NodeId) -> HashSet<NodeId> {
    let mut set: HashSet<NodeId> = HashSet::new();
    for n in &tree.nodes {
        if n.id == id || n.parent.is_some_and(|p| set.contains(&p)) {
            set.insert(n.id);
        }
    }
    set
}

impl Live {
    /// A new control of `type_name` as Write puts it: its type's defaults then `props` over them, or
    /// for a paste (props or children given) those alone, as the app builds it; then `children`.
    fn spec(&self, type_name: &str, props: &[(String, String)], children: &[ControlSpec]) -> ControlSpec {
        let paste = !props.is_empty() || !children.is_empty();
        let mut out: Vec<(String, String)> = if paste { vec![] } else { self.controls.iter().find(|k| k.type_name == type_name).map(|k| k.defaults.clone()).unwrap_or_default() };
        for (name, value) in props {
            match out.iter_mut().find(|(n, _)| n == name) {
                Some(slot) => slot.1 = value.clone(),
                None => out.push((name.clone(), value.clone())),
            }
        }
        let props = out.into_iter().map(|(name, value)| SpecProp { name, value, type_name: String::new() }).collect();
        ControlSpec { type_name: type_name.to_owned(), props, children: children.to_vec() }
    }

    /// Something went to the app (from an editor or the MCP server); `tree` is the app's last.
    pub fn sent(&mut self, msg: &HubToApp, tree: Option<&Tree>) {
        match msg {
            HubToApp::Insert { request, type_name, props, children, .. } => {
                // A restore already registered under this request keeps its meaning.
                if !self.requests.contains_key(request) {
                    let spec = self.spec(type_name, props, children);
                    self.requests.insert(*request, Pending::New(spec));
                }
            }
            HubToApp::PlaceMode { type_name } => self.placing = type_name.clone(),
            // A drop from Controls onto the canvas: answered as a place-mode click.
            HubToApp::PlaceAt { type_name, .. } => self.placing = Some(type_name.clone()),
            HubToApp::Copy { request, node } => {
                self.copy_requests.insert(*request, *node);
            }
            HubToApp::SetProperty { node, name, value, .. } => {
                let edits = self.live_set.entry(*node).or_default();
                match edits.iter_mut().find(|(n, _)| n == name) {
                    Some(e) => e.1 = value.clone(),
                    None => edits.push((name.clone(), value.clone())),
                }
            }
            HubToApp::Move { request, node, .. } => {
                // Only a control the code makes, at its place in the code (moved again: the first
                // place is the code's).
                let Some(tree) = tree else { return };
                if !self.in_code(tree, *node) || self.moves.iter().any(|m| m.node == *node) {
                    return;
                }
                let Some(parent) = tree.nodes.iter().find(|n| n.id == *node).and_then(|n| n.parent) else { return };
                let before = tree.nodes.iter().filter(|m| m.parent == Some(parent)).take_while(|m| m.id != *node).map(|m| m.id).collect();
                self.requests.insert(*request, Pending::Move(Movement { node: *node, parent, before }));
            }
            HubToApp::Remove { node, .. } => {
                let Some(tree) = tree else { return };
                let Some(n) = tree.nodes.iter().find(|n| n.id == *node) else { return };
                let in_code = self.in_code(tree, *node);
                // Inserts inside it go with it; they were never in the code.
                let inside = subtree(tree, *node);
                let inserted: HashSet<NodeId> = self.inserts.keys().copied().filter(|i| inside.contains(i)).flat_map(|i| subtree(tree, i)).collect();
                self.inserts.retain(|i, _| !inside.contains(i));
                // Inside a pasted copy not written: it goes from what that copy is written with.
                if !in_code && !self.inserts.contains_key(node)
                    && let Some(parent) = n.parent
                    && let Some((top, path)) = self.insert_above(tree, parent)
                    && let Some(i) = tree.nodes.iter().filter(|m| m.parent == Some(parent) && !m.internal).position(|m| m.id == *node)
                    && let Some(spec) = self.inserts.get_mut(&top).and_then(|s| spec_at(s, &path))
                    && i < spec.children.len()
                {
                    spec.children.remove(i);
                }
                // Itself inserted (or inside an insert): not in the code, nothing to remember.
                if !in_code {
                    return;
                }
                let Some(parent) = n.parent else { return };
                let nodes = tree.nodes.iter().filter(|m| inside.contains(&m.id) && !inserted.contains(&m.id)).cloned().collect();
                let before = tree.nodes.iter().filter(|m| m.parent == Some(parent)).take_while(|m| m.id != *node).map(|m| m.id).collect();
                self.removes.push(Removal { nodes, parent, before });
            }
            _ => {}
        }
    }

    /// An editor puts back control `node`, taken out live, with insert `request`.
    pub fn restoring(&mut self, request: u32, node: NodeId) {
        self.requests.insert(request, Pending::Restore(node));
    }

    /// Where a taken-out control goes back in the app now: its parent and the index among the
    /// parent's children that are the app's own (no library parts).
    pub fn restore_place(&self, tree: &Tree, node: NodeId) -> Option<(NodeId, String, u32)> {
        let r = self.removes.iter().find(|r| r.node() == node)?;
        let kids: Vec<&Node> = tree.nodes.iter().filter(|n| n.parent == Some(r.parent) && !n.internal).collect();
        let after = r.before.iter().rev().find_map(|b| kids.iter().position(|k| k.id == *b));
        Some((r.parent, r.nodes[0].type_name.clone(), after.map_or(0, |i| i as u32 + 1)))
    }

    /// Something the app said; `tree` is its last tree. An insert put back after a rebuild answers
    /// with its old and new id, and the moves into it that waited for it.
    pub fn heard(&mut self, msg: &AppMsg, tree: Option<&Tree>) -> Option<Replay> {
        match msg {
            AppMsg::Copied { request, spec } => {
                let (Some(source), Some(spec), Some(tree)) = (self.copy_requests.remove(request), spec, tree) else { return None };
                let inside = subtree(tree, source);
                let live_set = self.live_set.iter().filter(|(id, _)| inside.contains(id)).map(|(id, e)| (*id, e.iter().map(|(n, _)| n.clone()).collect())).collect();
                self.copies.push(CopySource { spec: spec.clone(), source, tree: self.code_view(tree, None), live_set });
                if self.copies.len() > COPIES {
                    self.copies.remove(0);
                }
            }
            AppMsg::Controls { controls } => self.controls = controls.clone(),
            AppMsg::Moved { request, node, .. } => {
                if let (Some(Pending::Move(m)), Some(_)) = (self.requests.remove(request), node) {
                    self.moves.push(m);
                }
            }
            AppMsg::Inserted { request, node, .. } => {
                let pending = self.requests.remove(request);
                let Some(id) = node else { return None };
                match pending {
                    Some(Pending::Move(_)) => {}
                    Some(Pending::Restore(old)) => self.removes.retain(|r| r.node() != old),
                    Some(Pending::Replay(old, spec)) => {
                        self.inserts.insert(*id, spec);
                        let (now, later) = std::mem::take(&mut self.waiting).into_iter().partition(|w| w.insert == old);
                        self.waiting = later;
                        let sends = now.into_iter().map(|w: WaitingMove| HubToApp::Move { request: w.request, node: w.node, parent: *id, index: w.index }).collect();
                        return Some(Replay { sends, moved: vec![(old, *id)], ..Default::default() });
                    }
                    Some(Pending::New(d)) => {
                        self.inserts.insert(*id, d);
                    }
                    None => {
                        // A click in place mode.
                        let d = self.placing.take().map(|t| self.spec(&t, &[], &[])).unwrap_or_default();
                        self.inserts.insert(*id, d);
                    }
                }
            }
            // A new tree: see `rebuilt`, called by the hub with the tree before it.
            _ => {}
        }
        None
    }

    /// A write to code made `change` true in the code.
    pub fn wrote(&mut self, change: &Change) {
        match change {
            Change::Insert(id, _) => {
                self.inserts.remove(id);
            }
            Change::Remove(r) => self.removes.retain(|x| x.node() != r.node()),
            Change::Move(m) => self.moves.retain(|x| x.node != m.node),
            Change::Prop(node, name) => {
                if let Some(edits) = self.live_set.get_mut(node) {
                    edits.retain(|(n, _)| n != name);
                }
            }
        }
    }

    /// That write was taken back: the change is live-only again, when its control (or its parent)
    /// is still in the app.
    pub fn undone(&mut self, change: &Change, tree: Option<&Tree>) {
        let has = |id: NodeId| tree.is_some_and(|t| t.nodes.iter().any(|n| n.id == id));
        match change {
            Change::Insert(id, d) if has(*id) => {
                self.inserts.insert(*id, d.clone());
            }
            Change::Remove(r) if has(r.parent) && !has(r.node()) => self.removes.push(r.clone()),
            Change::Move(m) if has(m.node) && has(m.parent) => self.moves.push(m.clone()),
            _ => {}
        }
    }

    /// Whether the code makes control `id`: neither it nor a parent of it was inserted live.
    pub fn in_code(&self, tree: &Tree, id: NodeId) -> bool {
        let mut at = Some(id);
        while let Some(i) = at {
            if self.inserts.contains_key(&i) {
                return false;
            }
            at = tree.nodes.iter().find(|n| n.id == i).and_then(|n| n.parent);
        }
        true
    }

    /// The tree the code makes: `tree` without the controls inserted live (except `keep`, the one
    /// being written now), with the ones taken out live back where they were.
    pub fn code_view(&self, tree: &Tree, keep: Option<NodeId>) -> Tree {
        let mut view = tree.clone();
        // Moved controls back where the code has them, the last moved first (`keep`: where it is
        // now, the one being written). Before the inserts go: one moved into an insert stays.
        for m in self.moves.iter().rev().filter(|m| Some(m.node) != keep) {
            let inside = subtree(&view, m.node);
            let nodes: Vec<Node> = view.nodes.iter().filter(|n| inside.contains(&n.id)).cloned().collect();
            let Some(mut first) = nodes.first().cloned() else { continue };
            if !view.nodes.iter().any(|n| n.id == m.parent) {
                continue;
            }
            view.nodes.retain(|n| !inside.contains(&n.id));
            first.parent = Some(m.parent);
            let at = Self::place_after(&view, m.parent, &m.before);
            for (k, n) in std::iter::once(first).chain(nodes.into_iter().skip(1)).enumerate() {
                view.nodes.insert(at + k, n);
            }
        }
        let mut gone: HashSet<NodeId> = self.inserts.keys().copied().filter(|i| Some(*i) != keep).collect();
        view.nodes.retain(|n| {
            let out = gone.contains(&n.id) || n.parent.is_some_and(|p| gone.contains(&p));
            if out {
                gone.insert(n.id);
            }
            !out
        });
        // The last one taken out first: each goes after the last sibling it had that is there.
        for r in self.removes.iter().rev() {
            let Some(start) = view.nodes.iter().position(|n| n.id == r.parent) else { continue };
            let anchor = r.before.iter().rev().find(|b| view.nodes.iter().any(|n| n.id == **b && n.parent == Some(r.parent)));
            // After the anchor and what it holds, or first under the parent.
            let at = match anchor {
                Some(a) => {
                    let inside = subtree(&view, *a);
                    let from = view.nodes.iter().position(|n| n.id == *a).unwrap_or(start);
                    from + view.nodes[from..].iter().take_while(|n| inside.contains(&n.id)).count()
                }
                None => start + 1,
            };
            for (k, n) in r.nodes.iter().enumerate() {
                view.nodes.insert(at + k, n.clone());
            }
        }
        view
    }

    /// Where a control goes back under `parent` in `view`: after the last of `before` still there and
    /// what it holds, else first.
    fn place_after(view: &Tree, parent: NodeId, before: &[NodeId]) -> usize {
        let start = view.nodes.iter().position(|n| n.id == parent).unwrap_or(0);
        match before.iter().rev().find(|b| view.nodes.iter().any(|n| n.id == **b && n.parent == Some(parent))) {
            Some(a) => {
                let inside = subtree(view, *a);
                let from = view.nodes.iter().position(|n| n.id == *a).unwrap_or(start);
                from + view.nodes[from..].iter().take_while(|n| inside.contains(&n.id)).count()
            }
            None => start + 1,
        }
    }

    /// Where control `node` sits among its parent's children that are not library parts.
    pub fn index_in(view: &Tree, parent: NodeId, node: NodeId) -> Option<u32> {
        view.nodes.iter().filter(|n| n.parent == Some(parent) && !n.internal).position(|n| n.id == node).map(|i| i as u32)
    }
}

/// `tree` without control `id` and what it holds.
pub fn without(tree: &Tree, id: NodeId) -> Tree {
    let inside = subtree(tree, id);
    let mut out = tree.clone();
    out.nodes.retain(|n| !inside.contains(&n.id));
    out
}

/// Whether `spec` is the copy `copy` pasted: the same type, values and children (the values' types
/// may be left out).
pub fn same_copy(copy: &ControlSpec, spec: &ControlSpec) -> bool {
    copy.type_name == spec.type_name
        && copy.children == spec.children
        && copy.props.len() == spec.props.len()
        && copy.props.iter().zip(&spec.props).all(|(a, b)| a.name == b.name && a.value == b.value && (b.type_name.is_empty() || a.type_name == b.type_name))
}

/// The copy `c` as its code is written: each control keeps the properties its source's code sets
/// (`names` of the source control, None when no code makes it: then all of them) and the ones
/// changed live there; the values the library sets for itself go. With it, each spec node's source
/// control by its path of child indexes.
pub fn code_spec(c: &CopySource, names: &mut dyn FnMut(&Tree, NodeId) -> Option<Vec<String>>) -> (ControlSpec, HashMap<Vec<usize>, NodeId>) {
    fn walk(spec: &ControlSpec, id: Option<NodeId>, path: &mut Vec<usize>, c: &CopySource, names: &mut dyn FnMut(&Tree, NodeId) -> Option<Vec<String>>, sources: &mut HashMap<Vec<usize>, NodeId>) -> ControlSpec {
        if let Some(id) = id {
            sources.insert(path.clone(), id);
        }
        let set = id.and_then(|id| names(&c.tree, id));
        let live = id.and_then(|id| c.live_set.get(&id));
        let props = spec.props.iter().filter(|p| set.as_ref().is_none_or(|n| n.contains(&p.name)) || live.is_some_and(|l| l.contains(&p.name))).cloned().collect();
        // The spec's children are the source's own children in order (no library parts).
        let kids: Vec<NodeId> = id.map(|id| c.tree.nodes.iter().filter(|n| n.parent == Some(id) && !n.internal).map(|n| n.id).collect()).unwrap_or_default();
        let mut children = vec![];
        for (i, child) in spec.children.iter().enumerate() {
            path.push(i);
            children.push(walk(child, kids.get(i).copied().filter(|_| kids.len() == spec.children.len()), path, c, names, sources));
            path.pop();
        }
        ControlSpec { type_name: spec.type_name.clone(), props, children }
    }
    let mut sources = HashMap::new();
    let spec = walk(&c.spec, Some(c.source), &mut vec![], c, names, &mut sources);
    (spec, sources)
}

/// What putting the live changes back after a rebuild takes: the messages for the app, the controls
/// they are now (old id, new id), the inserts on their way back (old ids; their new id comes with
/// their [`AppMsg::Inserted`]), and the changes whose place is gone from the code.
#[derive(Default, Debug, PartialEq)]
pub struct Replay {
    pub sends: Vec<HubToApp>,
    pub moved: Vec<(NodeId, NodeId)>,
    pub replaying: Vec<NodeId>,
    pub lost: Vec<Refusal>,
}

impl Replay {
    pub fn is_empty(&self) -> bool {
        self.sends.is_empty() && self.moved.is_empty() && self.replaying.is_empty() && self.lost.is_empty()
    }
}

/// Where control `id` is in `tree`: its index among its parent's children at every level from the
/// root, and the types along that way.
fn place_of(tree: &Tree, id: NodeId) -> Option<(Vec<usize>, Vec<String>)> {
    let (mut path, mut types) = (vec![], vec![]);
    let mut at = Some(id);
    while let Some(i) = at {
        let n = tree.nodes.iter().find(|n| n.id == i)?;
        path.push(tree.nodes.iter().filter(|m| m.parent == n.parent).position(|m| m.id == i)?);
        types.push(n.type_name.clone());
        at = n.parent;
    }
    path.reverse();
    types.reverse();
    Some((path, types))
}

/// The control at `path` in `tree` when every control on the way has the type `types` says.
fn find_place(tree: &Tree, path: &[usize], types: &[String]) -> Option<NodeId> {
    let mut at: Option<NodeId> = None;
    for (i, t) in path.iter().zip(types) {
        let n = tree.nodes.iter().filter(|m| m.parent == at).nth(*i)?;
        if n.type_name != *t {
            return None;
        }
        at = Some(n.id);
    }
    at
}

/// The spec node at `path` (child indexes) under `spec`.
fn spec_at<'a>(spec: &'a mut ControlSpec, path: &[usize]) -> Option<&'a mut ControlSpec> {
    match path.split_first() {
        None => Some(spec),
        Some((i, rest)) => spec_at(spec.children.get_mut(*i)?, rest),
    }
}

fn short(type_name: &str) -> &str {
    type_name.rsplit(['.', '+']).next().unwrap_or(type_name)
}

impl Live {
    /// The nearest control inserted live at or above `id` in `tree`, with the path from it to `id`
    /// in child indexes among controls that are not library parts (as a spec's children go).
    fn insert_above(&self, tree: &Tree, id: NodeId) -> Option<(NodeId, Vec<usize>)> {
        let mut path = vec![];
        let mut at = id;
        loop {
            if self.inserts.contains_key(&at) {
                path.reverse();
                return Some((at, path));
            }
            let n = tree.nodes.iter().find(|n| n.id == at)?;
            let parent = n.parent?;
            path.push(tree.nodes.iter().filter(|m| m.parent == Some(parent) && !m.internal).position(|m| m.id == at)?);
            at = parent;
        }
    }

    /// A new tree from the app. When the controls of live changes are gone from it, the app built
    /// them again from its code (a hot reload): each change goes back onto the control at the same
    /// place in the code (path and types alike), and a change whose place is gone is reported lost.
    /// `old` is the app's tree before; `request` gives request ids for the hub's own messages.
    pub fn rebuilt(&mut self, old: Option<&Tree>, new: &Tree, request: &mut dyn FnMut() -> u32) -> Replay {
        let ids: HashSet<NodeId> = new.nodes.iter().map(|n| n.id).collect();
        let held: HashSet<NodeId> = self.removes.iter().flat_map(|r| r.nodes.iter().map(|n| n.id)).collect();
        let props_gone: Vec<NodeId> = self.live_set.keys().copied().filter(|id| !ids.contains(id)).collect();
        let inserts_gone: Vec<NodeId> = self.inserts.keys().copied().filter(|id| !ids.contains(id)).collect();
        let removes_gone: Vec<usize> = (0..self.removes.len()).filter(|&i| !ids.contains(&self.removes[i].parent) && !held.contains(&self.removes[i].parent)).collect();
        let moves_gone = self.moves.iter().any(|m| !ids.contains(&m.node));
        let mut out = Replay::default();
        if props_gone.is_empty() && inserts_gone.is_empty() && removes_gone.is_empty() && !moves_gone {
            return out;
        }
        let Some(old) = old else {
            self.live_set.retain(|id, _| ids.contains(id));
            self.inserts.retain(|id, _| ids.contains(id));
            self.removes.retain(|r| ids.contains(&r.parent) || held.contains(&r.parent));
            self.moves.retain(|m| ids.contains(&m.node));
            return out;
        };
        let view = self.code_view(old, None);
        let order = |id: &NodeId| old.nodes.iter().position(|n| n.id == *id).unwrap_or(usize::MAX);

        // Values set live on an inserted control (or inside a pasted one) go into what it is put back with.
        for id in &props_gone {
            if let Some((top, path)) = self.insert_above(old, *id)
                && let Some(edits) = self.live_set.remove(id)
                && let Some(spec) = self.inserts.get_mut(&top).and_then(|s| spec_at(s, &path))
            {
                for (name, value) in edits {
                    match spec.props.iter_mut().find(|p| p.name == name) {
                        Some(p) => p.value = value,
                        None => spec.props.push(SpecProp { name, value, type_name: String::new() }),
                    }
                }
            }
        }

        // An insert inside another one goes back inside it: deepest first, then in order.
        let mut nested: Vec<(usize, NodeId, usize, NodeId)> = vec![];
        for id in &inserts_gone {
            let Some(parent) = old.nodes.iter().find(|n| n.id == *id).and_then(|n| n.parent) else { continue };
            if self.insert_above(old, parent).is_some() {
                let depth = place_of(old, *id).map_or(0, |(p, _)| p.len());
                let index = old.nodes.iter().filter(|m| m.parent == Some(parent) && !m.internal).position(|m| m.id == *id).unwrap_or(0);
                nested.push((depth, parent, index, *id));
            }
        }
        nested.sort_by(|a, b| b.0.cmp(&a.0).then(order(&a.1).cmp(&order(&b.1))).then(a.2.cmp(&b.2)));
        for (_, parent, index, id) in nested {
            let Some(spec) = self.inserts.remove(&id) else { continue };
            if let Some((top, path)) = self.insert_above(old, parent)
                && let Some(target) = self.inserts.get_mut(&top).and_then(|s| spec_at(s, &path))
            {
                target.children.insert(index.min(target.children.len()), spec);
            }
        }

        // Inserts under a control the code makes: at their place among the children the code makes,
        // after the ones put back before them in the same parent.
        let mut tops: Vec<NodeId> = inserts_gone.iter().copied().filter(|id| self.inserts.contains_key(id)).collect();
        tops.sort_by_key(order);
        let places: Vec<Option<(NodeId, u32)>> = tops
            .iter()
            .map(|&id| {
                let p = old.nodes.iter().find(|n| n.id == id).and_then(|n| n.parent)?;
                let (path, types) = place_of(&view, p)?;
                let target = find_place(new, &path, &types)?;
                let index = Live::index_in(&self.code_view(old, Some(id)), p, id)?;
                Some((target, index))
            })
            .collect();
        let mut sent_to: HashMap<NodeId, u32> = HashMap::new();
        for (id, place) in tops.into_iter().zip(places) {
            let Some(spec) = self.inserts.remove(&id) else { continue };
            match place {
                Some((parent, index)) => {
                    let before = sent_to.entry(parent).or_default();
                    let r = request();
                    let props = spec.props.iter().map(|p| (p.name.clone(), p.value.clone())).collect();
                    out.sends.push(HubToApp::Insert { request: r, parent, index: Some(index + *before), type_name: spec.type_name.clone(), props, children: spec.children.clone() });
                    *before += 1;
                    self.requests.insert(r, Pending::Replay(id, spec));
                    out.replaying.push(id);
                }
                None => out.lost.push(Refusal::new("hub-lost-insert", &[("type", &short(&spec.type_name))])),
            }
        }

        // Moves: the same control, built again at its place in the code, moved again to where it was
        // (into the same parent, built again too, at the same index).
        let mut kept_moves = vec![];
        for m in std::mem::take(&mut self.moves) {
            if ids.contains(&m.node) {
                kept_moves.push(m);
                continue;
            }
            let again = |id: NodeId| place_of(&view, id).and_then(|(path, types)| find_place(new, &path, &types));
            let to = old.nodes.iter().find(|n| n.id == m.node).and_then(|n| n.parent);
            // Its index there counts the siblings before it, so without itself.
            let index = to.and_then(|p| Live::index_in(old, p, m.node));
            // Into an insert put back now: once the app tells its new id.
            let into_replay = to.filter(|p| out.replaying.contains(p));
            match (again(m.node), to.and_then(again).or(into_replay), index) {
                (Some(node), Some(parent), Some(index)) => {
                    let from = new.nodes.iter().find(|n| n.id == node).and_then(|n| n.parent).unwrap_or(parent);
                    let before = new.nodes.iter().filter(|n| n.parent == Some(from)).take_while(|n| n.id != node).map(|n| n.id).collect();
                    out.moved.push((m.node, node));
                    out.moved.push((m.parent, from));
                    match into_replay {
                        Some(insert) => self.waiting.push(WaitingMove { insert, request: request(), node, index }),
                        None => out.sends.push(HubToApp::Move { request: request(), node, parent, index }),
                    }
                    kept_moves.push(Movement { node, parent: from, before });
                }
                _ => {
                    let t = old.nodes.iter().find(|n| n.id == m.node).map_or("", |n| short(&n.type_name)).to_owned();
                    out.lost.push(Refusal::new("hub-lost-move", &[("type", &t)]));
                }
            }
        }
        self.moves = kept_moves;

        // Removals: the same control, built again, taken out again.
        let mut kept = vec![];
        for (i, r) in std::mem::take(&mut self.removes).into_iter().enumerate() {
            if !removes_gone.contains(&i) {
                kept.push(r);
                continue;
            }
            let again = place_of(&view, r.node()).and_then(|(path, types)| find_place(new, &path, &types));
            let Some(node) = again else {
                out.lost.push(Refusal::new("hub-lost-remove", &[("type", &short(&r.nodes[0].type_name))]));
                continue;
            };
            let parent = new.nodes.iter().find(|n| n.id == node).and_then(|n| n.parent).unwrap_or(r.parent);
            let inside = subtree(new, node);
            let nodes = new.nodes.iter().filter(|m| inside.contains(&m.id)).cloned().collect();
            let before = new.nodes.iter().filter(|m| m.parent == Some(parent)).take_while(|m| m.id != node).map(|m| m.id).collect();
            out.moved.push((r.node(), node));
            out.moved.push((r.parent, parent));
            out.sends.push(HubToApp::Remove { request: request(), node });
            kept.push(Removal { nodes, parent, before });
        }
        self.removes = kept;

        // Values set live on controls the code makes: onto the same control, built again.
        for id in props_gone {
            let Some(edits) = self.live_set.remove(&id) else { continue };
            // Inside a control taken out live: nothing to show it on.
            if held.contains(&id) {
                continue;
            }
            let again = place_of(&view, id).and_then(|(path, types)| find_place(new, &path, &types));
            match again {
                Some(node) => {
                    out.moved.push((id, node));
                    for (name, value) in &edits {
                        out.sends.push(HubToApp::SetProperty { request: request(), node, name: name.clone(), value: value.clone() });
                    }
                    self.live_set.insert(node, edits);
                }
                None => {
                    let t = old.nodes.iter().find(|n| n.id == id).map_or("", |n| short(&n.type_name)).to_owned();
                    for (name, _) in edits {
                        out.lost.push(Refusal::new("hub-lost-prop", &[("type", &t), ("prop", &name)]));
                    }
                }
            }
        }
        out
    }
}
