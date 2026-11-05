//! Moving controls from Layers: a row dragged between rows or onto a container, or Alt+Up / Alt+Down
//! among its siblings. Live first ([`HubToApp::Move`], answered by [`AppMsg::Moved`]); the row then
//! has the dot, Undo and Write as an added control has, and Write moves it in the code
//! ([`EditorMsg::MoveCode`]). While a control is dragged over Layers the rows open a gap where it
//! will land, the list moves when the pointer nears its top or bottom, and Esc puts it all back.

use super::*;

/// A control moved here: where it was before (its parent and index, to move it back) and, once
/// written, the write Undo takes back. Kept by `#id` before the write, by its place after.
#[derive(Clone, Debug)]
pub(crate) struct MovedRow {
    pub from: (u64, u32),
    pub written: Option<u32>,
}

/// The gap the rows open where a dragged control will land: where it is (rows, from the top of
/// the list) and how open (0 shut, 1 a row high), easing toward where it should be.
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct Gap {
    pub pos: f32,
    pub open: f32,
    pub target_pos: f32,
    pub target_open: f32,
    pub ticking: bool,
    /// The tree depth the control lands at (kept while the gap shuts).
    pub depth: usize,
}

/// Milliseconds between two frames of the gap and of the moving list.
const TICK_MS: f32 = 16.0;
/// Points from the top or bottom of Layers where a held drag keeps the list moving, and points per tick.
const EDGE_ZONE: f32 = 32.0;
const EDGE_STEP: f32 = 8.0;

impl App {
    /// How far Layers row `index` is pushed down by the gap, points.
    pub(crate) fn row_shift(&self, index: usize) -> f32 {
        if self.gap.open <= 0.001 {
            return 0.0;
        }
        view::ROW * self.gap.open * (index as f32 - self.gap.pos + 1.0).clamp(0.0, 1.0)
    }

    /// The row a drop opens the gap before: the line's row, or after what a container row holds.
    fn gap_row(&self, drop: &DropAt) -> usize {
        if !drop.into {
            return drop.row;
        }
        let depth = self.rows.get(drop.row).map_or(0, |r| r.depth);
        drop.row + 1 + self.rows.iter().skip(drop.row + 1).take_while(|r| r.depth > depth).count()
    }

    /// The drop changed: the gap eases toward it (or shuts), one tick at a time.
    pub(crate) fn aim_gap(&mut self, cx: &mut Cx) {
        self.set_gap_target();
        self.start_tick(cx);
    }

    /// Where the gap goes for the drop now, and the depth the control lands at; no drop: it shuts.
    fn set_gap_target(&mut self) {
        match self.drop {
            Some(d) => {
                let row = self.gap_row(&d) as f32;
                // A gap opening from shut starts where it lands.
                if self.gap.open <= 0.001 {
                    self.gap.pos = row;
                }
                (self.gap.target_pos, self.gap.target_open) = (row, 1.0);
                self.gap.depth = if d.into { d.depth + 1 } else { d.depth };
            }
            None => self.gap.target_open = 0.0,
        }
    }

    fn start_tick(&mut self, cx: &mut Cx) {
        if !self.gap.ticking {
            self.gap.ticking = true;
            cx.after(self.tree_list, TICK_MS, gap_tick);
        }
    }

    /// The control dragged over Layers from the row of `node` (a move), or None (from Controls).
    pub(crate) fn dragged_node(&self) -> Option<u64> {
        self.drag.and_then(|d| match d.what {
            DragWhat::Row(id) => Some(id),
            DragWhat::Control(_) => None,
        })
    }

    /// Whether `node` can go into `parent`: not into itself or what it holds.
    pub(crate) fn can_move_into(&self, node: u64, parent: u64) -> bool {
        let mut at = Some(parent);
        while let Some(id) = at {
            if id == node {
                return false;
            }
            at = self.tree.nodes.iter().find(|n| n.id == id).and_then(|n| n.parent);
        }
        true
    }

    /// Moves `node` live into `parent` at `index` (counted with the control still where it is, as
    /// a drop counts; turned into the index after the move here). `record`: a new action of the
    /// history (not Undo or Redo).
    pub(crate) fn move_node(&mut self, node: u64, parent: u64, index: u32, record: bool) -> bool {
        if self.tree_stale {
            return false;
        }
        let Some(from_parent) = self.tree.nodes.iter().find(|n| n.id == node && !self.is_removed(n.id)).and_then(|n| n.parent) else { return false };
        if !self.accepts_children(parent) || !self.can_move_into(node, parent) {
            return false;
        }
        let from_index = self.live_index(from_parent, node);
        // Under its own parent, after itself: one less once it left its place.
        let index = if parent == from_parent && index > from_index { index - 1 } else { index };
        if parent == from_parent && index == from_index {
            return false;
        }
        let request = self.request();
        self.move_requests.insert(request, (node, (from_parent, from_index), record));
        self.tree_requests.insert(request);
        self.send(HubToApp::Move { request, node, parent, index });
        true
    }

    /// The app moved a control (or refused: `node` None, its error follows).
    pub(crate) fn moved_answer(&mut self, request: u32, node: Option<u64>, parent: Option<u64>, index: Option<u32>) {
        let Some((asked, from, record)) = self.move_requests.remove(&request) else { return };
        let type_name = self.tree.nodes.iter().find(|n| n.id == asked).map(|n| short_type(&n.type_name).to_owned()).unwrap_or_default();
        let (Some(id), Some(parent), Some(index)) = (node, parent, index) else {
            self.status = i18n::tr_args("status-not-moved", &[("type", type_name.into())]);
            self.status_error = true;
            return;
        };
        self.tree_requests.remove(&request);
        // A control added here and not written is simply somewhere else; one the code makes is
        // marked moved, from where the code has it (its first place).
        if self.inserted_state(id) != Some(false) {
            let key = format!("#{id}");
            match self.moved.get(&key).map(|m| m.from) {
                Some(first) if first == (parent, index) => {
                    self.moved.remove(&key);
                }
                Some(_) => {}
                None => {
                    self.moved.insert(key, MovedRow { from, written: None });
                }
            }
        }
        if record {
            self.record(Step::Move { node: id, type_name: type_name.clone(), from, to: (parent, index) });
        }
        self.status = i18n::tr_args("status-moved", &[("type", type_name.into())]);
        self.status_error = false;
        self.select_moved = Some(id);
    }

    /// The key of control `id` among the controls moved here: `#id` before the write, its place after.
    pub(crate) fn moved_key(&self, id: u64) -> Option<String> {
        if self.moved.is_empty() {
            return None;
        }
        let key = format!("#{id}");
        if self.moved.contains_key(&key) {
            return Some(key);
        }
        let path = self.node_path(id);
        self.moved.get(&path).filter(|m| m.written.is_some()).map(|_| path)
    }

    /// Write on a moved row: the move into the code. The write's request.
    pub(crate) fn write_move(&mut self, id: u64, cx: &mut Cx) -> Option<u32> {
        let app = self.app?;
        let node = self.tree.nodes.iter().find(|n| n.id == id).cloned()?;
        let parent = node.parent?;
        let index = self.live_index(parent, id);
        let request = self.request();
        self.move_code.insert(request, id);
        self.send_hub(EditorMsg::MoveCode { request, app, node: id, parent, index });
        self.status = i18n::tr_args("status-writing", &[("prop", short_type(&node.type_name).to_owned().into())]);
        self.status_error = false;
        cx.items_reset(self.tree_list);
        Some(request)
    }

    /// Undo on a moved row: before the write it goes back live; after, the write is taken back
    /// first, then it goes back (on CodeUndone).
    pub(crate) fn undo_move(&mut self, id: u64, cx: &mut Cx) {
        let Some(key) = self.moved_key(id) else { return };
        let Some(m) = self.moved.get(&key).cloned() else { return };
        match m.written {
            None => {
                self.moved.remove(&key);
                self.move_node(id, m.from.0, self.drop_index(id, m.from.0, m.from.1), false);
            }
            Some(request) => {
                self.undo_moves.insert(request, (id, key));
                self.send_hub(EditorMsg::UndoCode { request });
                let name = self.tree.nodes.iter().find(|n| n.id == id).map(|n| short_type(&n.type_name).to_owned()).unwrap_or_default();
                self.status = i18n::tr_args("status-undoing", &[("prop", name.into())]);
                self.status_error = false;
            }
        }
        cx.items_reset(self.tree_list);
    }

    /// `index` (after the move) as a drop counts it, the control still where it is.
    pub(crate) fn drop_index(&self, node: u64, parent: u64, index: u32) -> u32 {
        let here = self.tree.nodes.iter().find(|n| n.id == node).and_then(|n| n.parent) == Some(parent);
        if here && index >= self.live_index(parent, node) { index + 1 } else { index }
    }

    /// Alt+Up / Alt+Down: the selected control one place up or down among its siblings.
    pub(crate) fn move_selected_by(&mut self, step: i32) -> bool {
        let Some(node) = self.selected_live().filter(|n| n.parent.is_some() && !self.locked(n)).cloned() else { return false };
        let parent = node.parent.unwrap_or_default();
        let at = self.live_index(parent, node.id) as i32;
        let to = at + step;
        if to < 0 || to >= self.live_children(parent) as i32 {
            return false;
        }
        self.move_node(node.id, parent, self.drop_index(node.id, parent, to as u32), true)
    }
}

/// One frame of a drag over Layers: the list moves when the pointer is near its top or bottom (the
/// drop follows), and the gap eases toward the drop. Stops once the drag is over and the gap still.
fn gap_tick(app: &mut App, cx: &mut Cx) {
    // The list moves under a pointer held at an edge.
    if let (Some(d), Some(base)) = (app.drag.filter(|d| d.active), cx.base(app.layers_scroll)) {
        let (s, r) = (base.scale.max(0.1), base.rect);
        let y = d.at.y * s;
        let inside = d.at.x * s >= r.left && d.at.x * s <= r.right;
        let direction = if !inside {
            0.0
        } else if y < r.top + EDGE_ZONE * s && y >= r.top {
            1.0
        } else if y > r.bottom - EDGE_ZONE * s && y <= r.bottom {
            -1.0
        } else {
            0.0
        };
        if direction != 0.0 {
            let offset = cx.find::<SkiaScroll>(app.layers_scroll).map_or(app.layers_offset, |sc| sc.viewport_offset_y());
            cx.scroll_to(app.layers_scroll, 0.0, offset + direction * EDGE_STEP, 0);
            let point = drawnui::skia::Point::new(d.at.x * s, d.at.y * s);
            app.drop = app.drop_target(point, cx);
            app.set_gap_target();
        }
    }
    let g = &mut app.gap;
    // Eased: a third of the way each frame, then still.
    let ease = |now: &mut f32, to: f32| {
        *now += (to - *now) * 0.35;
        if (to - *now).abs() < 0.01 {
            *now = to;
        }
    };
    ease(&mut g.pos, g.target_pos);
    ease(&mut g.open, g.target_open);
    let still = g.pos == g.target_pos && g.open == g.target_open;
    if still && app.drag.is_none() {
        app.gap.ticking = false;
        return;
    }
    cx.after(app.tree_list, TICK_MS, gap_tick);
}
