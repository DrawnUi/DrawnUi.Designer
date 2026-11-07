//! A copy of each kind of control the sample app has, as the agent reads it (`Copy`) and the designer
//! writes it into the sample's code (Paste, Duplicate): the edited sample still compiles.

use designer_protocol::{ControlSpec, Node, Tree};
use drawnui::Aria;
use drawnui::prelude::*;

const SAMPLE: &str = include_str!("../sample/src/main.rs");

const TEXT: Color = Color::from_rgb(236, 239, 245);
const MUTED: Color = Color::from_rgb(150, 160, 180);
const CARD: Color = Color::from_rgb(34, 38, 52);
const ACCENT: Color = Color::from_rgb(64, 132, 246);
const QUIET: Color = Color::from_rgb(62, 70, 92);

#[derive(Default)]
struct State;

fn swatch(color: Color, name: &str) -> Build<SkiaShape> {
    SkiaShape::new().corner_radius(10).width_request(44).height_request(44).background_color(color).tag(name)
}

/// The sample's `build`, its controls with the same values (no handlers: they set no property).
fn sample_build(_s: &mut State) -> Build<SkiaLayout> {
    SkiaLayout::column().spacing(18).padding(28).children((
        SkiaLabel::new("Designer sample").font_size(28).text_color(TEXT).tag("title").accessibility_role(Aria::HEADING),
        SkiaLabel::new("Open DrawnUI Designer: this window's controls show there, and edits apply live.").font_size(15).text_color(MUTED).accessibility_role(Aria::TEXT),
        SkiaLayout::row().spacing(10).tag("swatches").children((swatch(ACCENT, "blue"), swatch(Color::from_rgb(242, 104, 84), "coral"), swatch(Color::from_rgb(76, 186, 120), "green"))),
        SkiaShape::new()
            .corner_radius(16)
            .padding(20)
            .background_color(CARD)
            .stroke_color(Color::from_rgb(70, 80, 108))
            .stroke_width(1)
            .tag("card")
            .children((SkiaLayout::column().spacing(6).children((
                SkiaLabel::new("Taps").font_size(13).text_color(MUTED),
                SkiaLabel::new("0").font_size(40).text_color(Color::from_rgb(242, 104, 84)).tag("count").accessibility_role(Aria::STATUS).accessibility_live(Aria::LIVE_POLITE),
            )),)),
        SkiaLayout::row().spacing(12).accessibility_role(Aria::TOOLBAR).accessibility_label("Taps").children((
            SkiaButton::new("Add one").background_color(ACCENT).text_color(Color::WHITE).accessibility_hint("Adds one to the taps"),
            SkiaButton::new("Reset").background_color(QUIET).text_color(Color::WHITE).accessibility_hint("Sets the taps back to zero"),
        )),
        SkiaLayout::row().spacing(12).children((
            SkiaSwitch::new().tag("menu switch").accessibility_label("Quick actions").vertical_options(LayoutOptions::Center).color_frame_on(ACCENT).color_frame_off(QUIET),
            SkiaLabel::new("Quick actions").font_size(15).text_color(MUTED).vertical_options(LayoutOptions::Center).accessibility_role(Aria::PRESENTATION),
        )),
    ))
}

/// The sample's tree as the designer matches it with its code (ids from 1, depth first).
fn sample_tree(t: &mut drawnui::Tree) -> Tree {
    fn walk(t: &mut drawnui::Tree, id: drawnui::ControlId, parent: Option<u64>, out: &mut Vec<Node>) {
        let me = out.len() as u64 + 1;
        out.push(Node { id: me, parent, type_name: t.type_name(id).unwrap_or_default().to_owned(), ..Default::default() });
        for c in crate::controls::app_children(t, id) {
            walk(t, c, Some(me), out);
        }
    }
    let mut nodes = vec![];
    let root = t.root().expect("a root");
    walk(t, root, None, &mut nodes);
    Tree { revision: 1, nodes, ..Default::default() }
}

/// `spec` as nodes of `tree`, under `parent`.
fn add_nodes(tree: &mut Tree, spec: &ControlSpec, parent: u64) {
    let id = tree.nodes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
    tree.nodes.push(Node { id, parent: Some(parent), type_name: spec.type_name.clone(), ..Default::default() });
    for c in &spec.children {
        add_nodes(tree, c, id);
    }
}

/// Every property a copy carries (more than a paste writes: the hub keeps those the code sets) of
/// the title, the swatches, the card, the toolbar and the switch row, pasted at the end of the
/// window's children: `cargo check` of the sample with them passes.
#[test]
fn copies_of_the_samples_controls_compile() {
    let mut ui = Ui::new(State, sample_build);
    let t = &mut ui.tree;
    let mut tree = sample_tree(t);
    let root = t.root().unwrap();
    let kids = crate::controls::app_children(t, root);
    let mut text = SAMPLE.to_owned();
    for index in [0, 2, 3, 4, 5] {
        let spec = crate::controls::spec(t, kids[index]).expect("a copy");
        let files = [("C:/app/src/main.rs".to_owned(), text.clone())];
        let at = tree.nodes.iter().filter(|n| n.parent == Some(1)).count();
        let edit = designer_sources::insert_spec(&files, &tree, 1, at, &spec, &[], &Default::default()).unwrap_or_else(|r| panic!("copy of child {index}: {r:?}"));
        text = edit.text;
        add_nodes(&mut tree, &spec, 1);
    }
    check_sample(&text);
}

/// `cargo check` of the sample crate with `main` as its main.rs (a copy of the crate in a temp
/// folder, built into this workspace's target folder).
fn check_sample(main: &str) {
    let agent = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let sample = agent.join("sample");
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("designer-copies-{}-{n}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let slash = |p: &std::path::Path| p.display().to_string().replace('\\', "/");
    let manifest = std::fs::read_to_string(sample.join("Cargo.toml")).unwrap();
    let manifest = manifest.replace("name = \"designer-agent-sample\"", "name = \"designer-copies-check\"").replace("path = \"..\"", &format!("path = \"{}\"", slash(agent))) + "\n[workspace]\n";
    std::fs::write(dir.join("Cargo.toml"), manifest).unwrap();
    for f in ["build.rs", "app.rc", "icon.ico"] {
        std::fs::copy(sample.join(f), dir.join(f)).unwrap();
    }
    std::fs::copy(agent.join("Cargo.lock"), dir.join("Cargo.lock")).unwrap();
    let font = slash(&agent.join("../../crates/designer-desktop/assets/OpenSans-Regular.ttf"));
    std::fs::write(dir.join("src/main.rs"), main.replace("../../../../crates/designer-desktop/assets/OpenSans-Regular.ttf", &font)).unwrap();
    let out = std::process::Command::new(env!("CARGO"))
        .args(["check", "--offline", "--quiet", "--message-format", "short"])
        .current_dir(&dir)
        .env("CARGO_TARGET_DIR", agent.join("target"))
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "the sample with the copies does not compile:\n{}\n{main}", String::from_utf8_lossy(&out.stderr));
}

/// A value other than `now` for a property of the type the agent names `type_name`, as an editor
/// sends it; `None` when the designer offers none.
fn other_value(type_name: &str, now: &str, choices: Option<&Vec<String>>) -> Option<String> {
    Some(match type_name {
        "System.Single" | "System.Double" | "System.Int32" | "System.Int64" => if now == "3" { "4" } else { "3" }.to_owned(),
        "System.Boolean" => if now == "true" { "false" } else { "true" }.to_owned(),
        "System.String" => "Copy".to_owned(),
        "drawnui.skia.Color" => "#FF112233".to_owned(),
        "Thickness" => "1,2,3,4".to_owned(),
        "CornerRadius" => "5,5,5,5".to_owned(),
        _ => choices?.iter().find(|c| !c.is_empty() && *c != now && !c.eq_ignore_ascii_case("none"))?.clone(),
    })
}

/// Every control the agent can make, each settable property set to another value, copied and
/// written into the sample (a few per check, the window's tuple takes 12): it compiles. A property
/// the code writer cannot spell is left out of the copy by the writer, never written wrong.
#[test]
fn copies_of_every_control_with_every_value_compile() {
    let mut ui = Ui::new(State, sample_build);
    let t = &mut ui.tree;
    let root = t.root().unwrap();
    let mut specs = vec![];
    for ty in drawnui::designer::control_types() {
        let Some(made) = drawnui::designer::create(ty) else { continue };
        let id = t.cx().insert_child(root, 0, made);
        for p in t.props(id).into_iter().filter(|p| p.settable) {
            let shown = crate::agent::prop(p.clone());
            if let Some(v) = other_value(&shown.type_name, &shown.value, shown.choices.as_ref()) {
                let _ = super::agent::set_property_for_tests(t, id, &shown.name, &v);
            }
        }
        specs.push(crate::controls::spec(t, id).expect("a copy"));
    }
    let mut tree = { let mut u = Ui::new(State, sample_build); sample_tree(&mut u.tree) };
    let base = tree.clone();
    let mut text = SAMPLE.to_owned();
    for (i, spec) in specs.iter().enumerate() {
        let files = [("C:/app/src/main.rs".to_owned(), text.clone())];
        let at = tree.nodes.iter().filter(|n| n.parent == Some(1)).count();
        let edit = designer_sources::insert_spec(&files, &tree, 1, at, spec, &[], &Default::default()).unwrap_or_else(|r| panic!("copy of {}: {r:?}", spec.type_name));
        text = edit.text;
        add_nodes(&mut tree, spec, 1);
        if (i + 1) % 5 == 0 || i + 1 == specs.len() {
            check_sample(&text);
            (text, tree) = (SAMPLE.to_owned(), base.clone());
        }
    }
}

/// The sample app itself (its window opens): a control inserted and taken out again, as Undo of a
/// Duplicate does; the app answers the removal with its tree. Run by hand on each desktop:
/// `cargo test real_sample -- --ignored`.
#[test]
#[ignore]
fn real_sample_answers_a_removal_with_its_tree() {
    use std::net::TcpStream;
    use tungstenite::{Message, WebSocket};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let agent = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut app = std::process::Command::new(env!("CARGO")).args(["run", "-q", "-p", "designer-agent-sample"]).current_dir(agent).env("DRAWNUI_DESIGNER_URL", format!("ws://127.0.0.1:{port}")).spawn().unwrap();
    let (stream, _) = listener.accept().unwrap();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    let mut ws = tungstenite::accept(stream).unwrap();
    // The app's messages until one `of` type (`None`: nothing came in 10 s).
    let next = |ws: &mut WebSocket<TcpStream>, of: &str| -> Option<serde_json::Value> {
        loop {
            match ws.read() {
                Ok(Message::Text(t)) => {
                    let v: serde_json::Value = serde_json::from_str(&t).unwrap();
                    if v["type"] == of {
                        return Some(v);
                    }
                }
                Ok(_) => {}
                Err(_) => return None,
            }
        }
    };
    let send = |ws: &mut WebSocket<TcpStream>, v: serde_json::Value| ws.send(Message::text(v.to_string())).unwrap();
    send(&mut ws, serde_json::json!({ "type": "getTree" }));
    let tree = next(&mut ws, "tree").expect("the tree");
    let (root, count) = (tree["nodes"][0]["id"].as_u64().unwrap(), tree["nodes"].as_array().unwrap().len());
    send(&mut ws, serde_json::json!({ "type": "insert", "request": 1, "parent": root, "index": 1, "typeName": "SkiaLabel", "props": [["text", "Copy"]], "children": [] }));
    let copy = next(&mut ws, "inserted").and_then(|m| m["node"].as_u64()).expect("inserted");
    let with = loop {
        let t = next(&mut ws, "tree").expect("the tree with the copy");
        if t["nodes"].as_array().unwrap().iter().any(|n| n["id"] == copy) {
            break t;
        }
    };
    assert_eq!(with["nodes"].as_array().unwrap().len(), count + 1);
    send(&mut ws, serde_json::json!({ "type": "remove", "request": 2, "node": copy }));
    let after = next(&mut ws, "tree");
    let _ = app.kill();
    let _ = app.wait();
    let after = after.expect("a tree after the removal");
    assert_eq!(after["nodes"].as_array().unwrap().len(), count, "the copy is gone from the tree");
}
