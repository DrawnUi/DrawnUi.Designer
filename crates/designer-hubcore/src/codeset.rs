//! What the app's code sets on a control ([`HubToEditor::CodeSet`]), answered after every
//! [`AppMsg::Props`]. The project's files are parsed once and again only when one changes (its
//! [`CodeStore`] stamp), and the match of an app's tree is kept until the tree or a file changes, so a
//! selection costs a lookup.

use std::collections::HashMap;
use std::sync::Arc;

use designer_sources::{ElementCode, RuntimeMatch};

use super::*;

#[derive(Default)]
struct ProjectCode {
    /// File uri -> the stamp it was read at.
    stamps: HashMap<String, Stamp>,
    texts: HashMap<String, String>,
    code: HashMap<String, HashMap<String, ElementCode>>,
    /// Rises whenever a file is added, changed or removed.
    generation: u64,
}

struct AppMatches {
    fingerprint: (u64, usize, u64),
    generation: u64,
    matches: Arc<HashMap<NodeId, RuntimeMatch>>,
}

/// The parsed sources of every project and the match of every app's tree.
#[derive(Default)]
pub struct CodeCache {
    projects: HashMap<String, ProjectCode>,
    apps: HashMap<AppId, AppMatches>,
}

impl ProjectCode {
    /// Reads what changed under `dir` since the last call.
    fn refresh(&mut self, store: &dyn CodeStore, dir: &Path) {
        let found = store.list(dir);
        let mut changed = self.stamps.len() != found.len();
        let mut stamps = HashMap::new();
        for (uri, stamp) in found {
            if self.stamps.get(&uri) != Some(&stamp) {
                changed = true;
                let Some(text) = store.read(&uri).ok().and_then(|b| utf8(&b).map(str::to_owned)) else { continue };
                self.code.insert(uri.clone(), designer_sources::element_code(&uri, &text));
                self.texts.insert(uri.clone(), text);
            }
            stamps.insert(uri, stamp);
        }
        self.texts.retain(|u, _| stamps.contains_key(u));
        self.code.retain(|u, _| stamps.contains_key(u));
        self.stamps = stamps;
        if changed {
            self.generation += 1;
        }
    }
}

impl CodeCache {
    /// The properties the code of `project` sets on `node` of the app's `tree`; `None` when no code
    /// creates it (a part the library builds, XAML, no match).
    pub fn names(&mut self, store: &dyn CodeStore, app: AppId, project: &str, tree: &Tree, node: NodeId) -> Option<Vec<String>> {
        if tree.nodes.iter().find(|n| n.id == node)?.internal {
            return None;
        }
        let pc = self.projects.entry(project_key(project)).or_default();
        pc.refresh(store, project_dir(project));
        let fingerprint = (tree.revision, tree.nodes.len(), tree.nodes.iter().fold(0u64, |s, n| s.wrapping_mul(31).wrapping_add(n.id)));
        let fresh = self.apps.get(&app).is_some_and(|m| m.fingerprint == fingerprint && m.generation == pc.generation);
        if !fresh {
            let texts: Vec<(String, String)> = pc.texts.iter().map(|(u, t)| (u.clone(), t.clone())).collect();
            let matches = designer_sources::match_runtime(&texts, tree).into_iter().map(|m| (m.node, m)).collect();
            self.apps.insert(app, AppMatches { fingerprint, generation: pc.generation, matches: Arc::new(matches) });
        }
        let m = self.apps[&app].matches.get(&node)?;
        designer_sources::code_set_names(m, &pc.code)
    }

    pub(crate) fn forget(&mut self, app: AppId) {
        self.apps.remove(&app);
    }
}
