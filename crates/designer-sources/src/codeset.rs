//! Which properties the code sets on each control it creates: what an editor shows as "changed".

use std::collections::HashMap;

use super::*;

/// What the code that creates one element sets on it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ElementCode {
    /// The type as written (`SkiaLabel`); `None` for `this` and an untyped `new()`.
    pub type_name: Option<String>,
    /// Property names, in source order, without duplicates.
    pub names: Vec<String>,
}

/// For every element of a file, keyed like [`read_csharp`]'s keys: the properties its initializer
/// (or constructor body, for `Class/this`) sets, its text constructor argument (`new SkiaLabel("..")`
/// sets Text) and its fluent setters (`.WithMargin(..)`, `.FillX()`, ...).
pub fn element_code(uri: &str, text: &str) -> HashMap<String, ElementCode> {
    if crate::tsx::is_tsx(uri) {
        return crate::tsx::element_code(uri, text);
    }
    if crate::rs::is_rust(uri) {
        return crate::rs::element_code(uri, text);
    }
    let ts = parse(text);
    let (src, handles) = walk(&ts, text, uri);
    let mut out = HashMap::new();
    for (i, n) in src.nodes.iter().enumerate().filter(|(_, n)| n.role == SourceRole::Element) {
        let mut names: Vec<String> = vec![];
        let mut add = |name: &str| {
            if !name.is_empty() && !names.iter().any(|x| x == name) {
                names.push(name.to_string());
            }
        };
        let h = handles[i];
        if is_creation(h) {
            let short = n.name.as_deref().and_then(|t| t.rsplit('.').next()).unwrap_or_default();
            if TEXT_CTORS.contains(&short) && call_args(h).len() == 1 {
                add("Text");
            }
        }
        for a in (i + 1..src.nodes.len()).filter(|&j| src.nodes[j].parent == Some(i) && src.nodes[j].role == SourceRole::Attribute) {
            add(src.nodes[a].name.as_deref().unwrap_or_default());
        }
        if is_creation(h) {
            let top = chain_top(h);
            let mut x = h;
            while x != top {
                let Some(p) = x.parent() else { break };
                if p.kind() == "invocation_expression"
                    && let Some(name) = p.child_by_field_name("function").and_then(|f| f.child_by_field_name("name"))
                {
                    fluent_props(&text[name.byte_range()]).iter().for_each(|n| add(n));
                }
                x = p;
            }
        }
        out.insert(n.key.clone(), ElementCode { type_name: n.name.clone(), names });
    }
    out
}

/// The controls whose constructor takes their text: `new SkiaLabel("Hi")` sets Text.
pub(crate) const TEXT_CTORS: [&str; 3] = ["SkiaLabel", "SkiaRichLabel", "SkiaButton"];

/// The properties a DrawnUI fluent call sets (`WithMargin` -> Margin, `FillX` -> HorizontalOptions).
fn fluent_props(method: &str) -> Vec<&str> {
    let h = "HorizontalOptions";
    let v = "VerticalOptions";
    match method {
        "Fill" | "Center" => vec![h, v],
        "FillX" | "CenterX" | "StartX" | "EndX" => vec![h],
        "FillY" | "CenterY" | "StartY" | "EndY" => vec![v],
        "CenterText" => vec!["HorizontalTextAlignment", "VerticalTextAlignment"],
        "CenterTextX" => vec!["HorizontalTextAlignment"],
        "CenterTextY" => vec!["VerticalTextAlignment"],
        "Shape" | "WithShapeType" => vec!["Type"],
        "WithCache" => vec!["UseCache"],
        "WithEnabled" => vec!["IsEnabled"],
        "WithVisibility" => vec!["IsVisible"],
        "WithHeight" => vec!["HeightRequest"],
        "WithWidth" => vec!["WidthRequest"],
        "WithGestures" | "WithParent" => vec![],
        m if m.starts_with("WithAccessibility") => vec![],
        m => m.strip_prefix("With").filter(|p| p.starts_with(char::is_uppercase)).map_or(vec![], |p| vec![p]),
    }
}

/// The properties the code sets on a matched control: its element's, plus what its class's
/// constructor sets when the element creates a class built there (`new ChipCell()`).
/// `code` maps each file's uri to its [`element_code`].
pub fn code_set_names(m: &RuntimeMatch, code: &HashMap<String, HashMap<String, ElementCode>>) -> Option<Vec<String>> {
    let element = code.get(&m.uri)?.get(&m.key)?;
    let mut names = element.names.clone();
    let class = element.type_name.as_deref().and_then(|t| t.split('<').next()).and_then(|t| t.rsplit('.').next());
    if let Some(class) = class {
        let (top, nested) = (format!("{class}/this"), format!(".{class}/this"));
        let this: Vec<&ElementCode> = code.values().flat_map(|f| f.iter().filter(|(k, _)| **k == top || k.ends_with(&nested)).map(|(_, c)| c)).collect();
        if let [one] = this.as_slice() {
            for n in &one.names {
                if !names.contains(n) {
                    names.push(n.clone());
                }
            }
        }
    }
    Some(names)
}
