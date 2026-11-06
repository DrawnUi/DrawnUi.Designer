//! The designer's undo history, per app: Ctrl+Z takes back the last changing action whatever it was,
//! Ctrl+Y (Ctrl+Shift+Z) does it again. A write to code is no action of its own: it joins the action
//! it wrote (Write, Write all, Auto before Restart or Run, a page's code.flush, Always), so Ctrl+Z
//! takes the action back in the app and in the code together, and Ctrl+Y does both again. Each step
//! goes back and forth through the same paths as the rows' own Undo and Write (SetProperty,
//! UndoCode, Remove, Restore, Insert), so the hub's record of live changes and its replay after a
//! rebuild stay in step.

use super::*;

/// How a control came: from Controls (a drop or a click in place mode), Paste or Duplicate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum AddVerb {
    Add,
    Paste,
    Duplicate,
}

/// One action of the history.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Step {
    /// Values set live on a control: one committed edit, or Paste style's all at once
    /// (property, before, after).
    Values { node: u64, values: Vec<(String, String, String)>, style: bool },
    /// Values written to code (Write, or Write all as one step `group`): property, value, request.
    WriteValues { node: u64, path: String, group: u32, writes: Vec<(String, String, u32)> },
    /// A control added live, with what brings it back (its parent, its place, what it is); `gone`:
    /// the other way round, an added control not written yet taken out by Delete.
    Add { node: u64, parent: u64, index: u32, type_name: String, spec: Option<Copied>, verb: AddVerb, gone: bool },
    /// An added control written to code.
    WriteAdd { node: u64, path: String, type_name: String, request: u32 },
    /// A control taken out live.
    Delete { node: u64, type_name: String },
    /// A taken-out control taken out of the code too.
    WriteDelete { node: u64, type_name: String, request: u32 },
    /// A control moved live: from (parent, index) to (parent, index), indexes after the move.
    Move { node: u64, type_name: String, from: (u64, u32), to: (u64, u32) },
    /// A moved control moved in the code too.
    WriteMove { node: u64, path: String, type_name: String, request: u32 },
    /// An action and the writes that put it into the code (the action first), taken back and done
    /// again together; or, for writes whose action is not in the history, the writes of one flush.
    Group { group: u32, steps: Vec<Step> },
}

/// One app's history: what Ctrl+Z takes back (last at the end) and what Ctrl+Y does again.
#[derive(Default, Debug)]
pub(crate) struct History {
    pub undo: Vec<Step>,
    pub redo: Vec<Step>,
}

impl Step {
    /// What the step is called in the status line and the menus ("Delete SkiaShape").
    pub(crate) fn label(&self) -> String {
        let t = |key: &str, name: &str, value: &str| i18n::tr_args(key, &[(name, value.to_owned().into())]);
        match self {
            Step::Values { style: true, .. } => i18n::tr("step-paste-style"),
            Step::Values { values, .. } => t("step-edit", "prop", values.first().map_or("", |v| v.0.as_str())),
            Step::WriteValues { writes, .. } if writes.len() > 1 => i18n::tr("step-write-all"),
            Step::WriteValues { writes, .. } => t("step-write", "name", writes.first().map_or("", |w| w.0.as_str())),
            Step::Add { type_name, gone: true, .. } => t("step-delete", "type", short_type(type_name)),
            Step::Add { type_name, verb, .. } => t(
                match verb {
                    AddVerb::Add => "step-add",
                    AddVerb::Paste => "step-paste",
                    AddVerb::Duplicate => "step-duplicate",
                },
                "type",
                short_type(type_name),
            ),
            Step::WriteAdd { type_name, .. } => t("step-write", "name", short_type(type_name)),
            Step::Delete { type_name, .. } => t("step-delete", "type", short_type(type_name)),
            Step::WriteDelete { type_name, .. } => t("step-write-delete", "type", short_type(type_name)),
            Step::Move { type_name, .. } => t("step-move", "type", short_type(type_name)),
            Step::WriteMove { type_name, .. } => t("step-write", "name", short_type(type_name)),
            Step::Group { steps, .. } if steps.len() == 1 || steps.first().is_some_and(|s| !s.is_write()) => steps[0].label(),
            Step::Group { .. } => i18n::tr("step-write-changes"),
        }
    }

    /// The control it acts on.
    pub(crate) fn node(&self) -> u64 {
        match self {
            Step::Values { node, .. }
            | Step::WriteValues { node, .. }
            | Step::Add { node, .. }
            | Step::WriteAdd { node, .. }
            | Step::Delete { node, .. }
            | Step::WriteDelete { node, .. }
            | Step::Move { node, .. }
            | Step::WriteMove { node, .. } => *node,
            Step::Group { steps, .. } => steps.first().map_or(0, Step::node),
        }
    }

    /// A write to code (not an action of its own).
    pub(crate) fn is_write(&self) -> bool {
        matches!(self, Step::WriteValues { .. } | Step::WriteAdd { .. } | Step::WriteDelete { .. } | Step::WriteMove { .. })
    }

    /// Whether `write` puts this action into the code: the same control (and its properties).
    fn written_by(&self, write: &Step) -> bool {
        match (self, write) {
            (Step::Values { node, values, .. }, Step::WriteValues { node: n, writes, .. }) => node == n && writes.iter().all(|w| values.iter().any(|v| v.0 == w.0)),
            (Step::Add { node, gone: false, .. }, Step::WriteAdd { node: n, .. }) | (Step::Delete { node, .. }, Step::WriteDelete { node: n, .. }) | (Step::Move { node, .. }, Step::WriteMove { node: n, .. }) => node == n,
            _ => false,
        }
    }

    fn ids_mut(&mut self) -> Vec<&mut u64> {
        match self {
            Step::Add { node, parent, .. } => vec![node, parent],
            Step::Move { node, from, to, .. } => vec![node, &mut from.0, &mut to.0],
            Step::Values { node, .. } | Step::WriteValues { node, .. } | Step::WriteAdd { node, .. } | Step::Delete { node, .. } | Step::WriteDelete { node, .. } | Step::WriteMove { node, .. } => vec![node],
            Step::Group { steps, .. } => steps.iter_mut().flat_map(Step::ids_mut).collect(),
        }
    }
}

impl App {
    pub(crate) fn history_mut(&mut self) -> Option<&mut History> {
        let app = self.app?;
        Some(self.histories.entry(app).or_default())
    }

    fn history(&self) -> Option<&History> {
        self.app.and_then(|a| self.histories.get(&a))
    }

    /// A new action: it goes on the history (after an edit still open), and what was undone can no
    /// longer be done again.
    pub(crate) fn record(&mut self, step: Step) {
        self.flush_edit();
        self.push_step(step);
    }

    fn push_step(&mut self, step: Step) {
        self.note_change(&step);
        if let Some(h) = self.history_mut() {
            h.undo.push(step);
            h.redo.clear();
        }
    }

    /// The open edit's change so far becomes one step (a color dragged is one step, not one per
    /// move); the edit goes on from there.
    pub(crate) fn flush_edit(&mut self) {
        let Some((node, prop, before)) = self.edit_start.clone() else { return };
        let Some(after) = self.pending.get(&(node, prop.clone())).cloned().filter(|a| *a != before) else { return };
        self.edit_start = Some((node, prop.clone(), after.clone()));
        self.push_step(Step::Values { node, values: vec![(prop, before, after)], style: false });
    }

    /// A write to code joins the newest action it puts into the code: Ctrl+Z then takes them back
    /// together. False when no such action is in the history (the write is recorded on its own then).
    pub(crate) fn attach_write(&mut self, write: Step) -> bool {
        self.flush_edit();
        let Some(h) = self.history_mut() else { return false };
        let Some(entry) = h.undo.iter_mut().rev().find(|s| match s {
            Step::Group { steps, .. } => steps.first().is_some_and(|a| !a.is_write() && a.written_by(&write)),
            s => s.written_by(&write),
        }) else {
            return false;
        };
        match entry {
            Step::Group { steps, .. } => steps.push(write),
            s => {
                let action = std::mem::replace(s, Step::Group { group: 0, steps: vec![] });
                *s = Step::Group { group: 0, steps: vec![action, write] };
            }
        }
        true
    }

    /// Writes to code answered: joined to the action they wrote, else one step, or one more write of
    /// the Write all it belongs to.
    pub(crate) fn record_write(&mut self, request: u32, node: u64, path: String, prop: String, value: String) {
        if self.history_quiet.remove(&request) {
            return;
        }
        if self.attach_write(Step::WriteValues { node, path: path.clone(), group: request, writes: vec![(prop.clone(), value.clone(), request)] }) {
            self.write_groups.remove(&request);
            return;
        }
        if self.record_flushed(request, Step::WriteValues { node, path: path.clone(), group: request, writes: vec![(prop.clone(), value.clone(), request)] }) {
            return;
        }
        let group = self.write_groups.remove(&request).unwrap_or(request);
        if let Some(Step::WriteValues { group: g, writes, .. }) = self.history_mut().and_then(|h| h.undo.last_mut())
            && *g == group
        {
            writes.push((prop, value, request));
            return;
        }
        self.record(Step::WriteValues { node, path, group, writes: vec![(prop, value, request)] });
    }

    /// An added control written by `request` is found by its place `path` from now on.
    pub(crate) fn history_written_at(&mut self, request: u32, path: &str) {
        for h in self.histories.values_mut() {
            for step in h.undo.iter_mut().chain(h.redo.iter_mut()) {
                if let Step::WriteAdd { request: r, path: p, .. } | Step::WriteMove { request: r, path: p, .. } = step
                    && *r == request
                {
                    *p = path.to_owned();
                }
            }
        }
    }

    /// What Ctrl+Z takes back now, and what Ctrl+Y does again, by name.
    pub(crate) fn undo_label(&self) -> Option<String> {
        let open = self.edit_start.as_ref().and_then(|(node, prop, before)| self.pending.get(&(*node, prop.clone())).filter(|a| *a != before).map(|_| i18n::tr_args("step-edit", &[("prop", prop.clone().into())])));
        open.or_else(|| self.history().and_then(|h| h.undo.last()).map(Step::label))
    }

    pub(crate) fn redo_label(&self) -> Option<String> {
        self.history().and_then(|h| h.redo.last()).map(Step::label)
    }

    /// Page mode: tells the page what its Undo and Redo buttons would do now, when that changed
    /// (`{type: "history", canUndo, canRedo, undo, redo}`, the step names as the Edit menu has them).
    pub(crate) fn post_history(&self) {
        if self.to_page.is_none() {
            return;
        }
        let (undo, redo) = (self.undo_label(), self.redo_label());
        let msg = serde_json::json!({
            "type": "history",
            "canUndo": undo.is_some(),
            "canRedo": redo.is_some(),
            "undo": undo.unwrap_or_default(),
            "redo": redo.unwrap_or_default(),
        });
        if self.history_posted.borrow().as_ref() != Some(&msg) {
            *self.history_posted.borrow_mut() = Some(msg.clone());
            self.post_page(msg);
        }
    }

    /// The controls of the history have new ids (a rebuild, a control put back or added again).
    pub(crate) fn history_remap(&mut self, old: u64, new: u64) {
        if self.rewrite_on_tree.remove(&old) {
            self.rewrite_on_tree.insert(new);
        }
        for h in self.histories.values_mut() {
            for step in h.undo.iter_mut().chain(h.redo.iter_mut()) {
                for id in step.ids_mut() {
                    if *id == old {
                        *id = new;
                    }
                }
            }
        }
    }

    /// A row's own Undo took something back out of order: it leaves the history (Ctrl+Z never
    /// takes it back twice); `keep` sees the steps newest first, edits each and says whether it
    /// stays. Nothing undone can be done again after it.
    pub(crate) fn forget(&mut self, mut keep: impl FnMut(&mut Step) -> bool) {
        self.flush_edit();
        if let Some(h) = self.history_mut() {
            h.undo.reverse();
            // An action and its writes are looked at one by one, the last write first.
            h.undo.retain_mut(|s| match s {
                Step::Group { steps, .. } => {
                    steps.reverse();
                    steps.retain_mut(|x| keep(x));
                    steps.reverse();
                    !steps.is_empty()
                }
                s => keep(s),
            });
            h.undo.reverse();
            h.redo.clear();
        }
    }

    /// The app is gone: so is its history.
    pub(crate) fn forget_gone_apps(&mut self) {
        let apps = &self.apps;
        self.histories.retain(|id, _| apps.iter().any(|a| a.id == *id));
    }

    /// Ctrl+Z: the last action back.
    pub(crate) fn undo_step(&mut self, cx: &mut Cx) {
        if self.edit_prop.is_some() {
            self.end_edit(cx);
        }
        self.flush_edit();
        let Some(step) = self.history_mut().and_then(|h| h.undo.pop()) else { return };
        let label = step.label();
        let step = self.step_back(step, cx);
        if let Some(h) = self.history_mut() {
            h.redo.push(step);
        }
        self.status = i18n::tr_args("status-undone", &[("step", label.into())]);
        self.status_error = false;
        self.rebuild_prop_rows();
        cx.items_reset(self.tree_list);
        cx.items_reset(self.props_list);
    }

    /// Ctrl+Y: the last action undone, again.
    pub(crate) fn redo_step(&mut self, cx: &mut Cx) {
        if self.edit_prop.is_some() {
            self.end_edit(cx);
        }
        let Some(step) = self.history_mut().and_then(|h| h.redo.pop()) else { return };
        let label = step.label();
        let step = self.step_again(step, cx);
        if let Some(h) = self.history_mut() {
            h.undo.push(step);
        }
        self.status = i18n::tr_args("status-redone", &[("step", label.into())]);
        self.status_error = false;
        self.rebuild_prop_rows();
        cx.items_reset(self.tree_list);
        cx.items_reset(self.props_list);
    }

    /// Sets `value` on `node`'s `prop` live, as an edit made here: the row's dot shows, unless it is
    /// the value from before every live edit (`clean`, or the one kept for it), then the row is
    /// clean again.
    fn set_live(&mut self, node: u64, prop: &str, value: &str, clean: &str) {
        let request = self.request();
        self.send(HubToApp::SetProperty { request, node, name: prop.to_owned(), value: value.to_owned() });
        self.send(HubToApp::GetProps { node });
        let key = (node, prop.to_owned());
        let first = self.live_original.get(&key).cloned().unwrap_or_else(|| clean.to_owned());
        if value == first {
            self.pending.remove(&key);
            self.live_original.remove(&key);
        } else {
            self.live_original.insert(key.clone(), first);
            self.pending.insert(key, value.to_owned());
        }
        if self.selected_node().is_some_and(|n| n.id == node)
            && let Some(p) = self.props.iter_mut().find(|p| p.name == prop)
        {
            p.value = value.to_owned();
        }
    }

    /// The control at `path` when `node` is gone from the tree (a rebuild gave it a new id).
    fn current(&mut self, node: u64, path: &str) -> u64 {
        if self.tree.nodes.iter().any(|n| n.id == node) {
            return node;
        }
        match self.node_at_path(path) {
            Some(now) => {
                self.history_remap(node, now);
                now
            }
            None => node,
        }
    }

    /// Inserts an added control again (Add done again, an added one's Delete taken back): its new
    /// id follows the history when the app answers.
    fn add_again(&mut self, node: u64, parent: u64, index: u32, type_name: &str, spec: &Option<Copied>) {
        let request = self.request();
        self.insert_requests.insert(request, type_name.to_owned());
        self.tree_requests.insert(request);
        self.redo_adds.insert(request, node);
        let (props, children) = match spec {
            Some(s) => {
                self.paste_props.insert(request, s.clone());
                (s.props.iter().map(|p| (p.name.clone(), p.value.clone())).collect(), s.children.clone())
            }
            None => (vec![], vec![]),
        };
        self.send(HubToApp::Insert { request, parent, index: Some(index), type_name: type_name.to_owned(), props, children });
    }

    /// Takes a code write back and keeps the change live (its dot, Undo and Write come back);
    /// `value`: a property's value, live again.
    fn unwrite(&mut self, request: u32, value: &str) {
        self.keep_live.insert(request, value.to_owned());
        self.send_hub(EditorMsg::UndoCode { request });
    }

    /// Takes back the write of `action` with the change: out of the code, and in the app back to
    /// how it was before the action (an added control goes, a taken-out one comes back, a moved one
    /// returns). A value the action changed from one not in code stays live, with its dot.
    fn unwrite_with(&mut self, write: Step, action: &Step) -> Step {
        match &write {
            Step::WriteValues { node, path, writes, .. } => {
                let node = self.current(*node, path);
                for (prop, _, request) in writes.iter().rev() {
                    let Some(list) = self.written.get_mut(&(path.clone(), prop.clone())) else { continue };
                    let Some(at) = list.iter().position(|(r, _)| r == request) else { continue };
                    let (_, in_code) = list.remove(at);
                    let live = match action {
                        Step::Values { values, .. } => values.iter().find(|v| v.0 == *prop).map(|v| v.1.clone()),
                        _ => None,
                    };
                    if let Some(live) = live.filter(|l| *l != in_code) {
                        self.keep_live.insert(*request, live);
                    }
                    self.undo_requests.insert(*request, (node, prop.clone(), in_code));
                    self.send_hub(EditorMsg::UndoCode { request: *request });
                }
            }
            Step::WriteAdd { node, path, request, .. } => {
                let id = self.current(*node, path);
                if self.inserted.get(path.as_str()).is_some_and(|i| i.written == Some(*request)) {
                    self.undo_inserts.insert(*request, (id, path.clone()));
                    self.send_hub(EditorMsg::UndoCode { request: *request });
                }
            }
            Step::WriteDelete { node, request, .. } => {
                self.undo_removes.insert(*request, *node);
                self.send_hub(EditorMsg::UndoCode { request: *request });
            }
            Step::WriteMove { node, path, request, .. } => {
                let id = self.current(*node, path);
                if self.moved.get(path.as_str()).is_some_and(|m| m.written == Some(*request)) {
                    self.undo_moves.insert(*request, (id, path.clone()));
                    self.send_hub(EditorMsg::UndoCode { request: *request });
                }
            }
            _ => {}
        }
        write
    }

    /// Writes `write` again after its action was done again: values at once; a control added or moved
    /// again once the app has it there (`rewrite_on_tree`).
    fn rewrite(&mut self, mut write: Step, cx: &mut Cx) -> Step {
        match &mut write {
            Step::WriteValues { node, path, writes, .. } => {
                let node = self.current(*node, path);
                for (prop, value, request) in writes.iter_mut() {
                    if let Some(r) = self.write_value(node, prop, value) {
                        self.history_quiet.insert(r);
                        *request = r;
                    }
                }
            }
            Step::WriteDelete { node, request, .. } => {
                if let Some(r) = self.write_removal(*node, cx) {
                    self.history_quiet.insert(r);
                    *request = r;
                }
            }
            Step::WriteAdd { node, .. } | Step::WriteMove { node, .. } => {
                self.rewrite_on_tree.insert(*node);
            }
            _ => {}
        }
        write
    }

    /// A control added or moved again by Ctrl+Y is in the app's tree now: its write goes again.
    pub(crate) fn rewrite_now(&mut self, id: u64, cx: &mut Cx) {
        if !self.rewrite_on_tree.remove(&id) {
            return;
        }
        let r = if self.inserted.contains_key(&format!("#{id}")) { self.write_insert(id, cx) } else { self.write_move(id, cx) };
        let Some(r) = r else { return };
        self.history_quiet.insert(r);
        for h in self.histories.values_mut() {
            for step in h.undo.iter_mut() {
                if let Step::Group { steps, .. } = step {
                    for s in steps {
                        if let Step::WriteAdd { node, request, .. } | Step::WriteMove { node, request, .. } = s
                            && *node == id
                        {
                            *request = r;
                        }
                    }
                }
            }
        }
    }

    /// Takes `step` back; it then goes on the redo stack.
    fn step_back(&mut self, mut step: Step, cx: &mut Cx) -> Step {
        match &mut step {
            Step::Values { node, values, .. } => {
                let node = *node;
                for (prop, before, _) in values.iter().rev() {
                    self.set_live(node, prop, before, before);
                }
            }
            Step::WriteValues { node, path, writes, .. } => {
                let node = self.current(*node, path);
                for (prop, value, request) in writes.iter().rev() {
                    let Some(list) = self.written.get_mut(&(path.clone(), prop.clone())) else { continue };
                    let Some(at) = list.iter().position(|(r, _)| r == request) else { continue };
                    let (_, before) = list.remove(at);
                    self.undo_requests.insert(*request, (node, prop.clone(), before));
                    self.unwrite(*request, value);
                }
            }
            Step::Add { node, parent, index, type_name, spec, gone, .. } => {
                if *gone {
                    self.add_again(*node, *parent, *index, &type_name.clone(), &spec.clone());
                } else {
                    self.undo_insert(*node, cx);
                }
            }
            Step::WriteAdd { node, path, request, .. } => {
                let id = self.current(*node, path);
                if self.inserted.get(path.as_str()).is_some_and(|i| i.written == Some(*request)) {
                    self.undo_inserts.insert(*request, (id, path.clone()));
                    self.unwrite(*request, "");
                }
            }
            Step::Delete { node, .. } => self.restore_live(*node),
            Step::WriteDelete { node, request, .. } => {
                self.undo_removes.insert(*request, *node);
                self.unwrite(*request, "");
            }
            Step::Move { node, from, .. } => {
                let index = self.drop_index(*node, from.0, from.1);
                self.move_node(*node, from.0, index, false);
            }
            Step::WriteMove { node, path, request, .. } => {
                let id = self.current(*node, path);
                if self.moved.get(path.as_str()).is_some_and(|m| m.written == Some(*request)) {
                    self.undo_moves.insert(*request, (id, path.clone()));
                    self.unwrite(*request, "");
                }
            }
            // An action with its writes: out of the code with the change; what the code's undo
            // does not do in the app already (values), back live too.
            Step::Group { steps, .. } if steps.first().is_some_and(|s| !s.is_write()) => {
                let mut all = std::mem::take(steps).into_iter();
                let action = all.next().expect("an action");
                let writes: Vec<Step> = all.collect();
                let back: Vec<Step> = writes.into_iter().rev().map(|w| self.unwrite_with(w, &action)).collect();
                let action = if matches!(action, Step::Values { .. }) || back.is_empty() { self.step_back(action, cx) } else { action };
                *steps = std::iter::once(action).chain(back.into_iter().rev()).collect();
            }
            // The last write first.
            Step::Group { steps, .. } => {
                let back: Vec<Step> = std::mem::take(steps).into_iter().rev().map(|s| self.step_back(s, cx)).collect();
                *steps = back.into_iter().rev().collect();
            }
        }
        step
    }

    /// Does `step` again; it then goes back on the undo stack.
    fn step_again(&mut self, mut step: Step, cx: &mut Cx) -> Step {
        match &mut step {
            Step::Values { node, values, .. } => {
                let node = *node;
                for (prop, before, after) in values.iter() {
                    self.set_live(node, prop, after, before);
                }
            }
            Step::WriteValues { node, path, writes, .. } => {
                let node = self.current(*node, path);
                for (prop, value, request) in writes.iter_mut() {
                    if let Some(r) = self.write_value(node, prop, value) {
                        self.history_quiet.insert(r);
                        *request = r;
                    }
                }
            }
            Step::Add { node, parent, index, type_name, spec, gone, .. } => {
                if *gone {
                    self.undo_insert(*node, cx);
                } else {
                    self.add_again(*node, *parent, *index, &type_name.clone(), &spec.clone());
                }
            }
            Step::WriteAdd { node, path, request, .. } => {
                let id = self.current(*node, path);
                if let Some(r) = self.write_insert(id, cx) {
                    self.history_quiet.insert(r);
                    *request = r;
                }
            }
            Step::Delete { node, .. } => self.delete_node(*node, cx),
            Step::Move { node, to, .. } => {
                let index = self.drop_index(*node, to.0, to.1);
                self.move_node(*node, to.0, index, false);
            }
            Step::WriteMove { node, path, request, .. } => {
                let id = self.current(*node, path);
                if let Some(r) = self.write_move(id, cx) {
                    self.history_quiet.insert(r);
                    *request = r;
                }
            }
            Step::WriteDelete { node, request, .. } => {
                if let Some(r) = self.write_removal(*node, cx) {
                    self.history_quiet.insert(r);
                    *request = r;
                }
            }
            // The action again, then into the code again.
            Step::Group { steps, .. } if steps.first().is_some_and(|s| !s.is_write()) => {
                let mut all = std::mem::take(steps).into_iter();
                let action = self.step_again(all.next().expect("an action"), cx);
                let writes: Vec<Step> = all.collect();
                let again: Vec<Step> = writes.into_iter().map(|w| self.rewrite(w, cx)).collect();
                *steps = std::iter::once(action).chain(again).collect();
            }
            Step::Group { steps, .. } => {
                let again: Vec<Step> = std::mem::take(steps).into_iter().map(|s| self.step_again(s, cx)).collect();
                *steps = again;
            }
        }
        step
    }
}
