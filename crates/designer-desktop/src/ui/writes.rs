//! Writing the live changes to code: the setting (Manual, Auto, Always), how many changes are not in
//! code yet, and a flush that writes them all as one step of the history.

use super::*;
use history::Step;

/// When the live changes are written to code (Settings, Code).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, serde::Serialize, serde::Deserialize)]
pub enum WriteMode {
    /// With a row's Write or Write all only.
    Manual,
    /// Before the code is needed: Restart and Run, and when a page asks (`code.flush`).
    #[default]
    Auto,
    /// Each finished change at once (Enter or leaving a field, a drop, a paste, a move, a delete).
    Always,
}

impl WriteMode {
    /// In the order Settings shows them.
    pub const ALL: [WriteMode; 3] = [WriteMode::Manual, WriteMode::Auto, WriteMode::Always];

    #[cfg_attr(not(target_os = "emscripten"), allow(dead_code))]
    pub fn from_name(name: &str) -> Option<WriteMode> {
        match name.to_ascii_lowercase().as_str() {
            "manual" => Some(WriteMode::Manual),
            "auto" => Some(WriteMode::Auto),
            "always" => Some(WriteMode::Always),
            _ => None,
        }
    }

    pub fn title(self) -> String {
        i18n::tr(match self {
            WriteMode::Manual => "write-mode-manual",
            WriteMode::Auto => "write-mode-auto",
            WriteMode::Always => "write-mode-always",
        })
    }
}

/// One change not in code yet.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Unwritten {
    Insert(u64),
    Move(u64),
    Prop(u64, String),
    Remove(u64),
}

/// How long a page's `code.flush` waits for its writes before it is answered anyway (the page gives up at 3 s).
const PAGE_FLUSH_WAIT_MS: f32 = 2500.0;

/// What waits for a flush to be answered.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum AfterFlush {
    /// A page's `code.flush` (its request): answered with `code.flushed`.
    Page(serde_json::Value),
    Restart,
    Run(String),
    Close,
}

/// A flush on its way: its writes not answered yet, how many took, and what waits for it.
#[derive(Debug)]
pub(crate) struct Flush {
    pub requests: std::collections::HashSet<u32>,
    pub written: usize,
    pub then: Vec<AfterFlush>,
    /// Why the writes refused were refused, in the order they came (each once).
    pub reasons: Vec<String>,
}

impl App {
    /// Every change not in code yet, in the order a flush writes them: added controls (parents
    /// first), moves, values, then the controls taken out.
    pub(crate) fn unwritten(&self) -> Vec<Unwritten> {
        let mut out: Vec<Unwritten> = self.tree.nodes.iter().filter(|n| self.inserted_state(n.id) == Some(false) && !self.is_removed(n.id)).map(|n| Unwritten::Insert(n.id)).collect();
        let mut moves: Vec<u64> = self.moved.iter().filter(|(_, m)| m.written.is_none()).filter_map(|(k, _)| k.strip_prefix('#').and_then(|id| id.parse().ok())).collect();
        moves.sort_unstable();
        out.extend(moves.into_iter().map(Unwritten::Move));
        let mut props: Vec<(u64, String)> = self.pending.keys().filter(|(n, _)| !self.is_removed(*n) && self.tree.nodes.iter().any(|x| x.id == *n)).cloned().collect();
        props.sort();
        out.extend(props.into_iter().map(|(n, p)| Unwritten::Prop(n, p)));
        out.extend(self.removed.iter().filter(|r| r.written.is_none()).map(|r| Unwritten::Remove(r.node.id)));
        out
    }

    pub(crate) fn unwritten_count(&self) -> usize {
        self.unwritten().len()
    }

    /// Writes `changes` to code, all of them one step of the history; `then` runs once every write
    /// is answered. One flush at a time: a second one waits on the first and is answered with it.
    pub(crate) fn flush(&mut self, changes: Vec<Unwritten>, then: Vec<AfterFlush>, cx: &mut Cx) {
        if let Some(f) = &mut self.flushing {
            f.then.extend(then);
            return;
        }
        if self.edit_prop.is_some() {
            self.end_edit(cx);
        }
        let group = self.request();
        let mut requests = std::collections::HashSet::new();
        for change in changes {
            let request = match &change {
                Unwritten::Insert(id) => self.write_insert(*id, cx),
                Unwritten::Move(id) => self.write_move(*id, cx),
                Unwritten::Prop(node, prop) => match self.pending.get(&(*node, prop.clone())).cloned() {
                    Some(value) => self.write_value(*node, prop, &value),
                    None => None,
                },
                Unwritten::Remove(id) => self.write_removal(*id, cx),
            };
            if let Some(r) = request {
                requests.insert(r);
                self.flush_groups.insert(r, group);
            }
        }
        let done = requests.is_empty();
        self.flushing = Some(Flush { requests, written: 0, then, reasons: vec![] });
        if done {
            self.finish_flush(cx);
        }
    }

    /// Write all changes: every change not in code, now.
    pub(crate) fn write_everything(&mut self, cx: &mut Cx) {
        let changes = self.unwritten();
        self.flush(changes, vec![], cx);
    }

    /// A write was answered (written or refused): the flush waiting for it goes on.
    pub(crate) fn flush_answered(&mut self, request: u32, written: bool, cx: &mut Cx) {
        self.flush_groups.remove(&request);
        let Some(f) = &mut self.flushing else { return };
        if !f.requests.remove(&request) {
            return;
        }
        if written {
            f.written += 1;
        }
        if f.requests.is_empty() {
            self.finish_flush(cx);
        }
    }

    fn finish_flush(&mut self, cx: &mut Cx) {
        let Some(f) = self.flushing.take() else { return };
        let left = self.unwritten_count();
        for then in f.then {
            match then {
                AfterFlush::Page(request) => self.post_page(serde_json::json!({ "type": "code.flushed", "request": request, "written": f.written, "left": left })),
                // Changes that could not be written are never dropped without asking: restart,
                // run or close anyway, or stay.
                other if left > 0 && self.confirm.is_none() => self.ask_failed(other, f.reasons.clone(), cx),
                other => self.go_on(other, cx),
            }
        }
    }

    /// After a flush that left changes not written: the question without a Write, with why.
    fn ask_failed(&mut self, then: AfterFlush, reasons: Vec<String>, cx: &mut Cx) {
        self.ask_unwritten(then, cx);
        self.confirm_failed = true;
        self.failed_reasons = reasons;
        if let Some(popup) = self.popup.take() {
            cx.close_popup_window(popup);
        }
        let size = cx.canvas_size();
        let anchor = drawnui::skia::Rect::from_xywh(size.width / 2.0 - 150.0, 44.0, 300.0, 0.0);
        self.popup = Some(cx.open_popup_window(view::unwritten_menu(self), anchor, PopupWindowOptions { cpu: true, ..Default::default() }));
    }

    /// What waited for the changes: done now.
    pub(crate) fn go_on(&mut self, then: AfterFlush, cx: &mut Cx) {
        match then {
            AfterFlush::Page(request) => {
                let left = self.unwritten_count();
                self.post_page(serde_json::json!({ "type": "code.flushed", "request": request, "written": 0, "left": left }));
            }
            AfterFlush::Restart => self.restart_now(cx),
            AfterFlush::Run(project) => self.run_now(project, cx),
            // Ends the designer without asking again.
            AfterFlush::Close => cx.close_app(),
        }
    }

    /// The window's close button, Alt+F4, Cmd+Q, File Exit: true closes now. With changes not in code
    /// the window stays while they are written (Auto, Always) or the question is open (Manual).
    pub(crate) fn closing(&mut self, cx: &mut Cx) -> bool {
        self.confirm.is_none() && !self.changes_first(AfterFlush::Close, cx)
    }

    /// Before Restart, Run or closing: the changes not in code are written first (Auto, Always), or
    /// the designer asks (Manual). True when it waits: `then` runs after.
    pub(crate) fn changes_first(&mut self, then: AfterFlush, cx: &mut Cx) -> bool {
        if self.unwritten_count() == 0 {
            return false;
        }
        match self.settings.write_mode {
            WriteMode::Manual => self.ask_unwritten(then, cx),
            WriteMode::Auto | WriteMode::Always => {
                let changes = self.unwritten();
                self.flush(changes, vec![then], cx);
            }
        }
        true
    }

    /// The question: write the changes first, go on without them, or stay.
    pub(crate) fn ask_unwritten(&mut self, then: AfterFlush, cx: &mut Cx) {
        self.close_popup(cx);
        self.confirm = Some(then);
        self.confirm_failed = false;
        let size = cx.canvas_size();
        let anchor = drawnui::skia::Rect::from_xywh(size.width / 2.0 - 150.0, 44.0, 300.0, 0.0);
        self.popup = Some(cx.open_popup_window(view::unwritten_menu(self), anchor, PopupWindowOptions { cpu: true, ..Default::default() }));
    }

    /// The question's answer: `write` the changes first, or not.
    pub(crate) fn answer_unwritten(&mut self, write: bool, cx: &mut Cx) {
        self.close_popup(cx);
        let Some(then) = self.confirm.take() else { return };
        if write {
            let changes = self.unwritten();
            self.flush(changes, vec![then], cx);
        } else {
            self.go_on(then, cx);
        }
    }

    /// A page's `code.flush`: written now (Auto, Always), or only answered (Manual).
    pub(crate) fn page_flush(&mut self, request: serde_json::Value, cx: &mut Cx) {
        if self.settings.write_mode == WriteMode::Manual {
            return self.go_on(AfterFlush::Page(request), cx);
        }
        let changes = self.unwritten();
        self.flush(changes, vec![AfterFlush::Page(request.clone())], cx);
        // The page waits 3 s for its answer: by then it comes, what is not written yet in `left`.
        if self.flushing.as_ref().is_some_and(|f| f.then.contains(&AfterFlush::Page(request.clone()))) {
            cx.after::<App>(self.inbox, PAGE_FLUSH_WAIT_MS, move |app, _cx| app.page_flush_late(request));
        }
    }

    /// A page's flush not done in time: answered now; its writes still go on.
    fn page_flush_late(&mut self, request: serde_json::Value) {
        let Some(f) = &mut self.flushing else { return };
        let Some(i) = f.then.iter().position(|t| *t == AfterFlush::Page(request.clone())) else { return };
        f.then.remove(i);
        let written = f.written;
        let left = self.unwritten_count();
        self.post_page(serde_json::json!({ "type": "code.flushed", "request": request, "written": written, "left": left }));
    }

    pub(crate) fn post_page(&self, msg: serde_json::Value) {
        if let Some(post) = &self.to_page {
            post(msg);
        }
    }

    /// A written change becomes part of the flush's one step of the history; false when it was not
    /// part of a flush.
    pub(crate) fn record_flushed(&mut self, request: u32, step: Step) -> bool {
        let Some(&group) = self.flush_groups.get(&request) else { return false };
        self.flush_edit();
        if let Some(h) = self.history_mut() {
            if let Some(Step::Group { group: g, steps }) = h.undo.last_mut()
                && *g == group
            {
                steps.push(step);
                return true;
            }
            h.undo.push(Step::Group { group, steps: vec![step] });
            h.redo.clear();
        }
        true
    }

    /// Always: the changes a new step made, written once nothing is being edited or dragged.
    pub(crate) fn note_change(&mut self, step: &Step) {
        if self.settings.write_mode != WriteMode::Always {
            return;
        }
        let made = match step {
            Step::Values { node, values, .. } => values.iter().map(|(p, ..)| Unwritten::Prop(*node, p.clone())).collect(),
            Step::Add { node, gone: false, .. } => vec![Unwritten::Insert(*node)],
            Step::Delete { node, .. } => vec![Unwritten::Remove(*node)],
            Step::Move { node, .. } => vec![Unwritten::Move(*node)],
            _ => vec![],
        };
        if made.is_empty() {
            return;
        }
        for c in made {
            if !self.always_due.contains(&c) {
                self.always_due.push(c);
            }
        }
        if let Some(w) = &self.waker {
            w.invalidate(self.inbox, Dirty::APPLY);
        }
    }

    /// Always: what is due, written now, unless an edit is open or a control is being dragged.
    pub(crate) fn write_if_due(&mut self, cx: &mut Cx) {
        if self.always_due.is_empty() || self.flushing.is_some() || self.edit_prop.is_some() || self.drag.is_some_and(|d| d.active) {
            return;
        }
        let now = self.unwritten();
        let due: Vec<Unwritten> = std::mem::take(&mut self.always_due).into_iter().filter(|c| now.contains(c)).collect();
        if !due.is_empty() {
            self.flush(due, vec![], cx);
        }
    }
}
