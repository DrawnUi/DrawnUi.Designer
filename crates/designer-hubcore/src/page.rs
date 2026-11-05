//! The code of a hub inside a web page (DrawFiddle): one snippet, held by the page's code editor.
//! The page sends its text with a version on every change; a write becomes one edit in UTF-16 code
//! units (what a browser editor such as Monaco counts in), posted to the page, and the text is
//! updated at once, before the page answers with its next version.

use std::path::Path;
use std::sync::Mutex;

use super::{CodeStore, Stamp};

/// The snippet's uri until the page names its own (a C# snippet; a React one is `fiddle:///snippet.tsx`).
pub const SNIPPET: &str = "fiddle:///snippet.cs";

/// Whether `uri` is a page snippet the hub takes (`fiddle:///snippet.<ext>`, any language the code
/// reader knows by its extension).
pub fn is_snippet(uri: &str) -> bool {
    uri.strip_prefix("fiddle:///snippet.").is_some_and(|ext| !ext.is_empty() && !ext.contains('/'))
}

/// The virtual project whose only file is [`SNIPPET`]: [`super::HubCore::page_project`].
pub const PROJECT: &str = "fiddle:///";

/// Replaces `start..end` (UTF-16 code units) with `text`.
#[derive(Debug, Clone, PartialEq)]
pub struct Utf16Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// What the store asks of the page.
#[derive(Debug, Clone, PartialEq)]
pub enum ToPage {
    /// Apply these edits, made against the text at `version`, as one undo step.
    Edit { version: u64, edits: Vec<Utf16Edit> },
    /// Send the current text.
    Get,
}

/// The one span between two texts (common start and end left out), in UTF-16 code units of `old`.
/// `None` when they are equal.
pub fn utf16_edit(old: &str, new: &str) -> Option<Utf16Edit> {
    if old == new {
        return None;
    }
    // Whole chars, so a span never starts or ends inside one.
    let head: usize = old.chars().zip(new.chars()).take_while(|(a, b)| a == b).map(|(c, _)| c.len_utf8()).sum();
    let tail: usize = old[head..].chars().rev().zip(new[head..].chars().rev()).take_while(|(a, b)| a == b).map(|(c, _)| c.len_utf8()).sum();
    let start = old[..head].encode_utf16().count();
    Some(Utf16Edit { start, end: start + old[head..old.len() - tail].encode_utf16().count(), text: new[head..new.len() - tail].to_owned() })
}

struct State {
    /// The snippet's uri, as the page named it last.
    uri: String,
    /// `None` until the page sent the text.
    text: Option<String>,
    version: u64,
    /// Rises on every change of `text` (the [`Stamp`] the hub's parse cache follows).
    changes: u64,
    /// A write was posted and the page has not sent a text since.
    waiting: bool,
}

/// The [`CodeStore`] of a page hub.
pub struct PageCode {
    state: Mutex<State>,
    post: Box<dyn Fn(ToPage) + Send + Sync>,
}

impl PageCode {
    pub fn new(post: impl Fn(ToPage) + Send + Sync + 'static) -> Self {
        Self { state: Mutex::new(State { uri: SNIPPET.to_owned(), text: None, version: 0, changes: 0, waiting: false }), post: Box::new(post) }
    }

    /// The snippet's uri, as the page named it last.
    pub fn uri(&self) -> String {
        self.state.lock().unwrap().uri.clone()
    }

    /// [`Self::set_text`] for a snippet the page names (`fiddle:///snippet.tsx` for React): a new
    /// name (another language) is a new file for the hub.
    pub fn set_text_for(&self, uri: &str, text: String, version: u64) {
        {
            let mut s = self.state.lock().unwrap();
            if s.uri != uri {
                s.uri = uri.to_owned();
                s.changes += 1;
            }
        }
        self.set_text(text, version);
    }

    /// The page's text at `version`: its answer to [`ToPage::Get`] and every change after: the
    /// truth, also over a write the page has not taken. Except its text from before a write posted
    /// at that same version (or older), sent again before the page took the write: the write is on
    /// its way, and that text would have the next write made again over it.
    pub fn set_text(&self, text: String, version: u64) {
        let mut s = self.state.lock().unwrap();
        if s.waiting && version <= s.version {
            return;
        }
        if s.text.as_deref() != Some(text.as_str()) {
            s.changes += 1;
        }
        s.text = Some(text);
        s.version = version;
        s.waiting = false;
    }
}

impl CodeStore for PageCode {
    fn list(&self, _dir: &Path) -> Vec<(String, Stamp)> {
        let s = self.state.lock().unwrap();
        s.text.as_ref().map(|t| (s.uri.clone(), (t.len() as u64, s.changes))).into_iter().collect()
    }

    fn read(&self, uri: &str) -> Result<Vec<u8>, String> {
        let s = self.state.lock().unwrap();
        match &s.text {
            Some(text) if uri == s.uri => Ok(text.as_bytes().to_vec()),
            _ => Err(format!("{uri} is not in the page")),
        }
    }

    fn write(&self, uri: &str, bytes: &[u8]) -> Result<(), String> {
        let new = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
        let mut s = self.state.lock().unwrap();
        if uri != s.uri {
            return Err(format!("{uri} is not in the page"));
        }
        let Some(old) = s.text.as_deref() else { return Err(format!("{uri} is not in the page")) };
        // Two edits stamped with one version would not fit: the second waits for the page's answer.
        if s.waiting {
            (self.post)(ToPage::Get);
            return Err("the editor has not taken the last edit yet".into());
        }
        if let Some(edit) = utf16_edit(old, new) {
            (self.post)(ToPage::Edit { version: s.version, edits: vec![edit] });
            s.text = Some(new.to_owned());
            s.changes += 1;
            s.waiting = true;
        }
        Ok(())
    }

    fn dir_exists(&self, dir: &Path) -> bool {
        dir == Path::new(PROJECT)
    }
}
