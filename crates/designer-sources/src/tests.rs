use super::*;

const PAGE: &str = r#"public class Page : SkiaLayer
{
    public Page()
    {
        Content = new SkiaLayout
        {
            Type = LayoutType.Column, Spacing = 8,
            Children =
            {
                new SkiaLabel { Text = "Héllo 🌍", FontSize = 18.5f, TextColor = Colors.White }.Assign(out _title),
                new SkiaButton { Text = "Tap", Margin = -4, IsVisible = true }.OnTapped(me => { Show(new SkiaLabel()); }),
                _header,
                new SkiaShape(),
            }
        };
    }
}
"#;

fn byte_of(text: &str, p: TextPosition) -> usize {
    let line_start = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .nth(p.line as usize)
        .unwrap();
    let mut units = 0;
    for (i, c) in text[line_start..].char_indices() {
        if units >= p.character {
            return line_start + i;
        }
        units += c.len_utf16() as u32;
    }
    text.len()
}

fn apply(text: &str, answer: &SourceEditAnswer) -> String {
    assert_eq!(answer.refused, None, "refused");
    let mut spans: Vec<_> =
        answer.edits.iter().map(|e| (byte_of(text, e.range.start), byte_of(text, e.range.end), &e.new_text)).collect();
    spans.sort_by_key(|s| std::cmp::Reverse(s.0));
    let mut out = text.to_string();
    for (a, b, t) in spans {
        out.replace_range(a..b, t);
    }
    out
}

fn node<'a>(t: &'a SourceTree, key: &str) -> &'a SourceNode {
    t.nodes.iter().find(|n| n.key == key).unwrap_or_else(|| panic!("no {key}: {:#?}", keys(t)))
}

fn keys(t: &SourceTree) -> Vec<&str> {
    t.nodes.iter().map(|n| n.key.as_str()).collect()
}

fn edit(text: &str, key: &str, e: SourceEdit) -> String {
    let t = read_csharp("page.cs", text);
    apply(text, &edit_csharp(text, &t, key, &e))
}

fn refusal(text: &str, key: &str, e: SourceEdit) -> Option<SourceEditRefusal> {
    let t = read_csharp("page.cs", text);
    edit_csharp(text, &t, key, &e).refused
}

fn check_invariants(t: &SourceTree) {
    let mut seen = HashSet::new();
    for (i, n) in t.nodes.iter().enumerate() {
        assert!(seen.insert(n.key.clone()), "duplicate key {}", n.key);
        if let Some(p) = n.parent {
            assert!(p < i, "parent after child at {}", n.key);
        }
        if i > 0 {
            let prev = t.nodes[i - 1].range.start;
            assert!((prev.line, prev.character) <= (n.range.start.line, n.range.start.character), "order at {}", n.key);
        }
    }
}

const ROOT: &str = "Page/this/Content/0";
const KIDS: &str = "Page/this/Content/0/Children";

#[test]
fn reads_the_fluent_tree() {
    let t = read_csharp("page.cs", PAGE);
    check_invariants(&t);
    let root = node(&t, ROOT);
    assert_eq!((root.role, root.name.as_deref(), root.parent.map(|p| t.nodes[p].key.as_str())), (SourceRole::Element, Some("SkiaLayout"), Some("Page/this/Content")));
    assert_eq!(node(&t, "Page/this/Content/0/Type/=").value.as_deref(), Some("LayoutType.Column"));
    assert_eq!(node(&t, "Page/this/Content/0/Spacing/=").value.as_deref(), Some("8"));
    let label = node(&t, &format!("{KIDS}/0"));
    assert_eq!(label.name.as_deref(), Some("SkiaLabel"));
    // Range is the creation, not the `.Assign(...)` chain; columns count UTF-16.
    assert_eq!(label.range.start, TextPosition { line: 9, character: 16 });
    let text_lit = node(&t, &format!("{KIDS}/0/Text/="));
    assert_eq!(text_lit.value.as_deref(), Some("\"Héllo 🌍\""));
    assert_eq!(text_lit.range.end.character - text_lit.range.start.character, 10); // 🌍 is 2 units
    assert_eq!(node(&t, &format!("{KIDS}/0/FontSize/=")).value.as_deref(), Some("18.5f"));
    assert_eq!(node(&t, &format!("{KIDS}/0/TextColor/=")).role, SourceRole::Literal);
    assert_eq!(node(&t, &format!("{KIDS}/1/Margin/=")).value.as_deref(), Some("-4"));
    assert_eq!(node(&t, &format!("{KIDS}/1/IsVisible/=")).value.as_deref(), Some("true"));
    assert_eq!(node(&t, &format!("{KIDS}/2")).role, SourceRole::Reference);
    assert_eq!(node(&t, &format!("{KIDS}/3")).name.as_deref(), Some("SkiaShape"));
    // The label created inside the OnTapped lambda is top-level.
    assert_eq!(node(&t, "Page.ctor/1").parent, None);
    assert_eq!(t.nodes[node(&t, KIDS).parent.unwrap()].key, ROOT);
}

const CELL: &str = "public class Cell : SkiaLayout
{
    public Cell()
    {
        _count = 3;
        HorizontalOptions = LayoutOptions.Fill;
        this.Padding = new Thickness(12, 8);
        AddSubView(new SkiaLabel { Text = \"a\" }.Assign(out _a));
        var tmp = new SkiaShape();
        Spacing = 4;
        this.Children.Add(new SkiaLabel { Text = \"b\" });
        Children.Add(_header);
        Tapped += (s, e) => Show(new SkiaLabel());
    }
    public Cell(int x) { Show(new SkiaLabel()); }
}
";

#[test]
fn constructor_builds_a_this_element() {
    let t = read_csharp("cell.cs", CELL);
    check_invariants(&t);
    let this = node(&t, "Cell/this");
    assert_eq!((this.role, this.name.as_deref(), this.parent), (SourceRole::Element, Some("Cell"), None));
    let attrs: Vec<_> = t.nodes.iter().filter(|n| n.parent.is_some_and(|p| t.nodes[p].key == "Cell/this")).map(|n| n.key.as_str()).collect();
    assert_eq!(attrs, ["Cell/this/HorizontalOptions", "Cell/this/Padding", "Cell/this/Children", "Cell/this/Spacing"]);
    assert_eq!(node(&t, "Cell/this/HorizontalOptions/=").value.as_deref(), Some("LayoutOptions.Fill"));
    assert_eq!(node(&t, "Cell/this/Padding/0").name.as_deref(), Some("Thickness"));
    assert_eq!(node(&t, "Cell/this/Children/0/Text/=").value.as_deref(), Some("\"a\""));
    assert_eq!(node(&t, "Cell/this/Children/1/Text/=").value.as_deref(), Some("\"b\""));
    assert_eq!(node(&t, "Cell/this/Children/2").role, SourceRole::Reference);
    // Top-level elements keep the keys they had before `this` existed.
    assert_eq!(node(&t, "Cell.ctor/2").name.as_deref(), Some("SkiaShape"));
    assert_eq!(node(&t, "Cell.ctor/4").parent, None);
    assert_eq!(node(&t, "Cell.ctor#2/0").name.as_deref(), Some("SkiaLabel"));
    assert!(!t.nodes.iter().any(|n| n.key == "Cell#2/this" || n.key.ends_with("/_count")));

    let out = edit(CELL, "Cell/this/Spacing", SourceEdit::SetValue { value: SourceEditValue::Number { value: 6.0 } });
    assert!(out.contains("        Spacing = 6;\n"), "{out}");
    let out = edit(CELL, "Cell/this/HorizontalOptions", SourceEdit::Remove);
    assert!(out.contains("        _count = 3;\n        this.Padding"), "{out}");
    let out = edit(CELL, "Cell/this", SourceEdit::AddProperty { name: "Opacity".into(), value: SourceEditValue::Number { value: 0.5 } });
    assert!(out.contains("        Spacing = 4;\n        Opacity = 0.5;\n        this.Children"), "{out}");
    assert_eq!(refusal(CELL, "Cell/this/Spacing", SourceEdit::AddProperty { name: "X".into(), value: SourceEditValue::Number { value: 1.0 } }), Some(SourceEditRefusal::NotEditable));
    assert_eq!(refusal(CELL, "Cell/this", SourceEdit::AddProperty { name: "Spacing".into(), value: SourceEditValue::Number { value: 1.0 } }), Some(SourceEditRefusal::InvalidValue));
    assert_eq!(refusal(CELL, "Cell/this/Children", SourceEdit::Remove), Some(SourceEditRefusal::NotEditable));
    assert_eq!(refusal(CELL, "Cell/this/Children/0", SourceEdit::Remove), Some(SourceEditRefusal::NotEditable));
    assert_eq!(refusal(CELL, "Cell/this", SourceEdit::Remove), Some(SourceEditRefusal::NotEditable));

    // Only `Add` calls: the new assignment goes before the first one.
    let src = "class C : SkiaLayer {\n    public C()\n    {\n        AddSubView(new SkiaLabel());\n    }\n}";
    let out = edit(src, "C/this", SourceEdit::AddProperty { name: "Tag".into(), value: SourceEditValue::String { value: "t".into() } });
    assert!(out.contains("    {\n        Tag = \"t\";\n        AddSubView(new SkiaLabel());"), "{out}");
}

#[test]
fn reads_collections_references_and_implicit_new() {
    let src = r##"class A {
    List<SkiaControl> _list = new();
    void M() {
        var x = new SkiaLayout { Children = new List<SkiaControl> { new SkiaLabel() }, Items = [ new SkiaLabel { Text = "a" }, ..more ], TextColor = Color.Parse("#fff"), Tag = null };
    }
    void M(int overload) { var y = new SkiaLabel(); }
}"##;
    let t = read_csharp("a.cs", src);
    check_invariants(&t);
    assert_eq!(node(&t, "A._list/0").name.as_deref(), Some("List<SkiaControl>"));
    assert_eq!(node(&t, "A.M/0/Children/0").name.as_deref(), Some("SkiaLabel"));
    assert_eq!(node(&t, "A.M/0/Items/0/Text/=").value.as_deref(), Some("\"a\""));
    assert_eq!(node(&t, "A.M/0/Items/1").role, SourceRole::Reference);
    assert_eq!(node(&t, "A.M/0/TextColor/=").role, SourceRole::Reference);
    assert_eq!(node(&t, "A.M/0/Tag/=").role, SourceRole::Reference);
    assert_eq!(node(&t, "A.M#2/0").name.as_deref(), Some("SkiaLabel"));
}

#[test]
fn set_value_respells_each_kind() {
    let k = |s: &str| format!("{KIDS}/{s}");
    let s = |v: SourceEditValue| SourceEdit::SetValue { value: v };
    let out = edit(PAGE, &k("0/Text"), s(SourceEditValue::String { value: "Ça \"va\"\n😀".into() }));
    assert!(out.contains(r#"Text = "Ça \"va\"\n😀", FontSize"#), "{out}");
    let out = edit(PAGE, &k("0/FontSize/="), s(SourceEditValue::Number { value: 20.0 }));
    assert!(out.contains("FontSize = 20f,"), "{out}");
    let out = edit(PAGE, "Page/this/Content/0/Spacing", s(SourceEditValue::Number { value: 0.5 }));
    assert!(out.contains("Spacing = 0.5,"), "{out}");
    let out = edit(PAGE, &k("1/Margin"), s(SourceEditValue::Number { value: 3.0 }));
    assert!(out.contains("Margin = 3,"), "{out}");
    let out = edit(PAGE, &k("1/IsVisible"), s(SourceEditValue::Boolean { value: false }));
    assert!(out.contains("IsVisible = false }"), "{out}");
    let out = edit(PAGE, "Page/this/Content/0/Type", s(SourceEditValue::Case { name: "Row".into() }));
    assert!(out.contains("Type = LayoutType.Row,"), "{out}");
    assert_eq!(out.len(), PAGE.len() - 3);
}

#[test]
fn set_value_refusals() {
    let s = |v: SourceEditValue| SourceEdit::SetValue { value: v };
    assert_eq!(refusal(PAGE, "nope", s(SourceEditValue::Boolean { value: true })), Some(SourceEditRefusal::UnknownNode));
    assert_eq!(
        refusal(PAGE, "Page/this/Content/0/Spacing", s(SourceEditValue::String { value: "x".into() })),
        Some(SourceEditRefusal::InvalidValue)
    );
    assert_eq!(
        refusal(PAGE, &format!("{KIDS}/2"), s(SourceEditValue::Number { value: 1.0 })),
        Some(SourceEditRefusal::NotEditable)
    );
    assert_eq!(refusal(PAGE, ROOT, s(SourceEditValue::Number { value: 1.0 })), Some(SourceEditRefusal::NotEditable));
    let src = "class A { void M() { var x = new SkiaLabel { TextColor = Color.Parse(\"#fff\") }; } }";
    assert_eq!(
        refusal(src, "A.M/0/TextColor", s(SourceEditValue::String { value: "x".into() })),
        Some(SourceEditRefusal::NotEditable)
    );
    assert_eq!(
        refusal(PAGE, ROOT, SourceEdit::InsertElement { type_name: "SkiaLabel".into(), index: 0 }),
        Some(SourceEditRefusal::NotEditable)
    );
}

#[test]
fn add_property_matches_the_style() {
    let add = |name: &str, v: SourceEditValue| SourceEdit::AddProperty { name: name.into(), value: v };
    // One line, no trailing comma.
    let out = edit(PAGE, &format!("{KIDS}/0"), add("Opacity", SourceEditValue::Number { value: 0.5 }));
    assert!(out.contains("TextColor = Colors.White, Opacity = 0.5 }.Assign"), "{out}");
    // Multi-line without trailing comma: new line, siblings' indentation.
    let out = edit(PAGE, ROOT, add("Padding", SourceEditValue::Number { value: 4.0 }));
    assert!(out.contains("            },\n            Padding = 4\n        };"), "{out}");
    // No initializer.
    let out = edit(PAGE, &format!("{KIDS}/3"), add("Text", SourceEditValue::String { value: "é".into() }));
    assert!(out.contains("new SkiaShape() { Text = \"é\" },"), "{out}");
    // Multi-line with trailing comma, CRLF.
    let src = "class A { void M() {\r\n    var x = new SkiaLabel\r\n    {\r\n\tText = \"a\",\r\n    };\r\n} }";
    let out = edit(src, "A.M/0", add("FontSize", SourceEditValue::Number { value: 12.0 }));
    assert!(out.contains("\tText = \"a\",\r\n\tFontSize = 12,\r\n    };"), "{out:?}");
    // Empty initializer, enum case.
    let src = "class A { void M() { var x = new SkiaLayout { }; } }";
    let out = edit(src, "A.M/0", add("Type", SourceEditValue::Case { name: "Row".into() }));
    assert!(out.contains("new SkiaLayout { Type = Row };"), "{out}");
    // Taken name.
    assert_eq!(refusal(PAGE, ROOT, add("Spacing", SourceEditValue::Number { value: 1.0 })), Some(SourceEditRefusal::InvalidValue));
}

#[test]
fn remove_attributes_and_elements() {
    let out = edit(PAGE, "Page/this/Content/0/Type", SourceEdit::Remove);
    assert!(out.contains("            Spacing = 8,\n"), "{out}");
    let out = edit(PAGE, &format!("{KIDS}/0/TextColor"), SourceEdit::Remove);
    assert!(out.contains("FontSize = 18.5f }.Assign"), "{out}");
    let out = edit(PAGE, &format!("{KIDS}/0"), SourceEdit::Remove);
    assert!(out.contains("            {\n                new SkiaButton"), "{out}");
    assert!(!out.contains("SkiaLabel {"), "{out}");
    // Last item with a trailing comma keeps the previous comma.
    let out = edit(PAGE, &format!("{KIDS}/3"), SourceEdit::Remove);
    assert!(out.contains("                _header,\n            }"), "{out}");
    // Only item; collection expression.
    let src = "class A { void M() { var x = new SkiaLayout { Children = [ new SkiaLabel { Text = \"a\" } ] }; } }";
    let out = edit(src, "A.M/0/Children/0", SourceEdit::Remove);
    assert!(out.contains("Children = [] }"), "{out}");
    let out = edit(src, "A.M/0/Children", SourceEdit::Remove);
    assert!(out.contains("new SkiaLayout { };"), "{out}");
    // Single element value: not a collection item.
    assert_eq!(refusal(src, "A.M/0/Children/0/Text/=", SourceEdit::Remove), Some(SourceEditRefusal::NotEditable));
    assert_eq!(refusal(PAGE, ROOT, SourceEdit::Remove), Some(SourceEditRefusal::NotEditable));
}

#[test]
fn move_keeps_items_verbatim() {
    let t = read_csharp("page.cs", PAGE);
    let out = apply(PAGE, &edit_csharp(PAGE, &t, &format!("{KIDS}/0"), &SourceEdit::Move { to: 2 }));
    let lines: Vec<&str> = out.lines().skip(9).take(4).map(str::trim).collect();
    assert!(lines[0].starts_with("new SkiaButton"), "{out}");
    assert_eq!(lines[1], "_header,");
    assert!(lines[2].starts_with("new SkiaLabel { Text = \"Héllo 🌍\"") && lines[2].ends_with(".Assign(out _title),"));
    assert_eq!(lines[3], "new SkiaShape(),");
    assert_eq!(out.len(), PAGE.len());
    let src = "class A { void M() { var x = new SkiaLayout { Children = [ new A(), new B() { X = 1 } ] }; } }";
    let out = edit(src, "A.M/0/Children/1", SourceEdit::Move { to: 0 });
    assert!(out.contains("[ new B() { X = 1 }, new A() ]"), "{out}");
    assert_eq!(refusal(PAGE, &format!("{KIDS}/0"), SourceEdit::Move { to: 9 }), Some(SourceEditRefusal::InvalidValue));
}

#[test]
fn round_trip_changes_only_the_value() {
    let before = read_csharp("page.cs", PAGE);
    let key = format!("{KIDS}/0/Text/=");
    let out = apply(PAGE, &edit_csharp(PAGE, &before, &key, &SourceEdit::SetValue { value: SourceEditValue::String { value: "Привет 👋🏽".into() } }));
    let after = read_csharp("page.cs", &out);
    assert_eq!(keys(&before), keys(&after));
    let edited_line = node(&before, &key).range.start.line;
    for (a, b) in before.nodes.iter().zip(&after.nodes) {
        assert_eq!((a.role, &a.name, a.parent), (b.role, &b.name, b.parent), "{}", a.key);
        if a.key == key {
            assert_eq!(b.value.as_deref(), Some("\"Привет 👋🏽\""));
        } else {
            assert_eq!(a.value, b.value, "{}", a.key);
        }
        if a.range.start.line != edited_line || a.range.start.character <= node(&before, &key).range.start.character {
            assert_eq!(a.range.start, b.range.start, "{}", a.key);
        }
    }
}

fn error_nodes(n: Node, text: &str, out: &mut Vec<String>) {
    if n.is_error() || n.is_missing() {
        let mut ctx = vec![];
        let mut p = n.parent();
        while let Some(x) = p {
            ctx.push(x.kind());
            p = x.parent();
        }
        let snippet: String = text[n.byte_range()].chars().take(40).collect();
        out.push(format!(
            "{} at {}:{} in {:?}: {snippet:?}",
            if n.is_missing() { "MISSING" } else { "ERROR" },
            n.start_position().row + 1,
            n.start_position().column + 1,
            &ctx[..ctx.len().min(4)]
        ));
    }
    let mut c = n.walk();
    for k in n.children(&mut c) {
        error_nodes(k, text, out);
    }
}

#[test]
fn newer_syntax_parses_cleanly() {
    let src = r#"
class A {
    int[] Xs = [1, 2, ..Other];
    List<int> L = new();
    string R = """
        raw "quoted" text
        """;
    string I = $"""{1 + 2} raw""";
    void M(object o) {
        if (o is SkiaLabel { Text: "a" or "b" } label && o is not null) { }
        var f = (int x, int y) => x + y;
        var g = static async () => await Task.Delay(1);
        var s = o switch { int n when n > 0 => "pos", _ => "other" };
        var p = new SkiaButton { Text = "x" }.OnTapped(me => { me.Text = "y"; });
        (var a, var b) = (1, 2);
        var r = new Point { X = 1 } with { X = 2 };
        Span<int> sp = [];
    }
    record Rec(int A, string B);
}
file class F(int x) { int X => x; }
"#;
    let mut errors = vec![];
    error_nodes(parse(src).root_node(), src, &mut errors);
    assert!(errors.is_empty(), "{errors:#?}");
}

/// The DrawnUi repository: DRAWNUI_REPO, or a DrawnUi folder next to this repository.
fn drawnui_repo() -> std::path::PathBuf {
    std::env::var_os("DRAWNUI_REPO").map(Into::into).unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(r"..\..\..\DrawnUi"))
}

#[test]
fn reads_hello_wpf_pages() {
    let pages = drawnui_repo().join(r"src\Wpf\Samples\HelloWpf\Pages");
    let Ok(dir) = std::fs::read_dir(&pages) else {
        eprintln!("skipped: {} not found", pages.display());
        return;
    };
    let constructs = [
        ("collection_expression", "collection expression"),
        ("implicit_object_creation_expression", "target-typed new()"),
        ("raw_string_literal", "raw string"),
        ("is_pattern_expression", "is pattern"),
        ("lambda_expression", "lambda"),
    ];
    let mut total = [0usize; 4];
    let mut this_pages = vec![];
    let mut sample = String::new();
    let mut calls = [0usize; 2];
    let mut found = [0usize; 5];
    let mut files: Vec<_> = dir.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "cs")).collect();
    files.sort();
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        let ts = parse(&text);
        let mut errors = vec![];
        error_nodes(ts.root_node(), &text, &mut errors);
        fn count(n: Node, kinds: &[(&str, &str)], found: &mut [usize]) {
            if let Some(i) = kinds.iter().position(|k| k.0 == n.kind()) {
                found[i] += 1;
            }
            let mut c = n.walk();
            for k in n.children(&mut c) {
                count(k, kinds, found);
            }
        }
        count(ts.root_node(), &constructs, &mut found);
        let t = read_csharp(path.to_str().unwrap(), &text);
        check_invariants(&t);
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        for (i, n) in t.nodes.iter().enumerate().filter(|(_, n)| n.key.ends_with("/this")) {
            let under = |mut j: usize| loop {
                match t.nodes[j].parent {
                    Some(p) if p == i => break true,
                    Some(p) => j = p,
                    None => break false,
                }
            };
            let kids = (i + 1..t.nodes.len()).filter(|&j| t.nodes[j].role == SourceRole::Element && under(j)).count();
            if kids > 0 {
                this_pages.push(format!("{} ({kids} elements)", n.key));
            }
        }
        let links = resolve_helpers(&t, &text);
        calls[0] += t.nodes.iter().filter(|n| n.role == SourceRole::Reference && n.name.is_some()).count();
        calls[1] += links.len();
        if file == "LayoutsPage.cs" {
            let i = t.nodes.iter().position(|n| n.key == "LayoutsPage/this").unwrap();
            sample = element_tree(&t, &links, i, 0, 4);
        }
        let by = |r: SourceRole| t.nodes.iter().filter(|n| n.role == r).count();
        let c = [by(SourceRole::Element), by(SourceRole::Attribute), by(SourceRole::Literal), errors.len()];
        for i in 0..4 {
            total[i] += c[i];
        }
        println!(
            "{:<32} elements {:>4} attributes {:>4} literals {:>4} errors {}",
            path.file_name().unwrap().to_string_lossy(),
            c[0],
            c[1],
            c[2],
            c[3]
        );
        for e in errors {
            println!("    {e}");
        }
        // Literals take a value of their kind, and the edited file re-reads with the same keys
        // (every 5th literal: each edit parses ~7 ms per 35 KB in release, 5x that in debug).
        for n in t.nodes.iter().filter(|n| n.role == SourceRole::Literal).step_by(5) {
            let v = n.value.as_deref().unwrap();
            let value = match v.chars().next().unwrap() {
                '"' | '@' | '$' => SourceEditValue::String { value: "é\"x".into() },
                '\'' => SourceEditValue::String { value: "é".into() },
                't' | 'f' if v == "true" || v == "false" => SourceEditValue::Boolean { value: v == "false" },
                c if c.is_ascii_digit() || c == '-' || c == '.' => SourceEditValue::Number { value: 2.5 },
                _ => SourceEditValue::Case { name: "Other".into() },
            };
            let a = edit_csharp(&text, &t, &n.key, &SourceEdit::SetValue { value });
            if a.refused == Some(SourceEditRefusal::InvalidValue) && v.contains(['u', 'U', 'l', 'L']) {
                continue; // 2.5 into an integer-suffixed literal
            }
            let re = read_csharp("x", &apply(&text, &a));
            assert_eq!(keys(&t), keys(&re), "{}", n.key);
        }
    }
    println!("this elements with child elements: {}\n  {}", this_pages.len(), this_pages.join("\n  "));
    println!("helper calls: {} resolved of {} call items", calls[1], calls[0]);
    println!("LayoutsPage/this, elements and calls to depth 4 (call -> returned element):\n{sample}");
    println!("TOTAL elements {} attributes {} literals {} errors {}", total[0], total[1], total[2], total[3]);
    for (i, (_, name)) in constructs.iter().enumerate() {
        println!("  {name}: {} occurrences", found[i]);
    }
}

/// Element types and helper calls under node `i`, attributes skipped, to `depth` levels.
fn element_tree(t: &SourceTree, links: &[HelperLink], i: usize, level: usize, depth: usize) -> String {
    let n = &t.nodes[i];
    let name = n.name.as_deref().unwrap_or("?");
    let label = match n.role {
        SourceRole::Element => name.to_string(),
        _ => match links.iter().find(|l| l.call == n.key) {
            Some(l) => format!("{name}(..) -> {}", t.nodes.iter().find(|x| x.key == l.returns).unwrap().name.as_deref().unwrap_or("?")),
            None => format!("{name}(..)"),
        },
    };
    let mut out = format!("{}{label}\n", "  ".repeat(level));
    if level == depth {
        return out;
    }
    let shown = |j: usize| {
        t.nodes[j].role == SourceRole::Element || (t.nodes[j].role == SourceRole::Reference && t.nodes[j].name.is_some())
    };
    let element_kids = (i + 1..t.nodes.len()).filter(|&j| {
        shown(j) && t.nodes[j].parent.is_some_and(|a| a == i || (t.nodes[a].role == SourceRole::Attribute && t.nodes[a].parent == Some(i)))
    });
    for j in element_kids.collect::<Vec<_>>() {
        out += &element_tree(t, links, j, level + 1, depth);
    }
    out
}

const HELPERS: &str = r#"class Page : SkiaLayer
{
    public Page()
    {
        Children = new List<SkiaControl>
        {
            Card("Title", new SkiaLabel { Text = "body" }),
            Heading("Hi").Assign(out _h),
            Helpers.Badge(Row(new SkiaShape())),
            this.Card(content: new SkiaImage(), title: "named"),
            External.Thing(new SkiaLabel()),
            Ambiguous(1),
            Box("b"),
            GridCell("g"),
            At(new SkiaLabel { Text = "direct" }, 1),
            new SkiaControl[] { x }.Concat(more),
        };
        Show(new SkiaLabel());
    }
    SkiaControl Card(string title, SkiaControl content)
    {
        var unused = 1;
        return new SkiaShape { Children = { new SkiaLabel { Text = title }, content } }.Assign(out _card);
    }
    static SkiaLabel Heading(string text, double size = 20) => new() { Text = text, FontSize = size };
    SkiaControl Row(SkiaControl a) { if (a == null) return null; return new SkiaRow { Children = { a } }; }
    SkiaControl Ambiguous(int a) => new SkiaLabel();
    SkiaControl Ambiguous(long a) => new SkiaLabel();
    static SkiaControl Box(string text) { var box = new SkiaShape { Children = { new SkiaLabel { Text = text } } }; box.Margin = 2; return box; }
    static SkiaControl GridCell(string caption) => At((SkiaControl)new SkiaShape { Tag = caption }, 0);
    static SkiaControl At(SkiaControl control, int column) { Grid.SetColumn(control, column); return control; }
}
static class Helpers { public static SkiaControl Badge(SkiaControl inner) => new SkiaShape { Content = inner }; }
"#;

#[test]
fn helper_calls_hold_their_argument_elements() {
    let t = read_csharp("h.cs", HELPERS);
    check_invariants(&t);
    let k = |s: &str| format!("Page/this/Children/{s}");
    let name = |key: &str| node(&t, key).name.clone();
    assert_eq!(name(&k("0")).as_deref(), Some("Card"));
    assert_eq!(name(&k("0/arg1")).as_deref(), Some("SkiaLabel"));
    assert_eq!(node(&t, &k("0/arg1/Text/=")).value.as_deref(), Some("\"body\""));
    assert_eq!(name(&k("1")).as_deref(), Some("Heading"));
    assert_eq!(name(&k("2")).as_deref(), Some("Helpers.Badge"));
    assert_eq!(name(&k("2/arg0")).as_deref(), Some("Row"));
    assert_eq!(name(&k("2/arg0/arg0")).as_deref(), Some("SkiaShape"));
    assert_eq!(name(&k("3")).as_deref(), Some("this.Card"));
    assert_eq!(name(&k("3/arg0")).as_deref(), Some("SkiaImage"));
    assert_eq!(name(&k("4/arg0")).as_deref(), Some("SkiaLabel"));
    // Absorbed argument elements keep their top-level slots: the label after them stays /5.
    assert_eq!(node(&t, "Page.ctor/5").parent, None);

    let links = resolve_helpers(&t, HELPERS);
    let link = |s: &str| links.iter().find(|l| l.call == k(s));
    let flows = |s: &str| link(s).unwrap().params.iter().map(|p| (p.arg, p.param.as_str(), p.used_at.as_str())).collect::<Vec<_>>();
    assert_eq!(link("0").unwrap().returns, "Page.Card/0");
    assert_eq!(flows("0"), [(0, "title", "Page.Card/0/Children/0/Text/="), (1, "content", "Page.Card/0/Children/1")]);
    assert_eq!(link("1").unwrap().returns, "Page.Heading/0");
    assert_eq!(name("Page.Heading/0").as_deref(), Some("SkiaLabel")); // target-typed new() in an arrow body
    assert_eq!(flows("1"), [(0, "text", "Page.Heading/0/Text/=")]);
    // A literal argument: its own node, and the attribute of the returned element it feeds.
    assert_eq!(node(&t, &k("1/arg0")).value.as_deref(), Some("\"Hi\""));
    let f = &link("1").unwrap().params[0];
    assert_eq!((f.attribute.as_deref(), f.arg_key.clone()), (Some("Page.Heading/0/Text"), Some(k("1/arg0"))));
    let out = edit(HELPERS, &k("1/arg0"), SourceEdit::SetValue { value: SourceEditValue::String { value: "Hello".into() } });
    assert!(out.contains("Heading(\"Hello\").Assign(out _h),"), "{out}");
    assert_eq!(link("0").unwrap().params[1].attribute, None); // a child, not an attribute value
    assert_eq!(link("0").unwrap().params[1].arg_key, Some(k("0/arg1")));
    assert_eq!(node(&t, &k("6/arg0")).role, SourceRole::Literal);
    assert_eq!(link("2").unwrap().returns, "Helpers.Badge/0");
    assert_eq!(flows("2"), [(0, "inner", "Helpers.Badge/0/Content/=")]);
    assert_eq!(flows("3"), [(0, "content", "Page.Card/0/Children/1"), (1, "title", "Page.Card/0/Children/0/Text/=")]);
    // Two returns, outside the file, ambiguous overloads.
    assert!(link("2/arg0").is_none() && link("4").is_none() && link("5").is_none());
    // A local, a call into another helper returning its parameter, a parameter returned as is.
    assert_eq!(link("6").unwrap().returns, "Page.Box/0");
    assert_eq!(flows("6"), [(0, "text", "Page.Box/0/Children/0/Text/=")]);
    assert_eq!(link("7").unwrap().returns, "Page.GridCell/0");
    assert_eq!(flows("7"), [(0, "caption", "Page.GridCell/0/Tag/=")]);
    assert_eq!(link("8").unwrap().returns, k("8/arg0"));
    assert_eq!(flows("8"), []);
    assert_eq!(name(&k("9")).as_deref(), Some("Concat"));
    assert_eq!(links.len(), 7);
}


const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), r"\..\..\fixtures\hellowpf");

fn hello_sources() -> Option<Vec<(String, String)>> {
    fn collect(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if p.is_dir() && name != "obj" && name != "bin" {
                collect(&p, out);
            } else if name.ends_with(".cs") {
                out.push((p.to_string_lossy().to_string(), std::fs::read_to_string(&p).unwrap()));
            }
        }
    }
    let mut out = vec![];
    collect(&drawnui_repo().join(r"src\Wpf\Samples\HelloWpf"), &mut out);
    (!out.is_empty()).then_some(out)
}

#[test]
fn matches_hello_wpf_runtime_trees() {
    let Some(files) = hello_sources() else {
        eprintln!("skipped: HelloWpf not found (set DRAWNUI_REPO)");
        return;
    };
    let mut all_stops: std::collections::BTreeMap<String, usize> = Default::default();
    for name in ["root", "layouts", "text", "shapes", "cells"] {
        let Ok(json) = std::fs::read_to_string(format!(r"{FIXTURES}\{name}.json")) else {
            eprintln!("skipped fixture {name}");
            continue;
        };
        let rt: designer_protocol::Tree = serde_json::from_str(&json).unwrap();
        let (matches, stops) = matcher::run(&files, &rt);
        for s in stops {
            *all_stops.entry(s).or_default() += 1;
        }
        let by_id: HashMap<u64, &matcher::RuntimeMatch> = matches.iter().map(|m| (m.node, m)).collect();
        let short = |t: &str| t.rsplit(['.', '+']).next().unwrap().to_string();
        // Per page: the subtree under each anchor (`Class/this`) that has no matched ancestor anchor.
        let mut anchor_of: HashMap<u64, u64> = HashMap::new();
        let mut unmatched: std::collections::BTreeMap<String, usize> = Default::default();
        for n in &rt.nodes {
            let parent_anchor = n.parent.and_then(|p| anchor_of.get(&p).copied());
            let anchor = match by_id.get(&n.id) {
                Some(m) if m.key.ends_with("/this") && parent_anchor.is_none() => Some(n.id),
                _ => parent_anchor,
            };
            if let Some(a) = anchor {
                anchor_of.insert(n.id, a);
            }
            if !by_id.contains_key(&n.id) && !n.internal {
                let parent_matched = n.parent.is_some_and(|p| by_id.contains_key(&p));
                let cat = match (anchor.is_some(), parent_matched) {
                    (false, _) => "outside pages".to_string(),
                    (true, true) => format!("in page, parent matched: {}", short(&n.type_name)),
                    (true, false) => format!("in page, parent unmatched: {}", short(&n.type_name)),
                };
                *unmatched.entry(cat).or_default() += 1;
            }
        }
        let mut pages: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
        for n in &rt.nodes {
            if let Some(a) = anchor_of.get(&n.id) {
                let page = short(&rt.nodes.iter().find(|x| x.id == *a).unwrap().type_name);
                let e = pages.entry(page).or_default();
                e.1 += usize::from(!n.internal);
                if by_id.contains_key(&n.id) {
                    e.0 += 1;
                }
            }
        }
        let internal = rt.nodes.iter().filter(|n| n.internal).count();
        println!("{name}: matched {} of {} runtime nodes ({} internal skipped)", matches.len(), rt.nodes.len() - internal, internal);
        for (p, (m, t)) in &pages {
            println!("    page {p}: {m}/{t}");
        }
        let mut u: Vec<_> = unmatched.into_iter().collect();
        u.sort_by_key(|x| std::cmp::Reverse(x.1));
        for (c, k) in u.iter().take(8) {
            println!("    unmatched {k:>4}  {c}");
        }
        let kids_of = |id: u64| rt.nodes.iter().filter(move |n| n.parent == Some(id));
        let of_type = |t: &'static str| rt.nodes.iter().filter(move |n| n.type_name.ends_with(t)).map(|n| by_id.get(&n.id).map(|m| m.key.clone())).collect::<Vec<_>>();
        if name == "root" {
            // One card per catalog sample, in the masonry the root page lays them out in.
            let masonry = rt.nodes.iter().find(|n| n.type_name.ends_with(".SkiaMasonry")).unwrap();
            let cards: Vec<_> = kids_of(masonry.id).map(|n| by_id[&n.id]).collect();
            assert_eq!(cards.len(), 21);
            assert!(cards.iter().all(|m| m.key == "RootPage.BuildCard/0" && m.via.iter().any(|v| v.ends_with("Children/=") )), "{:?}", cards[0]);
        }
        if name == "layouts" {
            // 4 buttons from `new[] { 0, 2, 3, 4 }.Select(..)`, then 3 `.Append(..)` buttons.
            let wrap = rt.nodes.iter().find(|w| w.type_name.ends_with(".SkiaWrap") && kids_of(w.id).count() == 7).unwrap();
            let keys: Vec<_> = kids_of(wrap.id).map(|n| by_id[&n.id].key.clone()).collect();
            assert!(keys[..4].iter().all(|k| *k == keys[0]), "{keys:?}");
            assert_eq!(keys[4..].iter().collect::<HashSet<_>>().len(), 3, "{keys:?}");
            assert!(!keys[4..].contains(&keys[0]));
            // Recycled cells map to the `new ChipCell()` of their layout's ItemTemplate (4 layouts).
            let chips = of_type(".ChipCell");
            assert!(chips.iter().all(|k| k.as_ref().is_some_and(|k| k.starts_with("LayoutsPage.ctor/"))), "{chips:?}");
            assert!(chips.iter().collect::<HashSet<_>>().len() <= 4);
            // Field anchors agree with the walk: every `field` node maps to its `.Assign(out _field)`.
            for n in rt.nodes.iter().filter(|n| n.field.is_some()) {
                assert!(by_id.contains_key(&n.id), "{:?} unmatched", n.field);
            }
        }
        if name == "text" {
            // Card(string title, params SkiaControl[] content): content args land in the card's stack,
            // matched to the argument at the call site, reached through the call.
            let args: Vec<_> = matches.iter().filter(|m| m.key.rsplit('/').next().unwrap().starts_with("arg")).collect();
            assert!(args.len() >= 5, "{}", args.len());
            assert!(args.iter().all(|m| m.via.last().is_some_and(|v| m.key.starts_with(&format!("{v}/arg")))), "{:?}", args[0]);
        }
        if name == "cells" {
            let cells = of_type(".ContactCell");
            assert!(cells.iter().all(|k| k.is_some()) && cells.iter().collect::<HashSet<_>>().len() == 1, "{cells:?}");
        }
        if name == "layouts" {
            let page = rt.nodes.iter().find(|n| n.type_name.ends_with(".LayoutsPage")).unwrap();
            let scroll = rt.nodes.iter().find(|n| n.parent == Some(page.id)).unwrap();
            let stack = rt.nodes.iter().find(|n| n.parent == Some(scroll.id)).unwrap();
            let first = rt.nodes.iter().find(|n| n.parent == Some(stack.id)).unwrap();
            let m = by_id[&first.id];
            println!("    LayoutsPage first heading -> {} via {:?}", m.key, m.via);
            assert_eq!(m.key, "LayoutsPage.Heading/0");
            assert_eq!(m.via.len(), 1);
            assert_eq!(by_id[&page.id].key, "LayoutsPage/this");
        }
    }
    let mut s: Vec<_> = all_stops.into_iter().collect();
    s.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!("stopped at:");
    for (t, k) in s.iter().take(15) {
        println!("    {k:>4}  {t}");
    }
}

const COLORS: &str = r##"namespace Demo;
public class Page : SkiaLayer
{
    public Page()
    {
        Children = new List<SkiaControl>
        {
            new SkiaLabel { TextColor = Color.Parse("#fff"), BackgroundColor = Colors.White, Margin = new Thickness(0, 16, 0, 0), Padding = new(4) },
            new SkiaLabel { TextColor = Muted, StrokeColor = Color.Parse("#80FF0000") }.WithMargin(8, 4),
            new SkiaLabel(),
            Box(Color.Parse("#00ff00")),
        };
    }
    static SkiaShape Box(Color color) => new SkiaShape { BackgroundColor = color };
}
"##;

fn colors_tree() -> designer_protocol::Tree {
    let node = |id, parent, t: &str| designer_protocol::Node { id, parent, type_name: t.into(), ..Default::default() };
    designer_protocol::Tree {
        nodes: vec![
            node(1, None, "Demo.Page"),
            node(2, Some(1), "DrawnUi.Draw.SkiaLabel"),
            node(3, Some(1), "DrawnUi.Draw.SkiaLabel"),
            node(4, Some(1), "DrawnUi.Draw.SkiaLabel"),
            node(5, Some(1), "DrawnUi.Draw.SkiaShape"),
        ],
        ..Default::default()
    }
}

/// Sets a Color / Thickness property from the agent's text and checks the file changed only there.
fn set_typed(node: u64, prop: &str, type_name: &str, value: &str) -> Result<String, String> {
    let files = vec![("page.cs".to_string(), COLORS.to_string())];
    let v = value_from_runtime(type_name, "", value)?;
    let e = set_property(&files, &colors_tree(), node, prop, &v)?;
    let lines = Lines::new(&e.text);
    let (a, b) = (setcode_byte(&e.text, &lines, e.range.start), setcode_byte(&e.text, &lines, e.range.end));
    let tail = e.text.len() - b;
    assert_eq!(&e.text[..a], &COLORS[..a]);
    assert_eq!(&e.text[b..], &COLORS[COLORS.len() - tail..]);
    Ok(e.text)
}

fn setcode_byte(text: &str, lines: &Lines, p: TextPosition) -> usize {
    let start = lines.starts[p.line as usize];
    start + text[start..].char_indices().scan(0u32, |u, (i, c)| { let r = (*u, i); *u += c.len_utf16() as u32; Some(r) })
        .find(|(u, _)| *u >= p.character).map_or(text.len() - start, |(_, i)| i)
}

#[test]
fn colors_keep_their_form() {
    const C: &str = "DrawnUi.Color";
    let has = |r: Result<String, String>, s: &str| {
        let t = r.unwrap_or_else(|e| panic!("refused: {e}"));
        assert!(t.contains(s), "{s} not in\n{t}");
    };
    has(set_typed(2, "TextColor", C, "#FFFF0000"), r##"TextColor = Color.Parse("#f00"),"##);
    has(set_typed(2, "TextColor", C, "#FF123456"), r##"TextColor = Color.Parse("#123456"),"##);
    has(set_typed(2, "TextColor", C, "#80123456"), r##"TextColor = Color.Parse("#80123456"),"##);
    has(set_typed(2, "BackgroundColor", C, "#FFFF0000"), "BackgroundColor = Colors.Red,");
    has(set_typed(2, "BackgroundColor", C, "#FF123456"), r##"BackgroundColor = Color.Parse("#123456"),"##);
    has(set_typed(2, "BackgroundColor", C, "#FFFFFFFF"), "BackgroundColor = Colors.White,");
    has(set_typed(3, "StrokeColor", C, "#FF00FF00"), r##"StrokeColor = Color.Parse("#FF00FF00") }"##);
    has(set_typed(4, "TextColor", C, "#00FF00"), r##"new SkiaLabel() { TextColor = Color.Parse("#00FF00") },"##);
    // Through the helper's parameter, at the call site.
    has(set_typed(5, "BackgroundColor", C, "#FF0000FF"), r##"Box(Color.Parse("#0000ff")),"##);
    let err = set_typed(3, "TextColor", C, "#FF000000").unwrap_err();
    assert!(err.contains("`Muted`"), "{err}");
    assert!(set_typed(2, "TextColor", C, "red").is_err());
}

#[test]
fn thickness_takes_the_shortest_form() {
    const T: &str = "DrawnUi.Views.Thickness";
    let has = |r: Result<String, String>, s: &str| {
        let t = r.unwrap_or_else(|e| panic!("refused: {e}"));
        assert!(t.contains(s), "{s} not in\n{t}");
    };
    has(set_typed(2, "Margin", T, "8,8,8,8"), "Margin = new Thickness(8),");
    has(set_typed(2, "Margin", T, "8,4,8,4"), "Margin = new Thickness(8, 4),");
    has(set_typed(2, "Margin", T, "0.5,1,2,3"), "Margin = new Thickness(0.5, 1, 2, 3),");
    has(set_typed(2, "Padding", T, "1,2,3,4"), "Padding = new(1, 2, 3, 4) }");
    has(set_typed(3, "Margin", T, "2"), ".WithMargin(2),");
    has(set_typed(4, "Padding", T, "1,2"), "new SkiaLabel() { Padding = new Thickness(1, 2) },");
    assert!(set_typed(2, "Margin", T, "1,2,3").is_err());
}

const MAUI_ANDROID: &str = concat!(env!("CARGO_MANIFEST_DIR"), r"\..\..\fixtures\maui-android");

/// Another page that also puts a SkiaLayout into a Canvas: the root's type alone does not tell.
const OTHER_PAGE: &str = "namespace DrawnUI.Tutorials.Other;\npublic class OtherPage : BasePageReloadable\n{\n    public override void Build()\n    {\n        Content = new Canvas { Content = new SkiaLayout { Children = { new SkiaLabel(\"Other\"), new SkiaButton(\"B\") } } };\n    }\n}\n";

/// The recorded firstappc screen, its page, and the other page; `edit` changes the tree first.
fn maui_case(with_other: bool, edit: impl Fn(&mut designer_protocol::Tree)) -> (Vec<(String, String)>, designer_protocol::Tree) {
    let page = std::fs::read_to_string(format!(r"{MAUI_ANDROID}\FirstAppPageCode.cs")).unwrap();
    let mut files = vec![("FirstAppPageCode.cs".to_string(), page.trim_start_matches('\u{feff}').to_string())];
    if with_other {
        files.push(("OtherPage.cs".into(), OTHER_PAGE.into()));
    }
    let mut tree: designer_protocol::Tree = serde_json::from_str(&std::fs::read_to_string(format!(r"{MAUI_ANDROID}\firstappc.json")).unwrap()).unwrap();
    edit(&mut tree);
    (files, tree)
}

fn keys_of(files: &[(String, String)], tree: &designer_protocol::Tree) -> Vec<(u64, String)> {
    match_runtime(files, tree).into_iter().map(|m| (m.node, m.key)).collect()
}

const FIRST_APP: [(u64, &str); 6] = [
    (30, "FirstAppPageCode.CreateMainLayout/0"),
    (31, "FirstAppPageCode.CreateMainLayout/0/Children/0"),
    (32, "FirstAppPageCode.CreateMainLayout/0/Children/1"),
    (33, "FirstAppPageCode.CreateMainLayout/0/Children/2"),
    (34, "FirstAppPageCode.CreateMainLayout/0/Children/3"),
    (38, "FirstAppPageCode.CreateMainLayout/0/Children/4"),
];

#[test]
fn a_field_anchor_climbs_to_its_parent_and_siblings() {
    let (files, tree) = maui_case(true, |_| {});
    let keys = keys_of(&files, &tree);
    for (node, key) in FIRST_APP {
        assert!(keys.contains(&(node, key.to_string())), "{node} -> {key} missing from {keys:?}");
    }
    assert!(keys.contains(&(39, "FirstAppPageCode.CreateMainLayout/0/Children/5".into())), "{keys:?}");

    // The label built with `new SkiaLabel("..")`: Text is its constructor argument.
    let v = value_from_runtime("System.String", "DrawnUI for .NET MAUI", "Hello").unwrap();
    let e = set_property(&files, &tree, 32, "Text", &v).unwrap();
    assert!(e.text.contains("new SkiaLabel(\"Hello\")\n") || e.text.contains("new SkiaLabel(\"Hello\")\r\n"), "{}", e.text);
    assert_eq!(e.text.replace("\"Hello\"", "\"DrawnUI for .NET MAUI\""), files[0].1);
    assert_eq!(e.range.start.line, 62);
    // Another property of that label: the literal in its initializer.
    let v = value_from_runtime("System.Double", "24", "30").unwrap();
    let e = set_property(&files, &tree, 32, "FontSize", &v).unwrap();
    assert_eq!(e.text.replacen("FontSize = 30,", "FontSize = 24,", 1), files[0].1);
}

#[test]
fn a_canvas_content_anchors_a_root_without_fields() {
    let no_field = |t: &mut designer_protocol::Tree| t.nodes.iter_mut().for_each(|n| n.field = None);
    // One page puts a SkiaLayout into a Canvas: that is the root.
    let (files, tree) = maui_case(false, no_field);
    let keys = keys_of(&files, &tree);
    for (node, key) in FIRST_APP {
        assert!(keys.contains(&(node, key.to_string())), "{node} -> {key} missing from {keys:?}");
    }
    // Two pages do: no field, no host hint, nothing is guessed.
    let (files, tree) = maui_case(true, no_field);
    assert_eq!(keys_of(&files, &tree), vec![]);
    // The host hint names the page.
    let (files, tree) = maui_case(true, |t| {
        no_field(t);
        t.nodes[0].host = Some("DrawnUI.Tutorials.FirstApp.FirstAppPageCode".into());
    });
    let keys = keys_of(&files, &tree);
    assert!(keys.contains(&(32, FIRST_APP[2].1.to_string())), "{keys:?}");
    let (files, tree) = maui_case(true, |t| {
        no_field(t);
        t.nodes[0].host = Some("DrawnUI.Tutorials.Other.OtherPage".into());
    });
    let keys = keys_of(&files, &tree);
    assert_eq!(keys.first().map(|k| k.1.as_str()), Some("OtherPage.Build/0/Content/0"), "{keys:?}");
}

#[test]
fn code_set_names_come_from_the_creating_code() {
    let (files, mut tree) = maui_case(false, |_| {});
    // A part the library builds under the button: no code for it.
    tree.nodes.push(designer_protocol::Node { id: 99, parent: Some(34), type_name: "DrawnUi.Draw.SkiaShape".into(), internal: true, ..Default::default() });
    let code: HashMap<String, HashMap<String, ElementCode>> = files.iter().map(|(u, t)| (u.clone(), element_code(u, t))).collect();
    let ms = match_runtime(&files, &tree);
    let names = |node: u64| ms.iter().find(|m| m.node == node).and_then(|m| code_set_names(m, &code));
    let v = |s: &[&str]| Some(s.iter().map(|x| x.to_string()).collect::<Vec<_>>());
    assert_eq!(names(32), v(&["Text", "UseCache", "FontSize", "FontAttributes", "TextColor", "HorizontalTextAlignment", "HorizontalOptions"]));
    assert_eq!(names(34), v(&["Text", "UseCache", "BackgroundColor", "TextColor", "CornerRadius", "HorizontalOptions"]));
    assert_eq!(names(30), v(&["Type", "HorizontalOptions", "Padding", "Spacing", "UseCache", "VerticalOptions", "Children"]));
    assert_eq!(names(99), None);

    // Fluent setters, and what a class's constructor sets on the instances it builds.
    let src = r#"namespace Demo;
public class Chip : SkiaShape
{
    public Chip() { HorizontalOptions = LayoutOptions.Fill; Padding = 4; }
}
public class Page : SkiaLayer
{
    public Page()
    {
        Children = new List<SkiaControl> { new SkiaLabel { FontSize = 12 }.WithMargin(4).FillX().OnTapped(me => { }), new Chip { Tag = "x" } };
    }
}
"#;
    let code: HashMap<String, HashMap<String, ElementCode>> = [("p.cs".to_string(), element_code("p.cs", src))].into();
    let m = |key: &str| RuntimeMatch { node: 1, uri: "p.cs".into(), key: key.into(), via: vec![] };
    assert_eq!(code_set_names(&m("Page/this/Children/0"), &code), v(&["FontSize", "Margin", "HorizontalOptions"]));
    assert_eq!(code_set_names(&m("Page/this/Children/1"), &code), v(&["Tag", "HorizontalOptions", "Padding"]));
    assert_eq!(code_set_names(&m("Page/this/Nope"), &code), None);
}

/// A DrawFiddle snippet: top-level statements whose last `return` is the runtime root, with a
/// local helper function, a lambda, a local and a custom control declared beside them.
const SNIPPET: &str = r#"SkiaControl Card(string title) => new SkiaShape
{
    Padding = 8,
    Children = { new SkiaLabel { Text = title, FontSize = 14 } }
};

var make = (string t) => new SkiaLabel { Text = t };
var footer = new SkiaLabel { Text = "End" };

return new SkiaStack
{
    Spacing = 4,
    Children = { new SkiaLabel { Text = "Hi" }, Card("First"), make("Made"), new Chip(), footer }
};

class Chip : SkiaShape
{
    public Chip()
    {
        Padding = 2;
        Children = new List<SkiaControl> { new SkiaLabel { Text = "chip" } };
    }
}
"#;

/// Canvas -> the Fiddle's own SkiaLayout -> the snippet's SkiaStack.
fn snippet_tree() -> designer_protocol::Tree {
    let n = |id, parent, t: &str| designer_protocol::Node { id, parent, type_name: format!("DrawnUi.Draw.{t}"), ..Default::default() };
    designer_protocol::Tree {
        revision: 1,
        nodes: vec![
            n(1, None, "SkiaLayout"),
            n(2, Some(1), "SkiaStack"),
            n(3, Some(2), "SkiaLabel"),
            n(4, Some(2), "SkiaShape"),
            n(5, Some(4), "SkiaLabel"),
            n(6, Some(2), "SkiaLabel"),
            designer_protocol::Node { id: 7, parent: Some(2), type_name: "Chip".into(), ..Default::default() },
            n(8, Some(7), "SkiaLabel"),
            n(9, Some(2), "SkiaLabel"),
        ],
        ..Default::default()
    }
}

#[test]
fn a_fiddle_snippet_matches_from_its_top_level_return() {
    let files = vec![("fiddle:///snippet.cs".to_string(), SNIPPET.to_string())];
    let tree = snippet_tree();
    let keys: HashMap<u64, String> = match_runtime(&files, &tree).into_iter().map(|m| (m.node, m.key)).collect();
    assert!(!keys.contains_key(&1), "the Fiddle's own layout is not the snippet's: {keys:?}");
    for node in 2..=9 {
        assert!(keys.contains_key(&node), "node {node} unmatched: {keys:?}");
    }
    let write = |node: u64, value: &str| {
        let v = value_from_runtime("System.String", "", value).unwrap();
        set_property(&files, &tree, node, "Text", &v).map(|e| e.text)
    };
    // In the returned stack; through the local function's parameter; through the lambda's; a local.
    assert_eq!(write(3, "Hello").unwrap(), SNIPPET.replace("new SkiaLabel { Text = \"Hi\" }", "new SkiaLabel { Text = \"Hello\" }"));
    assert_eq!(write(5, "Second").unwrap(), SNIPPET.replace("Card(\"First\")", "Card(\"Second\")"));
    assert_eq!(write(6, "Built").unwrap(), SNIPPET.replace("make(\"Made\")", "make(\"Built\")"));
    assert_eq!(write(9, "Bye").unwrap(), SNIPPET.replace("\"End\"", "\"Bye\""));
    // The custom control's own child, built in its constructor.
    assert_eq!(write(8, "pill").unwrap(), SNIPPET.replace("\"chip\"", "\"pill\""));
    // A file of plain classes has no snippet root: nothing is guessed from a method that returns a control.
    let class_file = vec![("page.cs".to_string(), "static class F { static SkiaControl Build() { return new SkiaStack { Children = { new SkiaLabel() } }; } }".to_string())];
    assert!(match_runtime(&class_file, &tree).is_empty());
}

const INSERT_PAGE: &str = r#"namespace Demo;
public class Page : SkiaLayer
{
    public Page()
    {
        Children = new List<SkiaControl>
        {
            new SkiaStack
            {
                Spacing = 8,
                Children =
                {
                    new SkiaLabel { Text = "Héllo 🌍" },
                    new SkiaLabel { Text = "Two" },
                },
            },
            new SkiaRow { Spacing = 4 },
            new SkiaWrap { Children = Enumerable.Range(0, 3).Select(i => (SkiaControl)new SkiaLabel { Text = $"{i}" }).ToList() },
            new SkiaLayout(),
            new SkiaStack { Children = { new SkiaLabel { Text = "Ünïcode 🌍" }, Pair() } },
        };
    }
    static SkiaControl[] Pair() => new SkiaControl[] { new SkiaLabel(), new SkiaLabel() };
}
"#;

fn insert_tree() -> designer_protocol::Tree {
    let n = |id, parent, t: &str| designer_protocol::Node { id, parent, type_name: format!("DrawnUi.Draw.{t}"), ..Default::default() };
    designer_protocol::Tree {
        revision: 1,
        nodes: vec![
            designer_protocol::Node { id: 1, type_name: "Demo.Page".into(), ..Default::default() },
            n(2, Some(1), "SkiaStack"),
            n(3, Some(2), "SkiaLabel"),
            n(4, Some(2), "SkiaLabel"),
            n(5, Some(1), "SkiaRow"),
            n(6, Some(1), "SkiaWrap"),
            n(7, Some(6), "SkiaLabel"),
            n(8, Some(6), "SkiaLabel"),
            n(9, Some(6), "SkiaLabel"),
            n(10, Some(1), "SkiaLayout"),
            n(11, Some(1), "SkiaStack"),
            n(12, Some(11), "SkiaLabel"),
            n(13, Some(11), "SkiaLabel"),
            n(14, Some(11), "SkiaLabel"),
        ],
        ..Default::default()
    }
}

const TYPES: [(&str, &str); 5] = [
    ("Text", "System.String"),
    ("FontSize", "System.Double"),
    ("TextColor", "DrawnUi.Color"),
    ("Padding", "DrawnUi.Views.Thickness"),
    ("HorizontalOptions", "DrawnUi.Views.LayoutOptions"),
];

fn insert(files: &[(String, String)], tree: &designer_protocol::Tree, parent: u64, index: usize, t: &str, defaults: &[(&str, &str)]) -> Result<CodeEdit, designer_protocol::Refusal> {
    let d: Vec<(String, String)> = defaults.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    let types: Vec<(String, String)> = TYPES.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    insert_element(files, tree, parent, index, t, &d, &types)
}

/// The text a range covers, counting UTF-16 columns like an editor.
fn range_text(text: &str, r: EditorRange) -> String {
    let lines = Lines::new(text);
    let at = |p: TextPosition| setcode_byte(text, &lines, p);
    text[at(r.start)..at(r.end)].to_string()
}

#[test]
fn insert_element_writes_into_the_children_list() {
    let files = vec![("page.cs".to_string(), INSERT_PAGE.to_string())];
    let tree = insert_tree();
    let label = r#"new SkiaLabel { Text = "Label" }"#;
    let ins = |parent, index| insert(&files, &tree, parent, index, "DrawnUi.Draw.SkiaLabel", &[("Text", "Label")]).unwrap();
    let list = |a: &str| INSERT_PAGE.replace("                    new SkiaLabel { Text = \"Héllo 🌍\" },\n                    new SkiaLabel { Text = \"Two\" },\n", a);
    // Start, middle, end: one control per line, the siblings' indentation, trailing commas kept.
    let e = ins(2, 0);
    assert_eq!(e.text, list(&format!("                    {label},\n                    new SkiaLabel {{ Text = \"Héllo 🌍\" }},\n                    new SkiaLabel {{ Text = \"Two\" }},\n")));
    assert_eq!(range_text(&e.text, e.range), label);
    let e = ins(2, 1);
    assert_eq!(e.text, list(&format!("                    new SkiaLabel {{ Text = \"Héllo 🌍\" }},\n                    {label},\n                    new SkiaLabel {{ Text = \"Two\" }},\n")));
    let e = ins(2, 2);
    assert_eq!(e.text, list(&format!("                    new SkiaLabel {{ Text = \"Héllo 🌍\" }},\n                    new SkiaLabel {{ Text = \"Two\" }},\n                    {label},\n")));
    assert_eq!(range_text(&e.text, e.range), label);
    // The outer list of the page constructor, at its end.
    let e = insert(&files, &tree, 1, 5, "DrawnUi.Draw.SkiaButton", &[]).unwrap();
    assert!(e.text.contains("Pair() } },\n            new SkiaButton(),\n        };"), "{}", e.text);

    // A parent with no children yet gets a Children list; the value spellings follow the types.
    let e = insert(&files, &tree, 5, 0, "DrawnUi.Draw.SkiaLabel", &[("Text", "Hi"), ("FontSize", "14"), ("TextColor", "#FF0000"), ("Padding", "8"), ("HorizontalOptions", "Center")]).unwrap();
    let el = r##"new SkiaLabel { Text = "Hi", FontSize = 14, TextColor = Color.Parse("#FF0000"), Padding = new Thickness(8), HorizontalOptions = LayoutOptions.Center }"##;
    assert_eq!(e.text, INSERT_PAGE.replace("new SkiaRow { Spacing = 4 },", &format!("new SkiaRow {{ Spacing = 4, Children = {{ {el} }} }},")));
    assert_eq!(range_text(&e.text, e.range), el);
}

#[test]
fn insert_element_refuses_what_it_cannot_place() {
    let files = vec![("page.cs".to_string(), INSERT_PAGE.to_string())];
    let tree = insert_tree();
    let key = |r: Result<CodeEdit, designer_protocol::Refusal>| r.unwrap_err().key;
    // Children made by a loop, by a helper that makes several, a parent built without an initializer.
    assert_eq!(key(insert(&files, &tree, 6, 0, "SkiaLabel", &[])), "src-insert-no-list");
    assert_eq!(key(insert(&files, &tree, 11, 0, "SkiaLabel", &[])), "src-insert-unmapped");
    assert_eq!(key(insert(&files, &tree, 10, 0, "SkiaLabel", &[])), "src-insert-no-initializer");
    assert_eq!(key(insert(&files, &tree, 2, 0, "SkiaLabel", &[("Opacity", "0.5")])), "src-insert-type-unknown");
    assert!(!designer_protocol::Refusal::new("src-insert-unmapped", &[]).to_string().starts_with("src-"), "English text exists");
}

#[test]
fn insert_after_non_ascii_on_one_line_and_in_a_snippet() {
    // A one-line list with non-ASCII text before the insert: the range still covers the new control.
    let src = "namespace Demo;\npublic class Page : SkiaLayer\n{\n    public Page()\n    {\n        Children = new List<SkiaControl> { new SkiaRow { Children = { new SkiaLabel { Text = \"Ünïcode 🌍\" } } } };\n    }\n}\n";
    let files = vec![("p.cs".to_string(), src.to_string())];
    let n = |id, parent, t: &str| designer_protocol::Node { id, parent, type_name: t.into(), ..Default::default() };
    let tree = designer_protocol::Tree { nodes: vec![n(1, None, "Demo.Page"), n(2, Some(1), "SkiaRow"), n(3, Some(2), "SkiaLabel")], ..Default::default() };
    let e = insert(&files, &tree, 2, 1, "SkiaButton", &[("Text", "Ok")]).unwrap();
    assert!(e.text.contains(r#"{ new SkiaLabel { Text = "Ünïcode 🌍" }, new SkiaButton { Text = "Ok" } }"#), "{}", e.text);
    assert_eq!(range_text(&e.text, e.range), r#"new SkiaButton { Text = "Ok" }"#);

    // A DrawFiddle snippet: the returned stack inside the layout the Fiddle adds.
    let snippet = "return new SkiaStack\n{\n    Children = { new SkiaLabel { Text = \"Hi\" } }\n};\n";
    let files = vec![("fiddle:///snippet.cs".to_string(), snippet.to_string())];
    let tree = designer_protocol::Tree { nodes: vec![n(1, None, "SkiaLayout"), n(2, Some(1), "SkiaStack"), n(3, Some(2), "SkiaLabel")], ..Default::default() };
    let e = insert(&files, &tree, 2, 0, "SkiaButton", &[("Text", "Go")]).unwrap();
    assert_eq!(e.text, snippet.replace("{ new SkiaLabel", "{ new SkiaButton { Text = \"Go\" }, new SkiaLabel"));
    let r = remove_element(&files, &tree, 3).unwrap();
    assert_eq!(r.text, snippet.replace("{ new SkiaLabel { Text = \"Hi\" } }", "{ }"));
}

#[test]
fn remove_element_takes_an_item_out_of_its_list() {
    let files = vec![("page.cs".to_string(), INSERT_PAGE.to_string())];
    let tree = insert_tree();
    let r = remove_element(&files, &tree, 4).unwrap();
    assert_eq!(r.text, INSERT_PAGE.replace("                    new SkiaLabel { Text = \"Two\" },\n", ""));
    let r = remove_element(&files, &tree, 5).unwrap();
    assert_eq!(r.text, INSERT_PAGE.replace("            new SkiaRow { Spacing = 4 },\n", ""));
    // A label made by a loop is not an item of a list.
    assert_eq!(remove_element(&files, &tree, 7).unwrap_err().key, "src-remove-not-in-list");
}

#[test]
fn corner_radius_and_nested_copies_spell_like_set_property() {
    // A corner radius the same everywhere is a number; different corners are refused.
    let cr = "DrawnUi.Draw.CornerRadius";
    assert_eq!(value_from_runtime(cr, "10", "10").unwrap(), PropValue::Plain(SourceEditValue::Number { value: 10.0 }));
    assert_eq!(value_from_runtime(cr, "4,4,4,4", "4,4,4,4").unwrap(), PropValue::Plain(SourceEditValue::Number { value: 4.0 }));
    assert_eq!(value_from_runtime(cr, "10,10,0,0", "10,10,0,0").unwrap_err().key, "src-type-unsupported");

    // A copy with a child, written at the end of a list: the root takes a type from `root_types`
    // when the spec has none.
    let files = vec![("page.cs".to_string(), INSERT_PAGE.to_string())];
    let tree = insert_tree();
    let p = |name: &str, value: &str, t: &str| designer_protocol::SpecProp { name: name.into(), value: value.into(), type_name: t.into() };
    let label = designer_protocol::ControlSpec { type_name: "DrawnUi.Draw.SkiaLabel".into(), props: vec![p("Text", "Start", "System.String")], children: vec![] };
    let spec = designer_protocol::ControlSpec { type_name: "DrawnUi.Draw.SkiaShape".into(), props: vec![p("CornerRadius", "12", "")], children: vec![label] };
    let e = insert_spec(&files, &tree, 2, 9, &spec, &[(String::from("CornerRadius"), String::from(cr))], &std::collections::HashMap::new()).unwrap();
    // With children: one property per line, `Children = { .. }` (no original form given), one level deeper.
    let el = "new SkiaShape\n                    {\n                        CornerRadius = 12,\n                        Children =\n                        {\n                            new SkiaLabel { Text = \"Start\" },\n                        },\n                    }";
    assert_eq!(e.text, INSERT_PAGE.replace("Text = \"Two\" },\n", &format!("Text = \"Two\" }},\n                    {el},\n")));
    assert_eq!(range_text(&e.text, e.range), el);
}

const APP_TSX: &str = r##"import { Canvas, Colors, SkiaLabel, SkiaShape, SkiaStack, Thickness } from "drawnui-react";

const footer = <SkiaLabel Text="Footer" />;

function Card({ title, children }: { title: string; children?: React.ReactNode }) {
  return (
    <SkiaShape CornerRadius={12} BackgroundColor="#2B3035">
      <SkiaLabel Text={title} FontSize={18} />
      {children}
    </SkiaShape>
  );
}

export function MainPage() {
  const items = ["a", "b"];
  return (
    <SkiaStack Spacing={8} Padding="12,6" HorizontalOptions="Center" IsClippedToBounds>
      <SkiaLabel Text="Héllo 🌍" TextColor={Colors.White} Margin={new Thickness(4, 2)} />
      <Card title="First">
        <SkiaLabel Text="Inside" />
      </Card>
      <SkiaStack Spacing={2}>
        <SkiaLabel Text="One" />
        <SkiaLabel Text="Two" />
      </SkiaStack>
      <SkiaStack Spacing={4} />
      {footer}
      {/* the rest */}
      {items.map((s) => <SkiaLabel key={s} Text={s} />)}
    </SkiaStack>
  );
}

createRoot(document.getElementById("root")!).render(
  <Canvas BackgroundColor="#212529">
    <MainPage />
  </Canvas>,
);
"##;

fn app_tree() -> designer_protocol::Tree {
    let n = |id, parent, t: &str| designer_protocol::Node { id, parent, type_name: format!("DrawnUi.Draw.{t}"), ..Default::default() };
    designer_protocol::Tree {
        revision: 1,
        nodes: vec![
            n(1, None, "SkiaStack"),
            n(2, Some(1), "SkiaLabel"),
            n(3, Some(1), "SkiaShape"),
            n(4, Some(3), "SkiaLabel"),
            n(5, Some(3), "SkiaLabel"),
            n(6, Some(1), "SkiaStack"),
            n(7, Some(6), "SkiaLabel"),
            n(8, Some(6), "SkiaLabel"),
            n(9, Some(1), "SkiaStack"),
            n(10, Some(1), "SkiaLabel"),
            n(11, Some(1), "SkiaLabel"),
            n(12, Some(1), "SkiaLabel"),
        ],
        ..Default::default()
    }
}

#[test]
fn tsx_reads_jsx_as_elements_attributes_and_children() {
    let t = read_tsx("app.tsx", APP_TSX);
    let node = |k: &str| t.nodes.iter().find(|n| n.key == k).unwrap_or_else(|| panic!("no {k}: {:?}", t.nodes.iter().map(|n| &n.key).collect::<Vec<_>>()));
    assert_eq!(node("MainPage/0").name.as_deref(), Some("SkiaStack"));
    assert_eq!(node("MainPage/0/Spacing/=").value.as_deref(), Some("{8}"));
    assert_eq!(node("MainPage/0/Padding/=").value.as_deref(), Some("\"12,6\""));
    assert_eq!(node("MainPage/0/HorizontalOptions/=").value.as_deref(), Some("\"Center\""));
    assert_eq!(node("MainPage/0/IsClippedToBounds/=").value.as_deref(), Some("true"));
    assert_eq!(node("MainPage/0/Children/0/TextColor/=").role, SourceRole::Literal);
    assert_eq!(node("MainPage/0/Children/0/Margin/=").role, SourceRole::Reference);
    assert_eq!(node("MainPage/0/Children/1").name.as_deref(), Some("Card"));
    assert_eq!(node("MainPage/0/Children/2/Children/1/Text/=").value.as_deref(), Some("\"Two\""));
    assert_eq!(node("MainPage/0/Children/4").role, SourceRole::Reference); // {footer}; the comment is no child
    assert_eq!(node("MainPage/0/Children/5").role, SourceRole::Reference); // {items.map(..)}
    assert_eq!(node("Card/0").name.as_deref(), Some("SkiaShape"));
    assert_eq!(node("<module>/0").name.as_deref(), Some("SkiaLabel"));
    // The label's range counts UTF-16 columns.
    let l = node("MainPage/0/Children/0/Margin");
    assert_eq!(l.range.start.character, 58); // `🌍` counts 2
}

#[test]
fn tsx_edits_spell_values_the_jsx_way() {
    let t = read_tsx("app.tsx", APP_TSX);
    let edit = |key: &str, e: SourceEdit| {
        let a = edit_tsx(APP_TSX, &t, key, &e);
        assert_eq!(a.refused, None, "{key}");
        let lines = Lines::new(APP_TSX);
        let x = a.edits[0].clone();
        let (s, en) = (setcode_byte(APP_TSX, &lines, x.range.start), setcode_byte(APP_TSX, &lines, x.range.end));
        format!("{}{}{}", &APP_TSX[..s], x.new_text, &APP_TSX[en..])
    };
    let s = |v: SourceEditValue| SourceEdit::SetValue { value: v };
    assert!(edit("MainPage/0/Spacing", s(SourceEditValue::Number { value: 10.0 })).contains("<SkiaStack Spacing={10} Padding"));
    assert!(edit("MainPage/0/Children/0/Text", s(SourceEditValue::String { value: "Say \"hi\"".into() })).contains(r#"<SkiaLabel Text={"Say \"hi\""} TextColor"#));
    assert!(edit("MainPage/0/IsClippedToBounds", s(SourceEditValue::Boolean { value: false })).contains("IsClippedToBounds={false}>"));
    assert!(edit("MainPage/0/HorizontalOptions", s(SourceEditValue::Case { name: "LayoutOptions.Fill".into() })).contains("HorizontalOptions=\"Fill\""));
    let added = edit("MainPage/0/Children/2", SourceEdit::AddProperty { name: "Padding".into(), value: SourceEditValue::Number { value: 4.0 } });
    assert!(added.contains("<SkiaStack Spacing={2} Padding={4}>"), "{added}");
    assert!(edit("MainPage/0/Spacing", SourceEdit::Remove).contains("<SkiaStack Padding=\"12,6\""));
    assert_eq!(edit("MainPage/0/Children/2/Children/0", SourceEdit::Remove), APP_TSX.replace("        <SkiaLabel Text=\"One\" />\n", ""));
    assert_eq!(edit_tsx(APP_TSX, &t, "MainPage/0/Children/0/Margin", &s(SourceEditValue::Number { value: 1.0 })).refused, Some(SourceEditRefusal::NotEditable));
}

#[test]
fn tsx_runtime_controls_match_through_canvas_components_and_consts() {
    let files = vec![("src/app.tsx".to_string(), APP_TSX.to_string())];
    let tree = app_tree();
    let ms = match_runtime(&files, &tree);
    let key = |node: u64| ms.iter().find(|m| m.node == node).map(|m| (m.key.clone(), m.via.clone()));
    let k = |s: &str| s.to_string();
    assert_eq!(key(1), Some((k("MainPage/0"), vec![k("<module>/1/Children/0")])));
    assert_eq!(key(2).unwrap().0, "MainPage/0/Children/0");
    assert_eq!(key(3).unwrap(), (k("Card/0"), vec![k("<module>/1/Children/0"), k("MainPage/0/Children/1")]));
    assert_eq!(key(4).unwrap().0, "Card/0/Children/0");
    assert_eq!(key(5).unwrap().0, "MainPage/0/Children/1/Children/0"); // `{children}` of the Card usage
    assert_eq!(key(8).unwrap().0, "MainPage/0/Children/2/Children/1");
    assert_eq!(key(9).unwrap().0, "MainPage/0/Children/3");
    assert_eq!(key(10).unwrap().0, "<module>/0"); // `{footer}`
    assert_eq!(key(11), None); // made by `.map`: not guessed
}

#[test]
fn tsx_writes_values_inserts_and_removals() {
    let files = vec![("src/app.tsx".to_string(), APP_TSX.to_string())];
    let tree = app_tree();
    let set = |node: u64, prop: &str, t: &str, v: &str| {
        let value = value_from_runtime(t, v, v).unwrap();
        set_property(&files, &tree, node, prop, &value).map(|e| e.text)
    };
    assert_eq!(set(2, "Text", "System.String", "Hello").unwrap(), APP_TSX.replace("Text=\"Héllo 🌍\"", "Text=\"Hello\""));
    assert_eq!(set(2, "TextColor", "DrawnUi.Color", "#FFFF0000").unwrap(), APP_TSX.replace("{Colors.White}", "{Colors.Red}"));
    assert_eq!(set(2, "Margin", "DrawnUi.Views.Thickness", "8").unwrap(), APP_TSX.replace("new Thickness(4, 2)", "new Thickness(8)"));
    assert_eq!(set(1, "Padding", "DrawnUi.Views.Thickness", "16,6").unwrap(), APP_TSX.replace("Padding=\"12,6\"", "Padding=\"16,6\""));
    assert_eq!(set(3, "BackgroundColor", "DrawnUi.Color", "#FF112233").unwrap(), APP_TSX.replace("\"#2B3035\"", "\"#112233\""));
    // A component's prop: written where the component is used.
    assert_eq!(set(4, "Text", "System.String", "Second").unwrap(), APP_TSX.replace("title=\"First\"", "title=\"Second\""));
    // A property the element does not set yet.
    assert_eq!(set(7, "FontSize", "System.Double", "20").unwrap(), APP_TSX.replace("<SkiaLabel Text=\"One\" />", "<SkiaLabel Text=\"One\" FontSize={20} />"));

    let types = vec![("Text".to_string(), "System.String".to_string())];
    let ins = |parent: u64, index: usize| insert_element(&files, &tree, parent, index, "DrawnUi.Draw.SkiaButton", &[("Text".to_string(), "Go".to_string())], &types);
    let e = ins(6, 1).unwrap();
    assert_eq!(e.text, APP_TSX.replace("        <SkiaLabel Text=\"One\" />\n", "        <SkiaLabel Text=\"One\" />\n        <SkiaButton Text=\"Go\" />\n"));
    let lines = Lines::new(&e.text);
    assert_eq!(&e.text[setcode_byte(&e.text, &lines, e.range.start)..setcode_byte(&e.text, &lines, e.range.end)], "<SkiaButton Text=\"Go\" />");
    // A parent with no children: `<SkiaStack .. />` opens.
    assert_eq!(ins(9, 0).unwrap().text, APP_TSX.replace("      <SkiaStack Spacing={4} />\n", "      <SkiaStack Spacing={4}>\n        <SkiaButton Text=\"Go\" />\n      </SkiaStack>\n"));
    // The root's children end in `.map(..)`: they do not line up with the code.
    assert_eq!(ins(1, 0).unwrap_err().key, "src-insert-unmapped");
    // Removing a control takes its line.
    assert_eq!(remove_element(&files, &tree, 8).unwrap().text, APP_TSX.replace("        <SkiaLabel Text=\"Two\" />\n", ""));
    // What the code sets: the attributes.
    let code = element_code("src/app.tsx", APP_TSX);
    assert_eq!(code["MainPage/0/Children/0"].names, vec!["Text", "TextColor", "Margin"]);
}

/// A DrawFiddle React snippet: the module's default export is rendered by the Canvas directly.
const FIDDLE_TSX: &str = r##"const PADDING = new Thickness(24);

function Badge({ text }) {
  return <SkiaLabel Text={text} FontSize={12} />;
}

export default function App() {
  const [count, setCount] = useState(0);
  const title = `Clicked ${count}`;
  return (
    <SkiaLayout Type="Column" Spacing={18} Padding={PADDING} HorizontalOptions="Center">
      <SkiaLabel Text="Welcome to DrawnUI" FontSize={26} TextColor="#FFFFFF" />
      <Badge text="New" />
      <SkiaButton Text={title} Tapped={() => setCount((c) => c + 1)} />
    </SkiaLayout>
  );
}
"##;

#[test]
fn tsx_fiddle_snippet_matches_from_its_default_export() {
    let files = vec![("fiddle:///snippet.tsx".to_string(), FIDDLE_TSX.to_string())];
    let n = |id, parent, t: &str| designer_protocol::Node { id, parent, type_name: format!("DrawnUi.Draw.{t}"), ..Default::default() };
    let tree = designer_protocol::Tree { nodes: vec![n(1, None, "SkiaLayout"), n(2, Some(1), "SkiaLabel"), n(3, Some(1), "SkiaLabel"), n(4, Some(1), "SkiaButton")], ..Default::default() };
    let keys: HashMap<u64, String> = match_runtime(&files, &tree).into_iter().map(|m| (m.node, m.key)).collect();
    assert_eq!(keys.get(&1).map(String::as_str), Some("App/0"));
    assert_eq!(keys.get(&3).map(String::as_str), Some("Badge/0"));
    assert_eq!(keys.get(&4).map(String::as_str), Some("App/0/Children/2"));
    let set = |node: u64, prop: &str, t: &str, v: &str| set_property(&files, &tree, node, prop, &value_from_runtime(t, v, v).unwrap());
    assert_eq!(set(2, "Text", "System.String", "Hello").unwrap().text, FIDDLE_TSX.replace("\"Welcome to DrawnUI\"", "\"Hello\""));
    assert_eq!(set(3, "Text", "System.String", "Old").unwrap().text, FIDDLE_TSX.replace("text=\"New\"", "text=\"Old\""));
    assert_eq!(set(1, "Spacing", "System.Double", "20").unwrap().text, FIDDLE_TSX.replace("Spacing={18}", "Spacing={20}"));
    // `Text={title}`: a value the code computes, not a literal.
    assert_eq!(set(4, "Text", "System.String", "x").unwrap_err().key, "src-not-literal");
    let types = vec![("Text".to_string(), "System.String".to_string())];
    let e = insert_element(&files, &tree, 1, 3, "SkiaLabel", &[("Text".to_string(), "End".to_string())], &types).unwrap();
    assert!(e.text.contains("c + 1)} />\n      <SkiaLabel Text=\"End\" />\n    </SkiaLayout>"), "{}", e.text);

    // A default export that is an element.
    let files = vec![("fiddle:///snippet.tsx".to_string(), "export default <SkiaLabel Text=\"Hi\" />;\n".to_string())];
    let tree = designer_protocol::Tree { nodes: vec![n(1, None, "SkiaLabel")], ..Default::default() };
    assert_eq!(match_runtime(&files, &tree)[0].key, "<module>/0");
}

#[test]
fn tsx_location_hints_match_controls_structure_cannot() {
    // Made by `.map`: unmatched by structure, matched by the hint the React agent sends.
    let uri = r"C:\app\src\app.tsx";
    let files = vec![(uri.to_string(), APP_TSX.to_string())];
    let mut tree = app_tree();
    let at = APP_TSX.find("<SkiaLabel key={s}").unwrap();
    let line = APP_TSX[..at].matches('\n').count() + 1;
    let col = at - APP_TSX[..at].rfind('\n').map_or(0, |i| i + 1) + 1;
    tree.nodes[10].source = Some(format!("/src/app.tsx:{line}:{col}"));
    tree.nodes[11].source = Some(format!("src/app.tsx:{line}")); // no column: the first element on the line
    let keys: HashMap<u64, String> = match_runtime(&files, &tree).into_iter().map(|m| (m.node, m.key)).collect();
    assert_eq!(keys.get(&11).map(String::as_str), Some("MainPage/1"));
    assert_eq!(keys.get(&12).map(String::as_str), Some("MainPage/1"));
    assert_eq!(keys.get(&2).map(String::as_str), Some("MainPage/0/Children/0"), "structure still matches the rest");
}

#[test]
fn move_element_reorders_and_moves_csharp_list_items() {
    let files = vec![("page.cs".to_string(), INSERT_PAGE.to_string())];
    let tree = insert_tree();
    let mv = |node, parent, index| move_element(&files, &tree, node, parent, index);
    let row = "            new SkiaRow { Spacing = 4 },\n";
    // Down: the row after the layout.
    let e = mv(5, 1, 3).unwrap();
    assert_eq!(e.text, INSERT_PAGE.replace(row, "").replace("            new SkiaStack { Children = { new SkiaLabel { Text = \"Ünïcode", &format!("{row}            new SkiaStack {{ Children = {{ new SkiaLabel {{ Text = \"Ünïcode")));
    assert_eq!(range_text(&e.text, e.range), "new SkiaRow { Spacing = 4 }");
    // Up: the layout first.
    let e = mv(10, 1, 0).unwrap();
    assert_eq!(e.text, INSERT_PAGE.replace("            new SkiaLayout(),\n", "").replace("        {\n            new SkiaStack\n", "        {\n            new SkiaLayout(),\n            new SkiaStack\n"));
    // A control over several lines to the end, its text as it was.
    let start = INSERT_PAGE.find("new SkiaStack\n").unwrap();
    let end = INSERT_PAGE[start..].find("\n            },").unwrap() + start + "\n            }".len();
    let stack = &INSERT_PAGE[start..end];
    let e = mv(2, 1, 4).unwrap();
    assert_eq!(e.text, INSERT_PAGE.replace(&format!("            {stack},\n"), "").replace("Pair() } },\n", &format!("Pair() }} }},\n            {stack},\n")));
    assert_eq!(range_text(&e.text, e.range), stack);
    // Where it already is: nothing changes.
    assert_eq!(mv(5, 1, 1).unwrap().text, INSERT_PAGE);
    // Across parents, after non-ASCII text: out of the inner list, into the outer one at its indentation.
    let hello = "new SkiaLabel { Text = \"Héllo 🌍\" }";
    let e = mv(3, 1, 1).unwrap();
    assert_eq!(e.text, INSERT_PAGE.replace(&format!("                    {hello},\n"), "").replace(row, &format!("            {hello},\n{row}")));
    assert_eq!(range_text(&e.text, e.range), hello);
    // Into a parent without children: it gets a list.
    let e = mv(4, 5, 0).unwrap();
    assert_eq!(e.text, INSERT_PAGE.replace("\n                    new SkiaLabel { Text = \"Two\" },", "").replace("new SkiaRow { Spacing = 4 },", "new SkiaRow { Spacing = 4, Children = { new SkiaLabel { Text = \"Two\" } } },"));
    assert_eq!(range_text(&e.text, e.range), "new SkiaLabel { Text = \"Two\" }");

    let key = |r: Result<CodeEdit, designer_protocol::Refusal>| r.unwrap_err().key;
    assert_eq!(key(mv(7, 1, 0)), "src-move-not-in-list", "made by a loop");
    assert_eq!(key(mv(2, 3, 0)), "src-move-into-itself");
    assert_eq!(key(mv(3, 6, 0)), "src-insert-no-list", "the target's children come from a loop");
    assert_eq!(key(mv(3, 11, 0)), "src-insert-unmapped", "a helper makes several of the target's children");
    assert!(!designer_protocol::Refusal::new("src-move-other-file", &[]).to_string().starts_with("src-"), "English text exists");

    // Another file: refused.
    let badge = "namespace Demo;\npublic class Badge : SkiaLayout\n{\n    public Badge()\n    {\n        Children = new List<SkiaControl> { new SkiaLabel { Text = \"B\" } };\n    }\n}\n";
    let page = "namespace Demo;\npublic class Page : SkiaLayer\n{\n    public Page()\n    {\n        Children = new List<SkiaControl> { new SkiaRow { Spacing = 4 }, new Badge() };\n    }\n}\n";
    let files = vec![("page.cs".to_string(), page.to_string()), ("badge.cs".to_string(), badge.to_string())];
    let n = |id, parent, t: &str| designer_protocol::Node { id, parent, type_name: t.into(), ..Default::default() };
    let tree = designer_protocol::Tree { nodes: vec![n(1, None, "Demo.Page"), n(2, Some(1), "DrawnUi.Draw.SkiaRow"), n(3, Some(1), "Demo.Badge"), n(4, Some(3), "DrawnUi.Draw.SkiaLabel")], ..Default::default() };
    assert_eq!(key(move_element(&files, &tree, 4, 2, 0)), "src-move-other-file");
}

#[test]
fn move_element_reorders_and_moves_jsx_children() {
    let files = vec![("src/app.tsx".to_string(), APP_TSX.to_string())];
    let mut tree = app_tree();
    let (one, two) = ("        <SkiaLabel Text=\"One\" />\n", "        <SkiaLabel Text=\"Two\" />\n");
    let swapped = APP_TSX.replace(&format!("{one}{two}"), &format!("{two}{one}"));
    // Down and up within one parent.
    let e = move_element(&files, &tree, 7, 6, 1).unwrap();
    assert_eq!(e.text, swapped);
    assert_eq!(range_text(&e.text, e.range), "<SkiaLabel Text=\"One\" />");
    assert_eq!(move_element(&files, &tree, 8, 6, 0).unwrap().text, swapped);
    assert_eq!(move_element(&files, &tree, 7, 6, 0).unwrap().text, APP_TSX);
    // A component usage over several lines, after non-ASCII text: cut from the root, re-indented
    // into the inner stack.
    let card = "      <Card title=\"First\">\n        <SkiaLabel Text=\"Inside\" />\n      </Card>\n";
    let e = move_element(&files, &tree, 3, 6, 2).unwrap();
    assert_eq!(e.text, APP_TSX.replace(card, "").replace(two, &format!("{two}        <Card title=\"First\">\n          <SkiaLabel Text=\"Inside\" />\n        </Card>\n")));
    assert_eq!(range_text(&e.text, e.range), "<Card title=\"First\">\n          <SkiaLabel Text=\"Inside\" />\n        </Card>");
    // Into a parent without children: `<SkiaStack .. />` opens.
    let e = move_element(&files, &tree, 7, 9, 0).unwrap();
    assert_eq!(e.text, APP_TSX.replace(one, "").replace("      <SkiaStack Spacing={4} />\n", "      <SkiaStack Spacing={4}>\n        <SkiaLabel Text=\"One\" />\n      </SkiaStack>\n"));

    let key = |r: Result<CodeEdit, designer_protocol::Refusal>| r.unwrap_err().key;
    assert_eq!(key(move_element(&files, &tree, 7, 1, 0)), "src-insert-unmapped", "the root's children end in `.map(..)`");
    assert_eq!(key(move_element(&files, &tree, 6, 7, 0)), "src-move-into-itself");
    // Made by `.map` (matched by its location hint): no list item to move.
    let at = APP_TSX.find("<SkiaLabel key={s}").unwrap();
    tree.nodes[11].source = Some(format!("src/app.tsx:{}", APP_TSX[..at].matches('\n').count() + 1));
    assert_eq!(key(move_element(&files, &tree, 12, 6, 0)), "src-move-not-in-list");
}


/// DrawFiddle's Kids preset: the shapes come from local helper functions, which is what the code
/// calls them.
const KIDS_PRESET: &str = r#"// ===== YOUR CODE =====

return Stack(
    Text("Hello World!"),
    Circle(Colors.Red, 110),
    Square(Colors.Orange, 80),
    Circle(Colors.Blue, 70)
);

// helpers
SkiaControl Stack(params SkiaControl[] shapes) => new SkiaStack
{
    Spacing = 16,
    Padding = new Thickness(30),
    Children = shapes.ToList(),
};

SkiaControl Circle(Color colour, double size) => new SkiaShape
{
    Type = ShapeType.Circle,
    WidthRequest = size, HeightRequest = size,
    BackgroundColor = colour,
};

SkiaControl Square(Color colour, double size) => new SkiaShape
{
    Type = ShapeType.Rectangle, CornerRadius = 4,
    WidthRequest = size, HeightRequest = size,
    BackgroundColor = colour,
};

SkiaControl Text(string words) => new SkiaLabel(words)
{
    FontSize = 44,
};
"#;

#[test]
fn controls_made_by_helpers_carry_the_helpers_names() {
    let files = vec![("fiddle:///snippet.cs".to_string(), KIDS_PRESET.to_string())];
    let n = |id, parent, t: &str| designer_protocol::Node { id, parent, type_name: format!("DrawnUi.Draw.{t}"), ..Default::default() };
    // The Fiddle's own layout, the snippet's stack, then Text, Circle, Square, Circle.
    let tree = designer_protocol::Tree {
        revision: 1,
        nodes: vec![n(1, None, "SkiaLayout"), n(2, Some(1), "SkiaStack"), n(3, Some(2), "SkiaLabel"), n(4, Some(2), "SkiaShape"), n(5, Some(2), "SkiaShape"), n(6, Some(2), "SkiaShape")],
        ..Default::default()
    };
    let names = made_by(&files, &match_runtime(&files, &tree));
    let name = |id: u64| names.get(&id).map(String::as_str);
    assert_eq!((name(2), name(3), name(4), name(5), name(6)), (Some("Stack"), Some("Text"), Some("Circle"), Some("Square"), Some("Circle")), "{names:?}");
    assert_eq!(name(1), None, "the Fiddle's own layout");
    // Written in place: no name; a helper of a class (a method), and a class instance (its type is its name).
    let files = vec![("fiddle:///snippet.cs".to_string(), SNIPPET.to_string())];
    let names = made_by(&files, &match_runtime(&files, &snippet_tree()));
    assert_eq!(names.get(&4).map(String::as_str), Some("Card"), "{names:?}");
    for written_in_place in [2, 3, 5, 7, 8, 9] {
        assert!(!names.contains_key(&written_in_place), "{written_in_place}: {names:?}");
    }
    // A method of a class, returning the element through a fluent call.
    let page = "class Page : SkiaLayer { public Page() { Children = new List<SkiaControl> { Badge(), new SkiaLabel() }; } SkiaControl Badge() { return new SkiaShape { Padding = 2 }.Assign(out _badge); } SkiaShape _badge; }";
    assert_eq!(helper_names("page.cs", page).values().collect::<Vec<_>>(), [&"Badge".to_string()]);
    // React: a component's own top-level element carries its name; what it holds is written in place.
    let files = vec![("src/app.tsx".to_string(), APP_TSX.to_string())];
    let names = made_by(&files, &match_runtime(&files, &app_tree()));
    assert_eq!((names.get(&1).map(String::as_str), names.get(&3).map(String::as_str)), (Some("MainPage"), Some("Card")), "{names:?}");
    assert!(!names.contains_key(&2) && !names.contains_key(&4) && !names.contains_key(&10), "{names:?}");
}

const TAGS: &str = r##"namespace Demo;
public class Page : SkiaLayer
{
    public Page()
    {
        Children = new List<SkiaControl>
        {
            Tag("One"),
            Tag("Two"),
            Card(),
            Card(),
        };
    }
    SkiaControl Tag(string t) => new SkiaLabel { Text = t, FontSize = 18 };
    SkiaLabel Plain(string t) => new SkiaLabel { Text = t };
    SkiaControl Card() => new SkiaShape { Children = { new SkiaLabel { Text = "title", FontSize = 12 } } };
}
"##;

/// Two uses of one helper: a value edited on one goes on that call (`.Adapt(..)`, its own chain's
/// setter when it has one), never into the helper; inside a helper used twice it is refused.
#[test]
fn a_value_changes_only_the_edited_call_of_a_helper() {
    let n = |id, parent, t: &str| designer_protocol::Node { id, parent, type_name: format!("DrawnUi.Draw.{t}"), ..Default::default() };
    let tree = designer_protocol::Tree {
        nodes: vec![
            designer_protocol::Node { id: 1, type_name: "Demo.Page".into(), ..Default::default() },
            n(2, Some(1), "SkiaLabel"),
            n(3, Some(1), "SkiaLabel"),
            n(4, Some(1), "SkiaShape"),
            n(5, Some(4), "SkiaLabel"),
            n(6, Some(1), "SkiaShape"),
            n(7, Some(6), "SkiaLabel"),
        ],
        ..Default::default()
    };
    let set = |text: &str, node, prop, t: &str, v: &str| {
        let value = value_from_runtime(t, v, v).map_err(|r| r.key)?;
        set_property(&[("page.cs".to_string(), text.to_string())], &tree, node, prop, &value).map(|e| e.text).map_err(|r| r.key)
    };
    let once = set(TAGS, 3, "FontSize", "System.Double", "32").unwrap();
    assert_eq!(once, TAGS.replace("Tag(\"Two\"),", "Tag(\"Two\").Adapt(me => ((SkiaLabel)me).FontSize = 32),"));
    // Again: the value in that Adapt.
    assert_eq!(set(&once, 3, "FontSize", "System.Double", "40").unwrap(), once.replace("FontSize = 32)", "FontSize = 40)"));
    // The text through the parameter: that call's argument, as before.
    assert_eq!(set(TAGS, 3, "Text", "System.String", "Three").unwrap(), TAGS.replace("Tag(\"Two\")", "Tag(\"Three\")"));
    // A color on the call, and a `.WithX` the call already has.
    let colored = set(TAGS, 2, "TextColor", "DrawnUi.Color", "#FFFF0000").unwrap();
    assert_eq!(colored, TAGS.replace("Tag(\"One\"),", "Tag(\"One\").Adapt(me => ((SkiaLabel)me).TextColor = Color.Parse(\"#FF0000\")),"));
    let with = TAGS.replace("Tag(\"One\"),", "Tag(\"One\").WithMargin(4),");
    assert_eq!(set(&with, 2, "Margin", "DrawnUi.Views.Thickness", "8,2").unwrap(), with.replace("WithMargin(4)", "WithMargin(8, 2)"));
    // Inside `Card`, used twice: refused.
    assert_eq!(set(TAGS, 5, "FontSize", "System.Double", "9"), Err("src-shared-helper".into()));
}

const TSX_TAGS: &str = r##"function Tag({ text, ...rest }) {
  return <SkiaLabel Text={text} FontSize={18} {...rest} />;
}

function Plain({ text }) {
  return <SkiaLabel Text={text} FontSize={18} />;
}

function Card() {
  return (
    <SkiaShape>
      <SkiaLabel Text="title" FontSize={12} />
    </SkiaShape>
  );
}

export default function App() {
  return (
    <SkiaStack>
      <Tag text="One" />
      <Tag text="Two" />
      <Plain text="A" />
      <Plain text="B" />
      <Card />
      <Card />
    </SkiaStack>
  );
}
"##;

/// Two uses of one component: a value edited on one is a prop of that use when the component passes
/// its props on, never a change of the component's body; refused with why otherwise.
#[test]
fn a_tsx_value_changes_only_the_edited_use_of_a_component() {
    let n = |id, parent, t: &str| designer_protocol::Node { id, parent, type_name: t.into(), ..Default::default() };
    let tree = designer_protocol::Tree {
        nodes: vec![n(1, None, "SkiaStack"), n(2, Some(1), "SkiaLabel"), n(3, Some(1), "SkiaLabel"), n(4, Some(1), "SkiaLabel"), n(5, Some(1), "SkiaLabel"), n(6, Some(1), "SkiaShape"), n(7, Some(6), "SkiaLabel"), n(8, Some(1), "SkiaShape"), n(9, Some(8), "SkiaLabel")],
        ..Default::default()
    };
    let set = |text: &str, node, prop, t: &str, v: &str| {
        let value = value_from_runtime(t, v, v).map_err(|r| r.key)?;
        set_property(&[("App.tsx".to_string(), text.to_string())], &tree, node, prop, &value).map(|e| e.text).map_err(|r| r.key)
    };
    let once = set(TSX_TAGS, 3, "FontSize", "System.Double", "32").unwrap();
    assert_eq!(once, TSX_TAGS.replace("<Tag text=\"Two\" />", "<Tag text=\"Two\" FontSize={32} />"));
    assert_eq!(set(&once, 3, "FontSize", "System.Double", "40").unwrap(), once.replace("FontSize={32}", "FontSize={40}"));
    assert_eq!(set(TSX_TAGS, 3, "Text", "System.String", "Three").unwrap(), TSX_TAGS.replace("text=\"Two\"", "text=\"Three\""));
    assert_eq!(set(TSX_TAGS, 5, "FontSize", "System.Double", "9"), Err("src-component-no-prop".into()));
    assert_eq!(set(TSX_TAGS, 7, "FontSize", "System.Double", "9"), Err("src-shared-helper".into()));
}
