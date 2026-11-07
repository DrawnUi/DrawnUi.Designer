//! Writing a property of a running control back into the C# that creates it.

use designer_protocol::Refusal;
use designer_protocol::{NodeId, Tree as RuntimeTree};

use super::*;

/// A file's new text after one property edit.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeEdit {
    pub uri: String,
    pub text: String,
    /// The written text in the new file.
    pub range: EditorRange,
}

/// A property value to write.
#[derive(Debug, Clone, PartialEq)]
pub enum PropValue {
    /// A number, string, boolean or case: written as a literal.
    Plain(SourceEditValue),
    /// `0xAARRGGBB`: written as `Color.Parse("#..")` or `Colors.Name`.
    Color(u32),
    /// Left, top, right, bottom: written as `new Thickness(..)`.
    Thickness([f64; 4]),
}

/// The value to write for a property of .NET type `type_name` (as the agent's `getProps` reports
/// it), from the text an editor sent. `current` is the property's value now, as the agent
/// formatted it: an enum (or a static member such as `LayoutOptions.Center`) shows as a bare name.
pub fn value_from_runtime(type_name: &str, current: &str, text: &str) -> Result<PropValue, Refusal> {
    let t = text.trim();
    let short = type_name.rsplit(['.', '+']).next().unwrap_or(type_name);
    let plain = |v| Ok(PropValue::Plain(v));
    match type_name {
        "System.Double" | "System.Single" | "System.Decimal" | "System.Int32" | "System.Int64" | "System.Int16"
        | "System.Byte" | "System.UInt32" => match t.parse::<f64>() {
            Ok(value) if value.is_finite() => plain(SourceEditValue::Number { value }),
            _ => Err(Refusal::new("src-not-number", &[("text", &t)])),
        },
        "System.Boolean" => match t.to_ascii_lowercase().as_str() {
            "true" => plain(SourceEditValue::Boolean { value: true }),
            "false" => plain(SourceEditValue::Boolean { value: false }),
            _ => Err(Refusal::new("src-not-bool", &[("text", &t)])),
        },
        "System.String" => plain(SourceEditValue::String { value: text.into() }),
        _ if !type_name.starts_with("System.") && short == "Color" => {
            parse_color(t).map(|(argb, _)| PropValue::Color(argb)).ok_or_else(|| Refusal::new("src-not-color", &[("text", &t)]))
        }
        // A corner radius the same on every corner is a number in C# (`CornerRadius = 10`).
        _ if !type_name.starts_with("System.") && short == "CornerRadius" => match parse_thickness(t) {
            Some([a, b, c, d]) if a == b && b == c && c == d => plain(SourceEditValue::Number { value: a }),
            _ => Err(Refusal::new("src-type-unsupported", &[("type", &type_name)])),
        },
        _ if !type_name.starts_with("System.") && short == "Thickness" => {
            parse_thickness(t).map(PropValue::Thickness).ok_or_else(|| Refusal::new("src-not-thickness", &[("text", &t)]))
        }
        // Types without their own ToString show their type name: not a case. A generic type
        // (`Cow<'static, str>`, Rust) is never an enum.
        _ if !type_name.starts_with("System.") && !type_name.contains('<') && is_ident(current) && current != short && is_ident(t) => {
            plain(SourceEditValue::Case { name: format!("{short}.{t}") })
        }
        _ => Err(Refusal::new("src-type-unsupported", &[("type", &type_name)])),
    }
}

/// `#RGB`, `#ARGB`, `#RRGGBB`, `#AARRGGBB` (the `#` optional): the color and its digit count.
fn parse_color(s: &str) -> Option<(u32, usize)> {
    let h = s.strip_prefix('#').unwrap_or(s);
    if !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    let nib = |n: u32| (n & 0xF) * 0x11;
    let argb = match h.len() {
        3 => 0xFF00_0000 | nib(v >> 8) << 16 | nib(v >> 4) << 8 | nib(v),
        4 => nib(v >> 12) << 24 | nib(v >> 8) << 16 | nib(v >> 4) << 8 | nib(v),
        6 => 0xFF00_0000 | v,
        8 => v,
        _ => return None,
    };
    Some((argb, h.len()))
}

/// `a`, `h,v` or `l,t,r,b`.
fn parse_thickness(s: &str) -> Option<[f64; 4]> {
    let v: Vec<f64> = s.split(',').map(|x| x.trim().parse::<f64>().ok().filter(|f| f.is_finite())).collect::<Option<_>>()?;
    match v.as_slice() {
        [a] => Some([*a; 4]),
        [h, v] => Some([*h, *v, *h, *v]),
        [l, t, r, b] => Some([*l, *t, *r, *b]),
        _ => None,
    }
}

/// The hex digits of a color, in the length `old` had when it still fits (`None`: `#RRGGBB`, or
/// `#AARRGGBB` when not opaque).
pub(crate) fn color_hex(argb: u32, old: Option<usize>, upper: bool) -> String {
    let bytes = argb.to_be_bytes();
    let short_ok = |bs: &[u8]| bs.iter().all(|b| b >> 4 == b & 0xF);
    let opaque = bytes[0] == 0xFF;
    let s = match old {
        Some(3) if opaque && short_ok(&bytes[1..]) => format!("{:x}{:x}{:x}", bytes[1] & 0xF, bytes[2] & 0xF, bytes[3] & 0xF),
        Some(4) if short_ok(&bytes) => format!("{:x}{:x}{:x}{:x}", bytes[0] & 0xF, bytes[1] & 0xF, bytes[2] & 0xF, bytes[3] & 0xF),
        Some(8) => format!("{argb:08x}"),
        _ if opaque => format!("{:06x}", argb & 0xFF_FFFF),
        _ => format!("{argb:08x}"),
    };
    if upper { s.to_uppercase() } else { s }
}

pub(crate) fn color_parse_call(argb: u32) -> String {
    format!("Color.Parse(\"#{}\")", color_hex(argb, None, true))
}

/// The shortest `Thickness` constructor arguments for a value.
pub(crate) fn thickness_args([l, t, r, b]: [f64; 4]) -> String {
    let n = |v: f64| format!("{v}");
    if l == t && t == r && r == b {
        n(l)
    } else if l == r && t == b {
        format!("{}, {}", n(l), n(t))
    } else {
        format!("{}, {}, {}, {}", n(l), n(t), n(r), n(b))
    }
}

/// Sets property `prop` of runtime control `node` in the code that creates it: the literal the
/// element's initializer (or constructor) sets it to, the literal argument a helper passes into
/// it, or a new `prop = value` when the element does not set it. Every instance of one
/// declaration (template cells, helper bodies) shares the edit. Errors say why nothing is written.
pub fn set_property(files: &[(String, String)], tree: &RuntimeTree, node: NodeId, prop: &str, value: &PropValue) -> Result<CodeEdit, Refusal> {
    let rn = tree.nodes.iter().find(|n| n.id == node).ok_or_else(|| Refusal::new("src-not-in-tree", &[]))?;
    if rn.internal {
        return Err(Refusal::new("src-library-part", &[]));
    }
    let m = match_runtime(files, tree).into_iter().find(|m| m.node == node).ok_or_else(|| Refusal::new("src-no-code", &[]))?;
    if crate::tsx::is_tsx(&m.uri) {
        return crate::tsx::set_property(files, &m, prop, value);
    }
    if crate::rs::is_rust(&m.uri) {
        return crate::rs::set_property(files, &m, prop, value);
    }
    let text = files.iter().find(|(u, _)| *u == m.uri).expect("matched file").1.as_str();
    let ts = parse(text);
    let (src, handles) = walk(&ts, text, &m.uri);
    let attr_key = format!("{}/{prop}", m.key);
    let index = |k: &str| src.nodes.iter().position(|n| n.key == k);

    // A helper parameter: the argument at the innermost call that feeds this attribute.
    let mut target = None;
    let links = if m.via.is_empty() { vec![] } else { resolve_helpers(&src, text) };
    if !m.via.is_empty() {
        for call in m.via.iter().rev() {
            let Some(link) = links.iter().find(|l| &l.call == call) else { continue };
            let flows: Vec<_> = link.params.iter().filter(|f| f.attribute.as_deref() == Some(attr_key.as_str())).collect();
            match flows.as_slice() {
                [] => continue,
                [f] => {
                    let k = f.arg_key.clone().ok_or_else(|| Refusal::new("src-arg-no-value", &[("prop", &prop), ("param", &f.param), ("call", &call)]))?;
                    target = Some(k);
                    break;
                }
                _ => return Err(Refusal::new("src-several-args", &[("prop", &prop), ("call", &call)])),
            }
        }
    }
    // Only the edited control changes, never another made by the same code. The calls that return it
    // (innermost first): the value goes on the outermost, never into the helper. A helper it is made
    // inside of, called more than once, would change every use: refused.
    let direct: Vec<&String> = m.via.iter().rev().take_while(|c| links.iter().any(|l| &l.call == *c && l.returns == m.key)).collect();
    let enclosing = &m.via[..m.via.len() - direct.len()];
    let shared = enclosing.iter().find_map(|c| {
        let link = links.iter().find(|l| &l.call == c)?;
        let uses = links.iter().filter(|l| l.returns == link.returns).count();
        let name = index(c).and_then(|i| src.nodes[i].name.clone()).unwrap_or_default();
        (uses > 1).then(|| Refusal::new("src-shared-helper", &[("helper", &name), ("count", &uses.to_string())]))
    });
    if target.is_none()
        && let Some(call) = direct.last()
    {
        if let Some(r) = shared {
            return Err(r);
        }
        let i = index(call).ok_or_else(|| Refusal::new("src-key-missing", &[("key", call)]))?;
        let el = index(&m.key).expect("matched key");
        let cast = helper_cast(text, handles[el], src.nodes[el].name.as_deref());
        let (a, b, new_text) = on_call(text, expr_of(handles[i]), prop, value, cast.as_deref())?;
        let mut new = text.to_string();
        new.replace_range(a..b, &new_text);
        let range = Lines::new(&new).range(a, a + new_text.len());
        return Ok(CodeEdit { uri: m.uri, text: new, range });
    }
    if target.is_none()
        && let Some(r) = shared
    {
        return Err(r);
    }
    let target = target.or_else(|| index(&attr_key).map(|_| attr_key.clone()));
    let e = match (value, target) {
        (PropValue::Plain(v), Some(key)) => edit_one(text, &src, &key, &SourceEdit::SetValue { value: v.clone() })?,
        // `new SkiaLabel("Hi")`: these constructors take the text, so the argument is where it is set.
        (PropValue::Plain(v), None) => match text_argument(text, &src, &handles, &m.key, prop) {
            Some(lit) => (lit.start_byte(), lit.end_byte(), respell(lit, text, v).map_err(|r| refusal_reason(&m.key, r))?),
            None => edit_one(text, &src, &m.key, &SourceEdit::AddProperty { name: prop.into(), value: v.clone() })?,
        },
        (_, Some(key)) => {
            let i = index(&key).ok_or_else(|| Refusal::new("src-key-missing", &[("key", &key)]))?;
            // An attribute's value: its literal or reference (`/=`), or its element (`/0`).
            let expr = match src.nodes[i].role {
                SourceRole::Attribute => {
                    let v = (i + 1..src.nodes.len()).find(|&j| src.nodes[j].parent == Some(i)).ok_or_else(|| Refusal::new("src-no-value", &[]))?;
                    expr_of(handles[v])
                }
                _ => expr_of(handles[i]),
            };
            rewrite(text, expr, prop, value)?
        }
        (_, None) => {
            let el = index(&m.key).expect("matched key");
            match fluent_setter(text, handles[el], prop) {
                Some(call) => rewrite_args(text, call, prop, value)?,
                None => {
                    let spelled = match value {
                        PropValue::Color(c) => color_parse_call(*c),
                        PropValue::Thickness(t) => format!("new Thickness({})", thickness_args(*t)),
                        PropValue::Plain(_) => unreachable!(),
                    };
                    add_assignment(text, &src, &handles, el, prop, &spelled).map_err(|r| refusal_reason(&m.key, r))?
                }
            }
        }
    };
    let (a, b, new_text) = e;
    let mut new = text.to_string();
    new.replace_range(a..b, &new_text);
    let lines = Lines::new(&new);
    let range = lines.range(a, a + new_text.len());
    Ok(CodeEdit { uri: m.uri, text: new, range })
}

/// A value for a control a helper returns, written on the call that made it (`Tag("Two")`): the
/// argument of the call's `.With{prop}(..)`, or the value of `me.{prop} = ..` in its `.Adapt(..)`, the
/// last one; else a new `.Adapt(me => me.{prop} = value)` after the call's chain. `cast` names the
/// control's type when the helper returns a base type (`((SkiaLabel)me).FontSize`).
fn on_call(text: &str, call: Node, prop: &str, value: &PropValue, cast: Option<&str>) -> Result<(usize, usize, String), Refusal> {
    // The call itself under the chain the list item may already have.
    let call = chain_base(call);
    let top = chain_top(call);
    let mut found = None;
    let mut n = call;
    while n != top {
        let Some(p) = n.parent() else { break };
        if p.kind() == "invocation_expression"
            && let Some(name) = p.child_by_field_name("function").and_then(|f| f.child_by_field_name("name"))
        {
            let name = &text[name.byte_range()];
            if name == format!("With{prop}") {
                found = Some((p, None));
            } else if name == "Adapt"
                && let Some(right) = adapt_value(text, p, prop)
            {
                found = Some((p, Some(right)));
            }
        }
        n = p;
    }
    let shown = |e: Node| text[e.byte_range()].to_string();
    match (found, value) {
        (Some((_, Some(right))), PropValue::Plain(v)) => Ok((right.start_byte(), right.end_byte(), respell(right, text, v).map_err(|_| Refusal::new("src-not-literal", &[("key", &shown(right))]))?)),
        (Some((_, Some(right))), _) => rewrite(text, right, prop, value),
        (Some((with, None)), PropValue::Plain(v)) => match call_args(with).as_slice() {
            [one] => {
                let lit = expr_of(*one);
                Ok((lit.start_byte(), lit.end_byte(), respell(lit, text, v).map_err(|_| Refusal::new("src-not-literal", &[("key", &shown(lit))]))?))
            }
            _ => Err(Refusal::new("src-with-misfit", &[("prop", &prop)])),
        },
        (Some((with, None)), _) => rewrite_args(text, with, prop, value),
        (None, _) => {
            let spelled = match value {
                PropValue::Plain(v) => spell(v).map_err(|_| Refusal::new("src-value-misfit", &[("key", &prop)]))?,
                PropValue::Color(c) => color_parse_call(*c),
                PropValue::Thickness(t) => format!("new Thickness({})", thickness_args(*t)),
            };
            let me = match cast {
                Some(t) => format!("(({t})me)"),
                None => "me".to_owned(),
            };
            Ok((top.end_byte(), top.end_byte(), format!(".Adapt(me => {me}.{prop} = {spelled})")))
        }
    }
}

/// `x.{prop} = value` inside an `.Adapt(..)` call's lambda: the value.
fn adapt_value<'t>(text: &str, adapt: Node<'t>, prop: &str) -> Option<Node<'t>> {
    let mut stack = vec![adapt.child_by_field_name("arguments")?];
    let mut found = None;
    while let Some(n) = stack.pop() {
        if n.kind() == "assignment_expression"
            && let Some(left) = n.child_by_field_name("left")
            && left.kind() == "member_access_expression"
            && left.child_by_field_name("name").is_some_and(|x| &text[x.byte_range()] == prop)
        {
            found = n.child_by_field_name("right");
        }
        let mut c = n.walk();
        stack.extend(n.named_children(&mut c));
    }
    found
}

/// The type to cast `me` to in a new `.Adapt(..)`: the control's own (`type_name`) when the helper
/// that returns `element` declares a different return type (`SkiaControl Tag(..) => new SkiaLabel ..`).
fn helper_cast(text: &str, element: Node, type_name: Option<&str>) -> Option<String> {
    let short = |t: &str| t.split('<').next().unwrap_or(t).rsplit('.').next().unwrap_or(t).trim().to_owned();
    let own = short(type_name?);
    let mut p = element.parent();
    while let Some(x) = p {
        if matches!(x.kind(), "method_declaration" | "local_function_statement") {
            let ret = x.child_by_field_name("returns").or_else(|| x.child_by_field_name("type"))?;
            return (short(&text[ret.byte_range()]) != own).then_some(own);
        }
        p = x.parent();
    }
    None
}

/// The string literal a text control's constructor takes, when `prop` is `Text` and the element
/// is written `new SkiaLabel("..")` (SkiaLabel, SkiaRichLabel and SkiaButton set Text from it).
fn text_argument<'t>(text: &str, src: &SourceTree, handles: &[Node<'t>], key: &str, prop: &str) -> Option<Node<'t>> {
    let i = src.nodes.iter().position(|n| n.key == key)?;
    let name = src.nodes[i].name.as_deref()?;
    if prop != "Text" || !codeset::TEXT_CTORS.contains(&name.rsplit('.').next()?) {
        return None;
    }
    let lit = match call_args(handles[i]).as_slice() {
        [one] => expr_of(*one),
        _ => return None,
    };
    (lit.kind() == "string_literal" && is_literal(lit, text)).then_some(lit)
}

fn refusal_reason(key: &str, r: SourceEditRefusal) -> Refusal {
    match r {
        SourceEditRefusal::NotEditable => Refusal::new("src-not-literal", &[("key", &key)]),
        SourceEditRefusal::InvalidValue => Refusal::new("src-value-misfit", &[("key", &key)]),
        SourceEditRefusal::UnknownNode => Refusal::new("src-key-missing", &[("key", &key)]),
    }
}

/// One [`edit_csharp`] change, as a byte range and its new text.
fn edit_one(text: &str, src: &SourceTree, key: &str, edit: &SourceEdit) -> Result<(usize, usize, String), Refusal> {
    let answer = edit_csharp(text, src, key, edit);
    if let Some(r) = answer.refused {
        return Err(refusal_reason(key, r));
    }
    let [e] = answer.edits.as_slice() else { return Err(Refusal::new("src-not-single", &[])) };
    Ok((byte_of(text, e.range.start), byte_of(text, e.range.end), e.new_text.clone()))
}

/// A Color or Thickness value expression rewritten in place.
fn rewrite(text: &str, mut e: Node, prop: &str, value: &PropValue) -> Result<(usize, usize, String), Refusal> {
    while e.kind() == "parenthesized_expression" {
        e = e.named_child(0).ok_or_else(|| Refusal::new("src-empty-parens", &[]))?;
    }
    let shown: String = text[e.byte_range()].split_whitespace().collect::<Vec<_>>().join(" ");
    let unsupported = |key: &str| Refusal::new(key, &[("prop", &prop), ("code", &shown)]);
    match value {
        PropValue::Color(argb) => {
            // `Color.Parse("#..")`, or any call with one string argument: the string keeps its format.
            if e.kind() == "invocation_expression"
                && let [arg] = call_args(e).as_slice()
                && let s = expr_of(*arg)
                && s.kind() == "string_literal"
            {
                let content = s.named_child(0).filter(|c| c.kind() == "string_literal_content").map_or("", |c| &text[c.byte_range()]);
                let old = parse_color(content).map(|(_, len)| len);
                let upper = !content.chars().any(|c| c.is_ascii_lowercase());
                let hash = if content.starts_with('#') || old.is_none() { "#" } else { "" };
                return Ok((s.start_byte(), s.end_byte(), format!("\"{hash}{}\"", color_hex(*argb, old, upper))));
            }
            // `Colors.X`: another name when one has this value, else `Color.Parse`.
            if is_case_access(e, text)
                && let (Some(q), Some(name)) = (e.child_by_field_name("expression"), e.child_by_field_name("name"))
                && text[q.byte_range()].rsplit('.').next() == Some("Colors")
            {
                let old = &text[name.byte_range()];
                let same = NAMED_COLORS.iter().any(|&(n, v)| n == old && v == *argb);
                return Ok(match NAMED_COLORS.iter().find(|&&(_, v)| v == *argb) {
                    _ if same => (name.start_byte(), name.end_byte(), old.to_string()),
                    Some((n, _)) => (name.start_byte(), name.end_byte(), n.to_string()),
                    None => (e.start_byte(), e.end_byte(), color_parse_call(*argb)),
                });
            }
            Err(unsupported("src-rewrites-colors"))
        }
        PropValue::Thickness(t) => {
            // `new Thickness(..)` / `new(..)`: only the arguments change.
            if is_creation(e) && initializer(e).is_none()
                && let Some(args) = child_of_kind(e, "argument_list")
            {
                return thickness_in(text, args, *t).ok_or_else(|| unsupported("src-rewrites-thickness-numbers"));
            }
            // `Padding = 24`: a uniform number stays one.
            if is_literal(e, text) && (is_number(e) || e.kind() == "prefix_unary_expression") {
                let [l, ..] = *t;
                let uniform = t.iter().all(|v| *v == l);
                return Ok((e.start_byte(), e.end_byte(), if uniform { format!("{l}") } else { format!("new Thickness({})", thickness_args(*t)) }));
            }
            Err(unsupported("src-rewrites-new-thickness"))
        }
        PropValue::Plain(_) => unreachable!(),
    }
}

/// New arguments for an argument list of numbers (`(16, 12)`), or `None` when one is not a number.
fn thickness_in(text: &str, args: Node, t: [f64; 4]) -> Option<(usize, usize, String)> {
    let numbers = items(args).iter().all(|a| {
        let x = expr_of(*a);
        a.kind() == "argument" && a.named_child_count() == 1 && (is_number(x) || (x.kind() == "prefix_unary_expression" && is_literal(x, text)))
    });
    numbers.then(|| (args.start_byte(), args.end_byte(), format!("({})", thickness_args(t))))
}

/// `.WithMargin(..)` / `.WithPadding(..)` in the fluent chain on an element.
fn fluent_setter<'t>(text: &str, creation: Node<'t>, prop: &str) -> Option<Node<'t>> {
    if !is_creation(creation) {
        return None;
    }
    let top = chain_top(creation);
    let mut n = creation;
    while n != top {
        let p = n.parent()?;
        if p.kind() == "invocation_expression"
            && let Some(name) = p.child_by_field_name("function").and_then(|f| f.child_by_field_name("name"))
            && text[name.byte_range()] == format!("With{prop}")
        {
            return Some(p);
        }
        n = p;
    }
    None
}

/// A fluent setter's arguments rewritten: numbers, or one value expression.
fn rewrite_args(text: &str, call: Node, prop: &str, value: &PropValue) -> Result<(usize, usize, String), Refusal> {
    let args = call.child_by_field_name("arguments").ok_or_else(|| Refusal::new("src-no-arguments", &[]))?;
    match (value, call_args(call).as_slice()) {
        (PropValue::Thickness(t), list) if list.len() != 1 || is_number(expr_of(list[0])) => {
            thickness_in(text, args, *t).ok_or_else(|| Refusal::new("src-with-not-numbers", &[("prop", &prop)]))
        }
        (_, [one]) => rewrite(text, expr_of(*one), prop, value),
        _ => Err(Refusal::new("src-with-misfit", &[("prop", &prop)])),
    }
}

/// The byte offset of a line and UTF-16 column.
pub(crate) fn byte_of(text: &str, p: TextPosition) -> usize {
    let lines = Lines::new(text);
    let start = lines.starts[(p.line as usize).min(lines.starts.len() - 1)];
    let mut units = 0;
    for (i, c) in text[start..].char_indices() {
        if units >= p.character {
            return start + i;
        }
        units += c.len_utf16() as u32;
    }
    text.len()
}

/// DrawnUi's `Colors` (SkiaSharp's named colors), in declaration order: the first name wins.
pub(crate) const NAMED_COLORS: &[(&str, u32)] = &[
    ("Empty", 0x00000000), ("Transparent", 0x00FFFFFF), ("AliceBlue", 0xFFF0F8FF), ("AntiqueWhite", 0xFFFAEBD7), ("Aqua", 0xFF00FFFF),
    ("Aquamarine", 0xFF7FFFD4), ("Azure", 0xFFF0FFFF), ("Beige", 0xFFF5F5DC), ("Bisque", 0xFFFFE4C4), ("White", 0xFFFFFFFF),
    ("BlanchedAlmond", 0xFFFFEBCD), ("Blue", 0xFF0000FF), ("BlueViolet", 0xFF8A2BE2), ("Black", 0xFF000000), ("Brown", 0xFFA52A2A),
    ("BurlyWood", 0xFFDEB887), ("CadetBlue", 0xFF5F9EA0), ("Chartreuse", 0xFF7FFF00), ("Chocolate", 0xFFD2691E), ("Coral", 0xFFFF7F50),
    ("CornflowerBlue", 0xFF6495ED), ("Cornsilk", 0xFFFFF8DC), ("Crimson", 0xFFDC143C), ("Cyan", 0xFF00FFFF), ("DarkBlue", 0xFF00008B),
    ("DarkCyan", 0xFF008B8B), ("DarkGoldenrod", 0xFFB8860B), ("Red", 0xFFFF0000), ("DarkRed", 0xFF8B0000), ("DarkGreen", 0xFF006400),
    ("DarkKhaki", 0xFFBDB76B), ("DarkMagenta", 0xFF8B008B), ("DarkOliveGreen", 0xFF556B2F), ("DarkOrange", 0xFFFF8C00), ("DarkOrchid", 0xFF9932CC),
    ("DarkSalmon", 0xFFE9967A), ("DarkSeaGreen", 0xFF8FBC8B), ("DarkSlateBlue", 0xFF483D8B), ("DarkSlateGray", 0xFF2F4F4F), ("DarkTurquoise", 0xFF00CED1),
    ("DarkViolet", 0xFF9400D3), ("DeepPink", 0xFFFF1493), ("DeepSkyBlue", 0xFF00BFFF), ("DimGray", 0xFF696969), ("DodgerBlue", 0xFF1E90FF),
    ("Firebrick", 0xFFB22222), ("FloralWhite", 0xFFFFFAF0), ("ForestGreen", 0xFF228B22), ("Fuchsia", 0xFFFF00FF), ("Gainsboro", 0xFFDCDCDC),
    ("GhostWhite", 0xFFF8F8FF), ("Gold", 0xFFFFD700), ("Goldenrod", 0xFFDAA520), ("Green", 0xFF008000), ("GreenYellow", 0xFFADFF2F),
    ("Honeydew", 0xFFF0FFF0), ("HotPink", 0xFFFF69B4), ("IndianRed", 0xFFCD5C5C), ("Indigo", 0xFF4B0082), ("Ivory", 0xFFFFFFF0),
    ("Khaki", 0xFFF0E68C), ("Lavender", 0xFFE6E6FA), ("LavenderBlush", 0xFFFFF0F5), ("LawnGreen", 0xFF7CFC00), ("LemonChiffon", 0xFFFFFACD),
    ("LightBlue", 0xFFADD8E6), ("LightCoral", 0xFFF08080), ("LightCyan", 0xFFE0FFFF), ("LightGoldenrodYellow", 0xFFFAFAD2), ("LightGray", 0xFFD3D3D3),
    ("LightGreen", 0xFF90EE90), ("LightPink", 0xFFFFB6C1), ("LightSalmon", 0xFFFFA07A), ("LightSeaGreen", 0xFF20B2AA), ("LightSkyBlue", 0xFF87CEFA),
    ("LightSlateGray", 0xFF778899), ("LightSteelBlue", 0xFFB0C4DE), ("LightYellow", 0xFFFFFFE0), ("Lime", 0xFF00FF00), ("Gray", 0xFF808080),
    ("DarkGray", 0xFFA9A9A9), ("Orange", 0xFFFFA500), ("LimeGreen", 0xFF32CD32), ("Linen", 0xFFFAF0E6), ("Magenta", 0xFFFF00FF),
    ("Maroon", 0xFF800000), ("MediumAquamarine", 0xFF66CDAA), ("MediumBlue", 0xFF0000CD), ("MediumOrchid", 0xFFBA55D3), ("MediumPurple", 0xFF9370DB),
    ("MediumSeaGreen", 0xFF3CB371), ("MediumSlateBlue", 0xFF7B68EE), ("MediumSpringGreen", 0xFF00FA9A), ("MediumTurquoise", 0xFF48D1CC), ("MediumVioletRed", 0xFFC71585),
    ("MidnightBlue", 0xFF191970), ("MintCream", 0xFFF5FFFA), ("MistyRose", 0xFFFFE4E1), ("Moccasin", 0xFFFFE4B5), ("NavajoWhite", 0xFFFFDEAD),
    ("Navy", 0xFF000080), ("OldLace", 0xFFFDF5E6), ("Olive", 0xFF808000), ("OliveDrab", 0xFF6B8E23), ("OrangeRed", 0xFFFF4500),
    ("Orchid", 0xFFDA70D6), ("PaleGoldenrod", 0xFFEEE8AA), ("PaleGreen", 0xFF98FB98), ("PaleTurquoise", 0xFFAFEEEE), ("PaleVioletRed", 0xFFDB7093),
    ("PapayaWhip", 0xFFFFEFD5), ("PeachPuff", 0xFFFFDAB9), ("Peru", 0xFFCD853F), ("Pink", 0xFFFFC0CB), ("Plum", 0xFFDDA0DD),
    ("PowderBlue", 0xFFB0E0E6), ("Purple", 0xFF800080), ("RosyBrown", 0xFFBC8F8F), ("RoyalBlue", 0xFF4169E1), ("SaddleBrown", 0xFF8B4513),
    ("Salmon", 0xFFFA8072), ("SandyBrown", 0xFFF4A460), ("SeaGreen", 0xFF2E8B57), ("SeaShell", 0xFFFFF5EE), ("Sienna", 0xFFA0522D),
    ("Silver", 0xFFC0C0C0), ("SkyBlue", 0xFF87CEEB), ("SlateBlue", 0xFF6A5ACD), ("SlateGray", 0xFF708090), ("Snow", 0xFFFFFAFA),
    ("SpringGreen", 0xFF00FF7F), ("SteelBlue", 0xFF4682B4), ("Tan", 0xFFD2B48C), ("Teal", 0xFF008080), ("Thistle", 0xFFD8BFD8),
    ("Tomato", 0xFFFF6347), ("Turquoise", 0xFF40E0D0), ("Violet", 0xFFEE82EE), ("Wheat", 0xFFF5DEB3), ("WhiteSmoke", 0xFFF5F5F5),
    ("Yellow", 0xFFFFFF00), ("YellowGreen", 0xFF9ACD32),
];
