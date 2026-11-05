//! The code side, in the shape of the NX language protocol (`@nx-lang/language-protocol` 0.9.0):
//! a language service reads a file into a [`SourceTree`], and answers a [`SourceEdit`] aimed at one
//! node with the [`TextEdit`]s that make it. Field names match NX's so an `<nx-viewer>` can read a
//! tree from any of our services. Only what the services fill today is here; NX's declaration
//! table and visualizers come when the editor needs them.

use serde::{Deserialize, Serialize};

/// A zero-based line and UTF-16 column, as editors count.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TextPosition {
    pub line: u32,
    pub character: u32,
}

/// A range in a document.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EditorRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

/// What a node of the source is. A subset of NX's `SourceRole`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SourceRole {
    /// A control constructed in the code (`new SkiaLabel { ... }`, `<SkiaLabel ... />`).
    Element,
    /// A property set on an element.
    Attribute,
    /// A literal value an editor can change.
    Literal,
    /// A value an editor cannot change in place (an expression, a call, a binding).
    Reference,
}

/// One piece of the source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceNode {
    pub role: SourceRole,
    pub range: EditorRange,
    /// The index of the enclosing node in `nodes`; absent for a top-level node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<usize>,
    /// The node's path in the document, stable under edits elsewhere. Compare, never split.
    pub key: String,
    /// An element's type, an attribute's property name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// A literal as written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// A document's nodes in order of start offset, each parent before its children.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceTree {
    pub uri: String,
    pub nodes: Vec<SourceNode>,
}

/// A value to write; the service decides its spelling.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SourceEditValue {
    String { value: String },
    Number { value: f64 },
    Boolean { value: bool },
    /// A case of a union or enum, such as `Fill` or `Center`.
    Case { name: String },
}

/// One change aimed at one node. NX's kinds, plus `insertElement` for the toolbox.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SourceEdit {
    SetValue { value: SourceEditValue },
    Remove,
    AddProperty { name: String, value: SourceEditValue },
    Move { to: usize },
    InsertElement { type_name: String, index: usize },
}

/// Why an edit was refused.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SourceEditRefusal {
    UnknownNode,
    NotEditable,
    InvalidValue,
}

/// One text replacement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TextEdit {
    pub range: EditorRange,
    pub new_text: String,
}

/// The edits that make a change, or none and why. Ranges are in the document as sent and never
/// overlap, so they apply together as one change.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceEditAnswer {
    pub edits: Vec<TextEdit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused: Option<SourceEditRefusal>,
}
