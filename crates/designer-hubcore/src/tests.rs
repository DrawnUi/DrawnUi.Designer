//! The hub's logic with files in memory and no sockets: what a browser page's hub does.

use super::*;

/// Files in memory; the stamp is (size, version), the version rises on every write.
#[derive(Default)]
struct MemStore(Mutex<HashMap<String, (Vec<u8>, u64)>>);

impl MemStore {
    fn with(files: &[(&str, &str)]) -> Arc<MemStore> {
        let store = MemStore::default();
        for (uri, text) in files {
            store.0.lock().unwrap().insert(uri.to_string(), (text.as_bytes().to_vec(), 1));
        }
        Arc::new(store)
    }

    fn text(&self, uri: &str) -> String {
        String::from_utf8(self.0.lock().unwrap()[uri].0.clone()).unwrap()
    }
}

impl CodeStore for MemStore {
    fn list(&self, dir: &Path) -> Vec<(String, Stamp)> {
        let files = self.0.lock().unwrap();
        files.iter().filter(|(u, _)| Path::new(u).starts_with(dir) && u.ends_with(".cs")).map(|(u, (b, v))| (u.clone(), (b.len() as u64, *v))).collect()
    }

    fn read(&self, uri: &str) -> Result<Vec<u8>, String> {
        self.0.lock().unwrap().get(uri).map(|(b, _)| b.clone()).ok_or_else(|| "not found".into())
    }

    fn write(&self, uri: &str, bytes: &[u8]) -> Result<(), String> {
        let mut files = self.0.lock().unwrap();
        let version = files.get(uri).map_or(1, |(_, v)| v + 1);
        files.insert(uri.into(), (bytes.to_vec(), version));
        Ok(())
    }

    fn dir_exists(&self, dir: &Path) -> bool {
        self.0.lock().unwrap().keys().any(|u| Path::new(u).starts_with(dir))
    }
}

const PAGE: &str = "namespace Demo;\npublic class Page : SkiaLayer\n{\n    public Page()\n    {\n        Children = new List<SkiaControl>\n        {\n            new SkiaLabel { Text = \"Hi\", FontSize = 18 },\n        };\n    }\n}\n";

fn info(name: &str, pid: u32, project: Option<&str>) -> AppInfo {
    AppInfo { protocol: PROTOCOL_VERSION.into(), name: name.into(), engine: "test".into(), pid, project: project.map(Into::into), agent: None, platform: None }
}

fn tree() -> Tree {
    let nodes = vec![
        Node { id: 1, type_name: "Demo.Page".into(), ..Default::default() },
        Node { id: 2, parent: Some(1), type_name: "DrawnUi.Draw.SkiaLabel".into(), ..Default::default() },
        Node { id: 3, parent: Some(2), type_name: "DrawnUi.Draw.SkiaLayout".into(), internal: true, ..Default::default() },
    ];
    Tree { revision: 1, width: 100.0, height: 50.0, nodes }
}

fn prop(name: &str, t: &str, v: &str) -> Prop {
    Prop { name: name.into(), type_name: t.into(), value: v.into(), is_set: true, ..Default::default() }
}

/// A hub with the demo page in memory and one app of it that reported its tree and its label's properties.
fn demo() -> (HubCore, Arc<MemStore>, AppId) {
    let store = MemStore::with(&[("/demo/Page.cs", PAGE)]);
    let mut core = HubCore::new(store.clone());
    let (app, _) = core.app_hello(info("Demo", 1, Some("/demo/Demo.csproj"))).unwrap();
    ap(&mut core, app, AppMsg::Tree(tree()));
    let props = vec![prop("FontSize", "System.Double", "18"), prop("Text", "System.String", "Hi")];
    ap(&mut core, app, AppMsg::Props { node: 2, props });
    (core, store, app)
}

/// An app's message, its work done at once.
fn ap(core: &mut HubCore, app: AppId, msg: AppMsg) -> Vec<Out> {
    let outs = core.app_msg(app, msg);
    core.run_inline(outs)
}

/// An editor's message, its work done at once.
fn ed(core: &mut HubCore, editor: u32, msg: EditorMsg) -> Vec<Out> {
    let outs = core.editor_msg(editor, msg);
    core.run_inline(outs)
}

/// The messages one editor gets (its own and every-editor ones).
fn to_editor(outs: &[Out], editor: u32) -> Vec<&HubToEditor> {
    outs.iter()
        .filter_map(|o| match o {
            Out::ToEditor(to, m) if to.is_none_or(|t| t == editor) => Some(m),
            _ => None,
        })
        .collect()
}

#[test]
fn apps_say_hello_and_a_returning_app_keeps_its_id() {
    let mut core = HubCore::new(MemStore::with(&[]));
    assert!(core.app_hello(AppInfo { protocol: "999.0".into(), ..info("Old", 1, None) }).is_none(), "another major version");
    let (first, outs) = core.app_hello(info("Phone", 4181, None)).unwrap();
    let [Out::ToEditor(None, HubToEditor::Apps { apps, .. })] = outs.as_slice() else { panic!("{outs:?}") };
    assert_eq!(apps.iter().map(|a| a.id).collect::<Vec<_>>(), [first]);
    let outs = core.app_closed(first);
    assert!(matches!(to_editor(&outs, 0).as_slice(), [HubToEditor::Apps { apps, .. }] if apps.is_empty()));
    assert_eq!(core.app_hello(info("Phone", 4181, None)).unwrap().0, first, "the same id again");
    let (twin, _) = core.app_hello(info("Phone", 4181, None)).unwrap();
    assert_ne!(twin, first, "two alike apps at once keep apart");
}

#[test]
fn a_new_editor_gets_the_apps_and_what_they_reported() {
    let (mut core, _, app) = demo();
    ap(&mut core, app, AppMsg::Screens { screens: vec![] });
    core.devices = Some("ws://x".into());
    core.set_cables(Some(1), None);
    assert!(core.editor_hello("999.0").is_none());
    let (editor, outs) = core.editor_hello(PROTOCOL_VERSION).unwrap();
    assert!(outs.iter().all(|o| matches!(o, Out::ToEditor(Some(e), _) if *e == editor)));
    let got = to_editor(&outs, editor);
    assert!(matches!(got[0], HubToEditor::Apps { apps, .. } if apps[0].id == app));
    assert!(matches!(got[1], HubToEditor::Devices { url: Some(_), .. }));
    assert!(matches!(got[2], HubToEditor::FromApp { msg: AppMsg::Screens { .. }, .. }));
    assert_eq!(got[3], &HubToEditor::FromApp { app, msg: AppMsg::Tree(tree()) });
    assert_eq!(got[4], &HubToEditor::Cables { android: 1, ios: 0 }, "cables last");
    assert_eq!(got.len(), 5);
}

#[test]
fn app_messages_reach_editors_and_editor_requests_reach_the_app() {
    let (mut core, _, app) = demo();
    let (editor, _) = core.editor_hello(PROTOCOL_VERSION).unwrap();
    let outs = core.app_msg(app, AppMsg::HotReload { stage: "applied".into() });
    assert!(matches!(outs.as_slice(), [Out::ToEditor(None, HubToEditor::FromApp { .. })]));
    assert_eq!(core.apps[&app].hot_reload.as_ref().unwrap().0, "applied");
    core.app_msg(app, AppMsg::Error { request: None, message: "oops".into() });
    assert_eq!(core.apps[&app].notes, ["oops"]);

    let pick = HubToApp::Pick { request: 3, x: 1.0, y: 2.0 };
    let outs = core.editor_msg(editor, EditorMsg::ToApp { app, msg: pick.clone() });
    assert!(matches!(outs.as_slice(), [Out::ToApp(a, m)] if *a == app && *m == pick));
    assert!(core.editor_msg(editor, EditorMsg::ToApp { app: 99, msg: pick }).is_empty(), "no such app");
    assert!(core.app_msg(99, AppMsg::Tree(tree())).is_empty(), "a closed app says nothing");
}

#[test]
fn props_are_followed_by_what_the_code_sets() {
    let (mut core, _, app) = demo();
    let props = vec![prop("FontSize", "System.Double", "18")];
    let outs = core.app_msg(app, AppMsg::Props { node: 2, props });
    assert!(matches!(outs.as_slice(), [Out::ToEditor(None, HubToEditor::FromApp { .. }), Out::Work(_)]), "{outs:?}");
    let outs = core.run_inline(outs);
    assert_eq!(to_editor(&outs, 0)[1], &HubToEditor::CodeSet { app, node: 2, names: Some(vec!["Text".into(), "FontSize".into()]) });
    // A part the library builds: no code.
    let outs = ap(&mut core, app, AppMsg::Props { node: 3, props: vec![] });
    assert_eq!(to_editor(&outs, 0)[1], &HubToEditor::CodeSet { app, node: 3, names: None });
}

#[test]
fn set_code_writes_and_undo_takes_it_back() {
    let (mut core, store, app) = demo();
    let (editor, _) = core.editor_hello(PROTOCOL_VERSION).unwrap();
    let set = |request, prop: &str, value: &str| EditorMsg::SetCode { request, app, node: 2, prop: prop.into(), value: value.into() };
    let outs = ed(&mut core, editor, set(1, "FontSize", "20"));
    assert!(matches!(outs.as_slice(), [Out::ToEditor(Some(e), HubToEditor::CodeEdited { request: 1, node: 2, uri, .. })] if *e == editor && uri == "/demo/Page.cs"), "{outs:?}");
    assert_eq!(store.text("/demo/Page.cs"), PAGE.replace("FontSize = 18", "FontSize = 20"));

    let refused = ed(&mut core, editor, set(2, "Opacity", "0.5"));
    assert!(matches!(refused.as_slice(), [Out::ToEditor(_, HubToEditor::CodeRefused { request: 2, .. })]), "untyped: {refused:?}");
    let gone = core.editor_msg(editor, EditorMsg::SetCode { request: 3, app: 99, node: 2, prop: "FontSize".into(), value: "1".into() });
    assert!(matches!(gone.as_slice(), [Out::ToEditor(_, HubToEditor::CodeRefused { request: 3, .. })]));

    let undo = ed(&mut core, editor, EditorMsg::UndoCode { request: 1 });
    assert!(matches!(undo.as_slice(), [Out::ToEditor(_, HubToEditor::CodeUndone { request: 1, .. })]), "{undo:?}");
    assert_eq!(store.text("/demo/Page.cs"), PAGE);
    let again = ed(&mut core, editor, EditorMsg::UndoCode { request: 1 });
    assert!(matches!(again.as_slice(), [Out::ToEditor(_, HubToEditor::CodeRefused { .. })]), "nothing left to take back");

    // The span changed since: refused, and kept for when the file is put back.
    ed(&mut core, editor, set(4, "FontSize", "30"));
    let edited = store.text("/demo/Page.cs");
    store.write("/demo/Page.cs", edited.replace("= 30", "= 31").as_bytes()).unwrap();
    let refused = ed(&mut core, editor, EditorMsg::UndoCode { request: 4 });
    assert!(matches!(refused.as_slice(), [Out::ToEditor(_, HubToEditor::CodeRefused { .. })]));
    store.write("/demo/Page.cs", edited.as_bytes()).unwrap();
    let undo = ed(&mut core, editor, EditorMsg::UndoCode { request: 4 });
    assert!(matches!(undo.as_slice(), [Out::ToEditor(_, HubToEditor::CodeUndone { request: 4, .. })]), "{undo:?}");
}

#[test]
fn a_closed_editor_leaves_no_undo_behind() {
    let (mut core, _, app) = demo();
    let (editor, _) = core.editor_hello(PROTOCOL_VERSION).unwrap();
    let set = |request| EditorMsg::SetCode { request, app, node: 2, prop: "FontSize".into(), value: "20".into() };
    ed(&mut core, editor, set(1));
    // A write still running when its editor closes is not kept either.
    let pending = core.editor_msg(editor, set(2));
    core.editor_closed(editor);
    core.run_inline(pending);
    assert!(core.written.is_empty());
    assert!(core.take_written(editor, 1).is_none());
}

/// A DrawFiddle snippet with non-ASCII text before the label: UTF-16 offsets differ from bytes and chars.
const SNIPPET: &str = "// Café ☕ 🎨\nreturn new SkiaStack { Children = { new SkiaLabel { Text = \"Hi\" } } };\n";

/// What a browser editor does with an edit: UTF-16 code units.
fn apply_utf16(text: &str, edit: &page::Utf16Edit) -> String {
    let mut units: Vec<u16> = text.encode_utf16().collect();
    units.splice(edit.start..edit.end, edit.text.encode_utf16());
    String::from_utf16(&units).unwrap()
}

#[test]
fn utf16_edit_is_one_span_in_utf16_units() {
    use page::utf16_edit;
    assert_eq!(utf16_edit("same", "same"), None);
    let old = "é🎨 \"Hi\" 🎨";
    let edit = utf16_edit(old, "é🎨 \"Hello\" 🎨").unwrap();
    // é = 1 unit, 🎨 = 2 units (a surrogate pair), then ` "` = 2: "Hi" starts at 5, its "i" at 6.
    assert_eq!(edit, page::Utf16Edit { start: 6, end: 7, text: "ello".into() });
    for new in ["", "x", "é🎨", "🎨 \"Hi\" 🎨", "é🎨 \"Hi\" 🎨 é", "é🎉 \"Hi\" 🎨"] {
        let e = utf16_edit(old, new).unwrap();
        assert_eq!(apply_utf16(old, &e), new, "{old:?} -> {new:?}: {e:?}");
    }
    // A change inside a surrogate pair replaces the whole char, never half of it.
    assert_eq!(utf16_edit("🎨", "🎉").unwrap(), page::Utf16Edit { start: 0, end: 2, text: "🎉".into() });
}

#[test]
fn a_page_hub_writes_the_snippet_through_the_page() {
    let posted: Arc<Mutex<Vec<page::ToPage>>> = Arc::default();
    let sink = posted.clone();
    let code = Arc::new(page::PageCode::new(move |m| sink.lock().unwrap().push(m)));
    let mut core = HubCore::new(code.clone());
    core.page_project = Some(page::PROJECT.into());
    let (editor, _) = core.editor_hello(PROTOCOL_VERSION).unwrap();
    // The Fiddle's agent names no project; its tree: the Fiddle's layout -> the snippet's stack -> label.
    let (app, _) = core.app_hello(AppInfo { platform: Some("web".into()), ..info("FiddleSnippet", 1, None) }).unwrap();
    let n = |id, parent, t: &str| Node { id, parent, type_name: format!("DrawnUi.Draw.{t}"), ..Default::default() };
    ap(&mut core, app, AppMsg::Tree(Tree { revision: 1, width: 100.0, height: 50.0, nodes: vec![n(1, None, "SkiaLayout"), n(2, Some(1), "SkiaStack"), n(3, Some(2), "SkiaLabel")] }));
    let props = || AppMsg::Props { node: 3, props: vec![prop("Text", "System.String", "Hi")] };
    // No text from the page yet: nothing known.
    assert_eq!(to_editor(&ap(&mut core, app, props()), editor)[1], &HubToEditor::CodeSet { app, node: 3, names: None });
    assert!(matches!(core.apps_msg(), HubToEditor::Apps { recent, .. } if recent.is_empty()), "a snippet is never a recent app");

    code.set_text(SNIPPET.into(), 7);
    assert_eq!(to_editor(&ap(&mut core, app, props()), editor)[1], &HubToEditor::CodeSet { app, node: 3, names: Some(vec!["Text".into()]) });

    let set = |request, value: &str| EditorMsg::SetCode { request, app, node: 3, prop: "Text".into(), value: value.into() };
    let outs = ed(&mut core, editor, set(1, "Hello"));
    assert!(matches!(outs.as_slice(), [Out::ToEditor(_, HubToEditor::CodeEdited { request: 1, uri, .. })] if uri == page::SNIPPET), "{outs:?}");
    let edited = SNIPPET.replace("\"Hi\"", "\"Hello\"");
    let page::ToPage::Edit { version: 7, edits } = posted.lock().unwrap().pop().unwrap() else { panic!("{posted:?}", posted = posted.lock().unwrap()) };
    // UTF-16 offsets: é and ☕ are one unit each, 🎨 two (in bytes 2, 3 and 4).
    let at = SNIPPET[..SNIPPET.find("Hi").unwrap()].encode_utf16().count();
    assert_eq!(edits, [page::Utf16Edit { start: at + 1, end: at + 2, text: "ello".into() }]);
    assert_eq!(apply_utf16(SNIPPET, &edits[0]), edited);

    // A second write before the page took the first: refused, and the page is asked for its text.
    let refused = ed(&mut core, editor, set(2, "Bye"));
    assert!(matches!(refused.as_slice(), [Out::ToEditor(_, HubToEditor::CodeRefused { request: 2, .. })]), "{refused:?}");
    assert_eq!(posted.lock().unwrap().pop(), Some(page::ToPage::Get));

    // The page took it (its next version); Undo goes back through the page the same way.
    code.set_text(edited.clone(), 8);
    let undo = ed(&mut core, editor, EditorMsg::UndoCode { request: 1 });
    assert!(matches!(undo.as_slice(), [Out::ToEditor(_, HubToEditor::CodeUndone { request: 1, .. })]), "{undo:?}");
    let Some(page::ToPage::Edit { version: 8, edits }) = posted.lock().unwrap().pop() else { panic!() };
    assert_eq!(apply_utf16(&edited, &edits[0]), SNIPPET);
}

/// A control taken out live is taken out of the code with the tree from before (the app's tree
/// no longer has it), and Undo puts it back byte for byte.
#[test]
fn a_removed_control_is_taken_out_of_the_code_and_put_back() {
    let (mut core, store, app) = demo();
    let (editor, _) = core.editor_hello(PROTOCOL_VERSION).unwrap();
    ed(&mut core, editor, EditorMsg::ToApp { app, msg: HubToApp::Remove { request: 1, node: 2 } });
    let mut t = tree();
    t.nodes.retain(|n| n.id == 1);
    ap(&mut core, app, AppMsg::Tree(t));
    let outs = ed(&mut core, editor, EditorMsg::RemoveCode { request: 7, app, node: 2 });
    assert!(matches!(to_editor(&outs, editor).as_slice(), [HubToEditor::CodeEdited { request: 7, node: 2, .. }]), "{outs:?}");
    assert!(!store.text("/demo/Page.cs").contains("SkiaLabel"), "{}", store.text("/demo/Page.cs"));
    let outs = ed(&mut core, editor, EditorMsg::UndoCode { request: 7 });
    assert!(matches!(to_editor(&outs, editor).as_slice(), [HubToEditor::CodeUndone { request: 7, .. }]), "{outs:?}");
    assert_eq!(store.text("/demo/Page.cs"), PAGE);
}

// ---- Writes while other live changes wait (the code view) ----

const LIST: &str = "namespace Demo;\npublic class Page : SkiaLayer\n{\n    public Page()\n    {\n        Children = new List<SkiaControl>\n        {\n            new SkiaStack\n            {\n                Children = new List<SkiaControl>\n                {\n                    new SkiaLabel { Text = \"A\" },\n                    new SkiaLabel { Text = \"B\" },\n                    new SkiaLabel { Text = \"C\" },\n                },\n            },\n            new SkiaStack\n            {\n                Children = Enumerable.Range(0, 2).Select(i => (SkiaControl)new SkiaLabel { Text = $\"L{i}\" }).ToList(),\n            },\n        };\n    }\n}\n";

const LABEL: &str = "DrawnUi.Draw.SkiaLabel";
const STACK: &str = "DrawnUi.Draw.SkiaStack";

fn node(id: u64, parent: Option<u64>, type_name: &str) -> Node {
    Node { id, parent, type_name: type_name.into(), ..Default::default() }
}

/// Page 1; stack 2 with labels A 3, B 4, C 5; stack 6 (a loop) with labels 7, 8.
fn list_tree() -> Tree {
    let nodes = vec![node(1, None, "Demo.Page"), node(2, Some(1), STACK), node(3, Some(2), LABEL), node(4, Some(2), LABEL), node(5, Some(2), LABEL), node(6, Some(1), STACK), node(7, Some(6), LABEL), node(8, Some(6), LABEL)];
    Tree { revision: 1, width: 100.0, height: 100.0, nodes }
}

struct Session {
    core: HubCore,
    store: Arc<MemStore>,
    app: AppId,
    editor: u32,
    tree: Tree,
}

impl Session {
    fn new() -> Session {
        let store = MemStore::with(&[("/demo/Page.cs", LIST)]);
        let mut core = HubCore::new(store.clone());
        let (app, _) = core.app_hello(info("Demo", 1, Some("/demo/Demo.csproj"))).unwrap();
        let (editor, _) = core.editor_hello(PROTOCOL_VERSION).unwrap();
        let controls = vec![ControlKind { type_name: LABEL.into(), group: "Text".into(), defaults: vec![("Text".into(), "Label".into())], ..Default::default() }];
        ap(&mut core, app, AppMsg::Controls { controls });
        let tree = list_tree();
        ap(&mut core, app, AppMsg::Tree(tree.clone()));
        Session { core, store, app, editor, tree }
    }

    fn code(&self) -> String {
        self.store.text("/demo/Page.cs")
    }

    /// The labels' texts in the code, in order (the loop's left out).
    fn labels(&self) -> Vec<String> {
        let code = self.code();
        code.split("new SkiaLabel { Text = \"").skip(1).map(|s| s.split('"').next().unwrap().to_owned()).collect()
    }

    /// The app sends its tree as it is now.
    fn tree_now(&mut self) {
        let t = self.tree.clone();
        ap(&mut self.core, self.app, AppMsg::Tree(t));
    }

    /// An editor inserts a label with `text` live into `parent` at `index`; the app answers with `id`.
    fn insert(&mut self, request: u32, parent: u64, index: usize, id: u64, text: &str) {
        let props = vec![("Text".to_owned(), text.to_owned())];
        ed(&mut self.core, self.editor, EditorMsg::ToApp { app: self.app, msg: HubToApp::Insert { request, parent, index: Some(index as u32), type_name: LABEL.into(), props, children: vec![] } });
        self.place(parent, index, node(id, Some(parent), LABEL));
        ap(&mut self.core, self.app, AppMsg::Inserted { request, node: Some(id), parent: Some(parent), index: Some(index as u32) });
        self.tree_now();
        ap(&mut self.core, self.app, AppMsg::Props { node: id, props: vec![prop("Text", "System.String", text)] });
    }

    /// `n` as child `index` of `parent` in the app's tree.
    fn place(&mut self, parent: u64, index: usize, n: Node) {
        let kids: Vec<usize> = self.tree.nodes.iter().enumerate().filter(|(_, m)| m.parent == Some(parent)).map(|(i, _)| i).collect();
        let at = kids.get(index).copied().unwrap_or_else(|| kids.last().map_or_else(|| self.tree.nodes.iter().position(|m| m.id == parent).unwrap() + 1, |l| l + 1));
        self.tree.nodes.insert(at, n);
    }

    /// An editor takes `id` out live; the app's tree no longer has it.
    fn remove(&mut self, request: u32, id: u64) {
        ed(&mut self.core, self.editor, EditorMsg::ToApp { app: self.app, msg: HubToApp::Remove { request, node: id } });
        self.tree.nodes.retain(|n| n.id != id && n.parent != Some(id));
        self.tree_now();
    }

    fn answer(&mut self, msg: EditorMsg) -> HubToEditor {
        let editor = self.editor;
        let outs = ed(&mut self.core, editor, msg);
        let got = to_editor(&outs, editor);
        assert_eq!(got.len(), 1, "{outs:?}");
        got[0].clone()
    }

    fn write_insert(&mut self, request: u32, id: u64, parent: u64) -> HubToEditor {
        self.answer(EditorMsg::InsertCode { request, app: self.app, node: id, parent, index: 0, type_name: LABEL.into(), defaults: vec![], spec: None })
    }

    fn write_remove(&mut self, request: u32, id: u64) -> HubToEditor {
        self.answer(EditorMsg::RemoveCode { request, app: self.app, node: id })
    }

    fn undo(&mut self, request: u32) -> HubToEditor {
        self.answer(EditorMsg::UndoCode { request })
    }
}

fn edited(m: &HubToEditor) -> bool {
    matches!(m, HubToEditor::CodeEdited { .. })
}

fn refusal(m: &HubToEditor) -> String {
    match m {
        HubToEditor::CodeRefused { refusal: Some(r), .. } => r.key.clone(),
        m => panic!("not refused: {m:?}"),
    }
}

#[test]
fn insert_write_then_undo() {
    let mut s = Session::new();
    s.insert(10, 2, 1, 20, "N");
    let m = s.write_insert(11, 20, 2);
    assert!(edited(&m), "{m:?}");
    assert_eq!(s.labels(), ["A", "N", "B", "C"]);
    assert!(s.core.apps[&s.app].live.inserts.is_empty(), "written: in the code now");
    assert!(matches!(s.undo(11), HubToEditor::CodeUndone { .. }));
    assert_eq!(s.code(), LIST, "byte for byte");
    assert!(s.core.apps[&s.app].live.inserts.contains_key(&20), "live-only again until it is taken out");
    s.remove(12, 20);
    assert!(s.core.apps[&s.app].live.inserts.is_empty() && s.core.apps[&s.app].live.removes.is_empty(), "taking out an unwritten insert leaves nothing");
}

#[test]
fn two_inserts_written_in_any_order() {
    let mut s = Session::new();
    s.insert(10, 2, 3, 21, "X"); // after C
    s.insert(11, 2, 0, 22, "Y"); // before A
    assert!(edited(&s.write_insert(12, 21, 2)), "X first, while Y is only live");
    assert_eq!(s.labels(), ["A", "B", "C", "X"]);
    assert!(edited(&s.write_insert(13, 22, 2)));
    assert_eq!(s.labels(), ["Y", "A", "B", "C", "X"]);
}

#[test]
fn a_paste_next_to_an_unwritten_delete_then_both_written() {
    let mut s = Session::new();
    s.remove(10, 4); // B out live
    s.insert(11, 2, 1, 23, "Y"); // right after A in the app: A, Y, C
    let m = s.write_insert(12, 23, 2);
    assert!(edited(&m), "{m:?}");
    assert_eq!(s.labels(), ["A", "B", "Y", "C"], "B is still in the code, where it was");
    assert!(edited(&s.write_remove(13, 4)));
    assert_eq!(s.labels(), ["A", "Y", "C"], "the code now makes what the app shows");
    assert!(s.core.apps[&s.app].live.removes.is_empty() && s.core.apps[&s.app].live.inserts.is_empty());
    // Undo the removal: B back in the code, and absent from the app again.
    assert!(matches!(s.undo(13), HubToEditor::CodeUndone { .. }));
    assert_eq!(s.labels(), ["A", "B", "Y", "C"]);
    assert_eq!(s.core.apps[&s.app].live.removes.len(), 1);
}

#[test]
fn a_written_delete_then_a_paste_after_a_rebuild() {
    let mut s = Session::new();
    s.remove(10, 4);
    assert!(edited(&s.write_remove(11, 4)));
    assert_eq!(s.labels(), ["A", "C"]);
    // Hot reload: the page is built again from the code, every control with a new id.
    s.tree = Tree { revision: 2, width: 100.0, height: 100.0, nodes: vec![node(101, None, "Demo.Page"), node(102, Some(101), STACK), node(103, Some(102), LABEL), node(105, Some(102), LABEL), node(106, Some(101), STACK), node(107, Some(106), LABEL), node(108, Some(106), LABEL)] };
    s.tree_now();
    let live = &s.core.apps[&s.app].live;
    assert!(live.inserts.is_empty() && live.removes.is_empty());
    s.insert(12, 102, 1, 124, "Z");
    assert!(edited(&s.write_insert(13, 124, 102)));
    assert_eq!(s.labels(), ["A", "Z", "C"]);
}

#[test]
fn a_property_written_while_other_changes_wait() {
    let mut s = Session::new();
    s.insert(10, 2, 0, 20, "N"); // before A, not written
    ap(&mut s.core, s.app, AppMsg::Props { node: 4, props: vec![prop("Text", "System.String", "B")] });
    let m = s.answer(EditorMsg::SetCode { request: 11, app: s.app, node: 4, prop: "Text".into(), value: "B2".into() });
    assert!(edited(&m), "{m:?}");
    assert_eq!(s.labels(), ["A", "B2", "C"], "B's own literal, not its neighbour's");
    // And while a removal before it waits.
    s.remove(12, 3);
    ap(&mut s.core, s.app, AppMsg::Props { node: 5, props: vec![prop("Text", "System.String", "C")] });
    assert!(edited(&s.answer(EditorMsg::SetCode { request: 13, app: s.app, node: 5, prop: "Text".into(), value: "C2".into() })));
    assert_eq!(s.labels(), ["A", "B2", "C2"]);
    // An unwritten insert has no code: refused by name.
    assert_eq!(refusal(&s.answer(EditorMsg::SetCode { request: 14, app: s.app, node: 20, prop: "Text".into(), value: "x".into() })), "hub-not-in-code-yet");
}

#[test]
fn a_taken_out_control_put_back_is_in_the_code() {
    let mut s = Session::new();
    s.remove(10, 4);
    let outs = ed(&mut s.core, s.editor, EditorMsg::Restore { request: 11, app: s.app, node: 4, props: vec![("Text".into(), "B".into())], children: vec![] });
    assert!(matches!(outs.as_slice(), [Out::ToApp(_, HubToApp::Insert { parent: 2, index: Some(1), .. })]), "back between A and C: {outs:?}");
    s.place(2, 1, node(30, Some(2), LABEL));
    ap(&mut s.core, s.app, AppMsg::Inserted { request: 11, node: Some(30), parent: Some(2), index: Some(1) });
    s.tree_now();
    let live = &s.core.apps[&s.app].live;
    assert!(live.inserts.is_empty() && live.removes.is_empty(), "the app and the code agree again");
    ap(&mut s.core, s.app, AppMsg::Props { node: 30, props: vec![prop("Text", "System.String", "B")] });
    assert!(edited(&s.answer(EditorMsg::SetCode { request: 12, app: s.app, node: 30, prop: "Text".into(), value: "B3".into() })));
    assert_eq!(s.labels(), ["A", "B3", "C"]);
}

#[test]
fn writes_that_cannot_be_placed_say_why() {
    let mut s = Session::new();
    // Into the loop's stack: refused by the writer with its own reason, the code untouched.
    s.insert(10, 6, 2, 25, "L");
    let key = refusal(&s.write_insert(11, 25, 6));
    assert!(key.starts_with("src-insert-"), "{key}");
    assert_eq!(s.code(), LIST);
    // A child of an unwritten insert.
    ed(&mut s.core, s.editor, EditorMsg::ToApp { app: s.app, msg: HubToApp::Insert { request: 12, parent: 2, index: Some(0), type_name: STACK.into(), props: vec![], children: vec![] } });
    s.place(2, 0, node(40, Some(2), STACK));
    ap(&mut s.core, s.app, AppMsg::Inserted { request: 12, node: Some(40), parent: Some(2), index: Some(0) });
    s.tree_now();
    s.insert(13, 40, 0, 41, "In");
    assert_eq!(refusal(&s.write_insert(14, 41, 40)), "hub-parent-not-in-code");
    // Written twice.
    s.insert(16, 2, 0, 26, "W");
    assert!(edited(&s.write_insert(17, 26, 2)));
    assert_eq!(refusal(&s.write_insert(18, 26, 2)), "hub-in-code-already");
    // An unwritten insert cannot be taken out of the code.
    assert_eq!(refusal(&s.write_remove(19, 41)), "hub-not-in-code-yet");
}

#[test]
fn place_mode_inserts_are_recorded_with_their_type_defaults() {
    let mut s = Session::new();
    ed(&mut s.core, s.editor, EditorMsg::ToApp { app: s.app, msg: HubToApp::PlaceMode { type_name: Some(LABEL.into()) } });
    s.place(2, 3, node(50, Some(2), LABEL));
    ap(&mut s.core, s.app, AppMsg::Inserted { request: 0, node: Some(50), parent: Some(2), index: Some(3) });
    s.tree_now();
    let recorded = s.core.apps[&s.app].live.inserts.get(&50).map(|spec| spec.props.iter().map(|p| (p.name.clone(), p.value.clone())).collect::<Vec<_>>());
    assert_eq!(recorded, Some(vec![("Text".to_owned(), "Label".to_owned())]));
    ap(&mut s.core, s.app, AppMsg::Props { node: 50, props: vec![prop("Text", "System.String", "Label")] });
    assert!(edited(&s.write_insert(51, 50, 2)), "empty defaults: the ones it was inserted with");
    assert_eq!(s.labels(), ["A", "B", "C", "Label"]);
}

/// A pasted copy with what is inside it is written whole, nested, and its undo is byte for byte.
#[test]
fn a_pasted_chip_is_written_with_its_label() {
    let mut s = Session::new();
    let p = |name: &str, value: &str, t: &str| SpecProp { name: name.into(), value: value.into(), type_name: t.into() };
    let label = ControlSpec { type_name: LABEL.into(), props: vec![p("Text", "Start", "System.String")], children: vec![] };
    let props = vec![("CornerRadius".to_owned(), "10".to_owned()), ("BackgroundColor".to_owned(), "#FF2B3035".to_owned())];
    ed(&mut s.core, s.editor, EditorMsg::ToApp { app: s.app, msg: HubToApp::Insert { request: 10, parent: 2, index: Some(1), type_name: "DrawnUi.Draw.SkiaShape".into(), props, children: vec![label.clone()] } });
    s.place(2, 1, node(60, Some(2), "DrawnUi.Draw.SkiaShape"));
    let at = s.tree.nodes.iter().position(|n| n.id == 60).unwrap();
    s.tree.nodes.insert(at + 1, node(61, Some(60), LABEL));
    ap(&mut s.core, s.app, AppMsg::Inserted { request: 10, node: Some(60), parent: Some(2), index: Some(1) });
    s.tree_now();
    ap(&mut s.core, s.app, AppMsg::Props { node: 60, props: vec![prop("CornerRadius", "System.Double", "10"), prop("BackgroundColor", "DrawnUi.Color", "#FF2B3035")] });
    // As the hub recorded it (no spec from the editor).
    let m = s.write_insert(11, 60, 2);
    assert!(edited(&m), "{m:?}");
    let code = s.code();
    assert!(
        code.contains("new SkiaLabel { Text = \"A\" },\n                    new SkiaShape\n                    {\n                        CornerRadius = 10,\n                        BackgroundColor = Color.Parse(\"#2B3035\"),\n                        Children =\n                        {\n                            new SkiaLabel { Text = \"Start\" },\n                        },\n                    },\n                    new SkiaLabel { Text = \"B\" },"),
        "{code}"
    );
    assert!(matches!(s.undo(11), HubToEditor::CodeUndone { .. }));
    assert_eq!(s.code(), LIST);
    // The editor's own spec, with every type in it: no cached properties needed.
    let spec = ControlSpec { type_name: "DrawnUi.Draw.SkiaShape".into(), props: vec![p("CornerRadius", "12", "DrawnUi.Draw.CornerRadius")], children: vec![label] };
    let m = s.answer(EditorMsg::InsertCode { request: 12, app: s.app, node: 60, parent: 2, index: 0, type_name: String::new(), defaults: vec![], spec: Some(spec) });
    assert!(edited(&m), "{m:?}");
    assert!(s.code().contains("new SkiaShape\n                    {\n                        CornerRadius = 12,\n                        Children =\n                        {\n                            new SkiaLabel { Text = \"Start\" },\n                        },\n                    },"), "{}", s.code());
}

/// A pasted copy is written as its source's code has it: values the library sets for itself (that
/// the app reads as set) stay out; a value changed live in the designer stays in.
#[test]
fn a_pasted_copy_keeps_only_what_its_code_and_the_designer_set() {
    let mut s = Session::new();
    let p = |name: &str, value: &str, t: &str| SpecProp { name: name.into(), value: value.into(), type_name: t.into() };
    // FontSize changed live on A, then A copied.
    ed(&mut s.core, s.editor, EditorMsg::ToApp { app: s.app, msg: HubToApp::SetProperty { request: 9, node: 3, name: "FontSize".into(), value: "30".into() } });
    ed(&mut s.core, s.editor, EditorMsg::ToApp { app: s.app, msg: HubToApp::Copy { request: 10, node: 3 } });
    let spec = ControlSpec { type_name: LABEL.into(), props: vec![p("FontSize", "30", "System.Double"), p("Text", "A", "System.String"), p("UseCache", "Operations", "DrawnUi.Draw.SkiaCacheType")], children: vec![] };
    ap(&mut s.core, s.app, AppMsg::Copied { request: 10, spec: Some(spec.clone()) });
    // Pasted after C, then written.
    let props = spec.props.iter().map(|p| (p.name.clone(), p.value.clone())).collect();
    ed(&mut s.core, s.editor, EditorMsg::ToApp { app: s.app, msg: HubToApp::Insert { request: 11, parent: 2, index: Some(3), type_name: LABEL.into(), props, children: vec![] } });
    s.place(2, 3, node(70, Some(2), LABEL));
    ap(&mut s.core, s.app, AppMsg::Inserted { request: 11, node: Some(70), parent: Some(2), index: Some(3) });
    s.tree_now();
    let m = s.answer(EditorMsg::InsertCode { request: 12, app: s.app, node: 70, parent: 2, index: 0, type_name: LABEL.into(), defaults: vec![], spec: Some(spec) });
    assert!(edited(&m), "{m:?}");
    assert!(s.code().contains("new SkiaLabel { Text = \"C\" },\n                    new SkiaLabel { FontSize = 30, Text = \"A\" },"), "{}", s.code());
    assert!(!s.code().contains("UseCache"));
}

// ---- Live changes kept across a rebuild (hot reload) ----

impl Session {
    /// The app builds its controls again from the code: page 101, stack 102 with the labels the code
    /// has now (ids 110 up), the loop's stack 106 with 107, 108.
    fn rebuild(&mut self) {
        let mut nodes = vec![node(101, None, "Demo.Page"), node(102, Some(101), STACK)];
        for i in 0..self.labels().len() {
            nodes.push(node(110 + i as u64, Some(102), LABEL));
        }
        nodes.extend([node(106, Some(101), STACK), node(107, Some(106), LABEL), node(108, Some(106), LABEL)]);
        self.tree = Tree { revision: self.tree.revision + 1, width: 100.0, height: 100.0, nodes };
    }

    /// The app sends its rebuilt tree: what the hub sends back (to editors and to the app).
    fn rebuilt(&mut self) -> Vec<Out> {
        let t = self.tree.clone();
        self.core.app_msg(self.app, AppMsg::Tree(t))
    }

    fn set_live(&mut self, request: u32, node: u64, name: &str, value: &str) {
        ed(&mut self.core, self.editor, EditorMsg::ToApp { app: self.app, msg: HubToApp::SetProperty { request, node, name: name.into(), value: value.into() } });
    }
}

fn replayed(outs: &[Out]) -> Option<(Vec<(u64, u64)>, Vec<u64>, Vec<String>)> {
    outs.iter().find_map(|o| match o {
        Out::ToEditor(None, HubToEditor::Replayed { moved, replaying, lost, .. }) => Some((moved.clone(), replaying.clone(), lost.iter().map(|r| r.key.clone()).collect())),
        _ => None,
    })
}

fn to_app(outs: &[Out]) -> Vec<HubToApp> {
    outs.iter().filter_map(|o| if let Out::ToApp(_, m) = o { Some(m.clone()) } else { None }).collect()
}

#[test]
fn values_set_live_and_not_written_are_put_back_after_a_rebuild() {
    let mut s = Session::new();
    // Paste style on B: three values live; one written.
    for (i, (name, value)) in [("FontSize", "13"), ("TextColor", "#FFFF0000"), ("Padding", "12,8,12,8")].into_iter().enumerate() {
        s.set_live(10 + i as u32, 4, name, value);
    }
    ap(&mut s.core, s.app, AppMsg::Props { node: 4, props: vec![prop("FontSize", "System.Double", "13"), prop("TextColor", "DrawnUi.Color", "#FFFF0000"), prop("Padding", "DrawnUi.Views.Thickness", "12,8,12,8"), prop("Text", "System.String", "B")] });
    assert!(edited(&s.answer(EditorMsg::SetCode { request: 20, app: s.app, node: 4, prop: "FontSize".into(), value: "13".into() })));
    // The write's hot reload: B is 111 now. The two other values go back onto it.
    s.rebuild();
    let outs = s.rebuilt();
    let (moved, replaying, lost) = replayed(&outs).expect("Replayed");
    assert_eq!((moved, replaying, lost), (vec![(4, 111)], vec![], vec![]));
    let set: Vec<(u64, String, String)> = to_app(&outs).into_iter().filter_map(|m| if let HubToApp::SetProperty { node, name, value, .. } = m { Some((node, name, value)) } else { None }).collect();
    assert_eq!(set, [(111, "TextColor".to_owned(), "#FFFF0000".to_owned()), (111, "Padding".to_owned(), "12,8,12,8".to_owned())], "not FontSize: it is in the code");
    // Editors hear it before the tree.
    let first = outs.iter().position(|o| matches!(o, Out::ToEditor(None, HubToEditor::Replayed { .. }))).unwrap();
    let tree_at = outs.iter().position(|o| matches!(o, Out::ToEditor(None, HubToEditor::FromApp { msg: AppMsg::Tree(_), .. }))).unwrap();
    assert!(first < tree_at);
    // They can still be written.
    ap(&mut s.core, s.app, AppMsg::Props { node: 111, props: vec![prop("TextColor", "DrawnUi.Color", "#FFFF0000"), prop("Text", "System.String", "B")] });
    assert!(edited(&s.answer(EditorMsg::SetCode { request: 21, app: s.app, node: 111, prop: "TextColor".into(), value: "#FFFF0000".into() })));
    assert!(s.code().contains("new SkiaLabel { Text = \"B\", FontSize = 13, TextColor = Color.Parse(\"#FF0000\") },"), "{}", s.code());
}

#[test]
fn an_unwritten_insert_survives_the_rebuild_of_another_write() {
    let mut s = Session::new();
    s.insert(10, 2, 1, 20, "X"); // after A, not written
    s.insert(11, 2, 4, 21, "Y"); // after C
    assert!(edited(&s.write_insert(12, 21, 2)));
    assert_eq!(s.labels(), ["A", "B", "C", "Y"]);
    // The rebuild has A, B, C, Y (110..113) and no X: X goes back after A.
    s.rebuild();
    let outs = s.rebuilt();
    let (_, replaying, lost) = replayed(&outs).expect("Replayed");
    assert_eq!((replaying, lost), (vec![20], vec![]));
    let Some(HubToApp::Insert { request, parent: 102, index: Some(1), props, .. }) = to_app(&outs).into_iter().next() else { panic!("{outs:?}") };
    assert_eq!(props, [("Text".to_owned(), "X".to_owned())]);
    // The app answers: editors hear the new id; X can be written.
    s.place(102, 1, node(130, Some(102), LABEL));
    let outs = s.core.app_msg(s.app, AppMsg::Inserted { request, node: Some(130), parent: Some(102), index: Some(1) });
    assert_eq!(replayed(&outs).map(|r| r.0), Some(vec![(20, 130)]));
    s.tree_now();
    ap(&mut s.core, s.app, AppMsg::Props { node: 130, props: vec![prop("Text", "System.String", "X")] });
    assert!(edited(&s.write_insert(13, 130, 102)));
    assert_eq!(s.labels(), ["A", "X", "B", "C", "Y"]);
}

#[test]
fn an_unwritten_removal_is_taken_out_again_after_a_rebuild() {
    let mut s = Session::new();
    s.remove(10, 4); // B out live
    s.insert(11, 2, 2, 21, "Y"); // after C in the app (A, C, Y)
    assert!(edited(&s.write_insert(12, 21, 2)));
    assert_eq!(s.labels(), ["A", "B", "C", "Y"]);
    s.rebuild(); // A 110, B 111, C 112, Y 113: B is back
    let outs = s.rebuilt();
    let (moved, _, lost) = replayed(&outs).expect("Replayed");
    assert!(lost.is_empty() && moved.contains(&(4, 111)) && moved.contains(&(2, 102)), "{moved:?}");
    assert!(to_app(&outs).iter().any(|m| matches!(m, HubToApp::Remove { node: 111, .. })), "{outs:?}");
    s.tree.nodes.retain(|n| n.id != 111);
    s.tree_now();
    assert!(edited(&s.write_remove(13, 111)));
    assert_eq!(s.labels(), ["A", "C", "Y"]);
}

#[test]
fn a_change_whose_place_left_the_code_is_reported() {
    let mut s = Session::new();
    s.set_live(10, 5, "FontSize", "30"); // on C
    // Someone takes C out of the code by hand; the app is rebuilt without it.
    let code = s.code().replace("                    new SkiaLabel { Text = \"C\" },\n", "");
    s.store.write("/demo/Page.cs", code.as_bytes()).unwrap();
    s.rebuild();
    let outs = s.rebuilt();
    let (_, _, lost) = replayed(&outs).expect("Replayed");
    assert_eq!(lost, ["hub-lost-prop"]);
    assert!(to_app(&outs).is_empty());
}

#[test]
fn a_copy_keeps_its_own_list_form() {
    let mut s = Session::new();
    let p = |name: &str, value: &str, t: &str| SpecProp { name: name.into(), value: value.into(), type_name: t.into() };
    // The stack (its code: `Children = new List<SkiaControl> { .. }`) copied whole and pasted after it.
    ed(&mut s.core, s.editor, EditorMsg::ToApp { app: s.app, msg: HubToApp::Copy { request: 10, node: 2 } });
    let label = |t: &str| ControlSpec { type_name: LABEL.into(), props: vec![p("Text", t, "System.String")], children: vec![] };
    let spec = ControlSpec { type_name: STACK.into(), props: vec![], children: vec![label("A"), label("B"), label("C")] };
    ap(&mut s.core, s.app, AppMsg::Copied { request: 10, spec: Some(spec.clone()) });
    ed(&mut s.core, s.editor, EditorMsg::ToApp { app: s.app, msg: HubToApp::Insert { request: 11, parent: 1, index: Some(1), type_name: STACK.into(), props: vec![], children: spec.children.clone() } });
    s.place(1, 1, node(80, Some(1), STACK));
    ap(&mut s.core, s.app, AppMsg::Inserted { request: 11, node: Some(80), parent: Some(1), index: Some(1) });
    s.tree_now();
    let m = s.answer(EditorMsg::InsertCode { request: 12, app: s.app, node: 80, parent: 1, index: 0, type_name: STACK.into(), defaults: vec![], spec: Some(spec) });
    assert!(edited(&m), "{m:?}");
    assert!(
        s.code().contains("            },\n            new SkiaStack\n            {\n                Children = new List<SkiaControl>\n                {\n                    new SkiaLabel { Text = \"A\" },\n                    new SkiaLabel { Text = \"B\" },\n                    new SkiaLabel { Text = \"C\" },\n                },\n            },\n            new SkiaStack\n"),
        "{}",
        s.code()
    );
}

/// The page names its snippet: a React snippet (`fiddle:///snippet.tsx`) is read and written as
/// that file, a C# one as `.cs`.
#[test]
fn a_page_snippet_keeps_the_name_the_page_gives_it() {
    use super::page::{is_snippet, PageCode};
    assert!(is_snippet("fiddle:///snippet.tsx") && is_snippet("fiddle:///snippet.cs") && !is_snippet("fiddle:///other.tsx") && !is_snippet("fiddle:///snippet."));
    let code = PageCode::new(|_| {});
    code.set_text_for("fiddle:///snippet.tsx", "export default <SkiaLabel Text=\"A\"/>;".into(), 1);
    let listed: Vec<String> = code.list(std::path::Path::new(super::page::PROJECT)).into_iter().map(|(u, _)| u).collect();
    assert_eq!(listed, ["fiddle:///snippet.tsx"]);
    assert!(code.read("fiddle:///snippet.tsx").is_ok());
    assert!(code.read("fiddle:///snippet.cs").is_err(), "the old name is gone");
    assert!(code.write("fiddle:///snippet.tsx", b"export default <SkiaLabel Text=\"B\"/>;").is_ok());
}

// ---- Moves ----

impl Session {
    /// An editor moves `id` live into `parent` at `index` (counted without it); the app answers.
    fn move_to(&mut self, request: u32, id: u64, parent: u64, index: usize) {
        ed(&mut self.core, self.editor, EditorMsg::ToApp { app: self.app, msg: HubToApp::Move { request, node: id, parent, index: index as u32 } });
        let inside: Vec<Node> = self.tree.nodes.iter().filter(|n| n.id == id).cloned().collect();
        self.tree.nodes.retain(|n| n.id != id);
        let mut n = inside[0].clone();
        n.parent = Some(parent);
        self.place(parent, index, n);
        ap(&mut self.core, self.app, AppMsg::Moved { request, node: Some(id), parent: Some(parent), index: Some(index as u32) });
        self.tree_now();
    }
}

#[test]
fn an_unwritten_move_keeps_the_code_view_and_comes_back_after_a_rebuild() {
    let mut s = Session::new();
    // A goes after C live (B, C, A); the code still has A first.
    s.move_to(10, 3, 2, 2);
    // X inserted live first (X, B, C, A): in the code it goes where A's code place leaves it, after A.
    s.insert(11, 2, 0, 20, "X");
    assert!(edited(&s.write_insert(12, 20, 2)));
    assert_eq!(s.labels(), ["A", "X", "B", "C"], "the code view had A at its place in the code");
    // The write's rebuild (A 110, X 111, B 112, C 113): A is moved again, after C.
    s.rebuild();
    let outs = s.rebuilt();
    let (moved, _, lost) = replayed(&outs).expect("Replayed");
    assert!(lost.is_empty() && moved.contains(&(3, 110)), "{moved:?} {lost:?}");
    assert!(to_app(&outs).iter().any(|m| matches!(m, HubToApp::Move { node: 110, parent: 102, index: 3, .. })), "{outs:?}");
}

/// A stack inserted live, then A moved into it, nothing written: after a rebuild the stack comes
/// back first, then A goes into it again once the app has told the stack's new id.
#[test]
fn a_move_into_an_unwritten_insert_comes_back_after_it() {
    let mut s = Session::new();
    ed(&mut s.core, s.editor, EditorMsg::ToApp { app: s.app, msg: HubToApp::Insert { request: 10, parent: 1, index: Some(1), type_name: STACK.into(), props: vec![], children: vec![] } });
    s.place(1, 1, node(40, Some(1), STACK));
    ap(&mut s.core, s.app, AppMsg::Inserted { request: 10, node: Some(40), parent: Some(1), index: Some(1) });
    s.tree_now();
    s.move_to(11, 3, 40, 0);
    // The code view still has A first in stack 2, and no stack 40.
    let view = s.core.apps[&s.app].live.code_view(&s.tree, None);
    assert_eq!(view.nodes.iter().find(|n| n.id == 3).and_then(|n| n.parent), Some(2), "{view:?}");
    assert!(!view.nodes.iter().any(|n| n.id == 40));
    // A hot reload builds the code again (A 110, B 111, C 112): the stack goes back, nothing is lost.
    s.rebuild();
    let outs = s.rebuilt();
    let (moved, replaying, lost) = replayed(&outs).expect("Replayed");
    assert_eq!((replaying, lost), (vec![40], Vec::<String>::new()));
    assert!(moved.contains(&(3, 110)), "{moved:?}");
    let sends = to_app(&outs);
    let Some(HubToApp::Insert { request, parent: 101, index: Some(1), .. }) = sends.first().cloned() else { panic!("{sends:?}") };
    assert!(!sends.iter().any(|m| matches!(m, HubToApp::Move { .. })), "not before the stack is there: {sends:?}");
    // The app tells the stack's new id: A goes into it.
    s.place(101, 1, node(140, Some(101), STACK));
    let outs = s.core.app_msg(s.app, AppMsg::Inserted { request, node: Some(140), parent: Some(101), index: Some(1) });
    assert_eq!(replayed(&outs).map(|r| r.0), Some(vec![(40, 140)]));
    assert!(to_app(&outs).iter().any(|m| matches!(m, HubToApp::Move { node: 110, parent: 140, index: 0, .. })), "{outs:?}");
}

#[test]
fn a_refused_move_is_not_recorded() {
    let mut s = Session::new();
    ed(&mut s.core, s.editor, EditorMsg::ToApp { app: s.app, msg: HubToApp::Move { request: 10, node: 3, parent: 2, index: 2 } });
    ap(&mut s.core, s.app, AppMsg::Moved { request: 10, node: None, parent: None, index: None });
    s.insert(11, 2, 0, 20, "X");
    assert!(edited(&s.write_insert(12, 20, 2)));
    assert_eq!(s.labels(), ["X", "A", "B", "C"]);
}

#[test]
fn a_move_is_written_and_undone() {
    let mut s = Session::new();
    s.move_to(10, 3, 2, 2); // A after C live
    assert!(edited(&s.answer(EditorMsg::MoveCode { request: 11, app: s.app, node: 3, parent: 2, index: 2 })));
    assert_eq!(s.labels(), ["B", "C", "A"]);
    assert!(matches!(s.undo(11), HubToEditor::CodeUndone { .. }));
    assert_eq!(s.labels(), ["A", "B", "C"]);
    // Undone: the move is live-only again, so the code view puts A back first for the next write.
    s.insert(12, 2, 0, 20, "X"); // X, B, C, A live
    assert!(edited(&s.write_insert(13, 20, 2)));
    assert_eq!(s.labels(), ["A", "X", "B", "C"]);
}

#[test]
fn a_move_into_a_loop_is_refused() {
    let mut s = Session::new();
    s.move_to(10, 3, 6, 0); // A into the loop's stack
    let r = s.answer(EditorMsg::MoveCode { request: 11, app: s.app, node: 3, parent: 6, index: 0 });
    assert!(!edited(&r), "{r:?}");
}

/// One Write is one code.edit, in a page, for a C# and a React snippet: also when the page sends its
/// text from before the write again before taking it, and when Write is pressed again after.
#[test]
fn one_write_is_one_page_edit() {
    const TSX: &str = "import { SkiaLabel, SkiaStack } from \"drawnui-react\";\n\nexport default function App() {\n  return (\n    <SkiaStack Spacing={8}>\n      <SkiaLabel Text=\"Hello React\" />\n    </SkiaStack>\n  );\n}\n";
    for (uri, text, hello) in [(page::SNIPPET, SNIPPET, "Hi"), ("fiddle:///snippet.tsx", TSX, "Hello React")] {
        let posted: Arc<Mutex<Vec<page::ToPage>>> = Arc::default();
        let sink = posted.clone();
        let code = Arc::new(page::PageCode::new(move |m| sink.lock().unwrap().push(m)));
        let mut core = HubCore::new(code.clone());
        core.page_project = Some(page::PROJECT.into());
        let (editor, _) = core.editor_hello(PROTOCOL_VERSION).unwrap();
        let (app, _) = core.app_hello(AppInfo { platform: Some("web".into()), ..info("FiddleSnippet", 1, None) }).unwrap();
        let n = |id, parent, t: &str| Node { id, parent, type_name: format!("DrawnUi.Draw.{t}"), ..Default::default() };
        let nodes = if uri == page::SNIPPET { vec![n(1, None, "SkiaLayout"), n(2, Some(1), "SkiaStack"), n(3, Some(2), "SkiaLabel")] } else { vec![n(2, None, "SkiaStack"), n(3, Some(2), "SkiaLabel")] };
        ap(&mut core, app, AppMsg::Tree(Tree { revision: 1, width: 100.0, height: 50.0, nodes }));
        code.set_text_for(uri, text.into(), 7);
        ap(&mut core, app, AppMsg::Props { node: 3, props: vec![prop("Text", "System.String", hello)] });
        let edits = |posted: &Mutex<Vec<page::ToPage>>| posted.lock().unwrap().iter().filter(|m| matches!(m, page::ToPage::Edit { .. })).count();
        let set = |request| EditorMsg::SetCode { request, app, node: 3, prop: "Text".into(), value: "Hello Designer".into() };
        let outs = ed(&mut core, editor, set(1));
        assert!(matches!(outs.as_slice(), [Out::ToEditor(_, HubToEditor::CodeEdited { request: 1, .. })]), "{uri}: {outs:?}");
        assert_eq!(edits(&posted), 1, "{uri}: one edit");
        let edited = text.replace(&format!("\"{hello}\""), "\"Hello Designer\"");
        // The page sends its text from before the write again (same version), then takes the write.
        code.set_text_for(uri, text.into(), 7);
        ap(&mut core, app, AppMsg::Props { node: 3, props: vec![prop("Text", "System.String", "Hello Designer")] });
        let again = ed(&mut core, editor, set(2));
        assert!(!matches!(again.as_slice(), [Out::ToEditor(_, HubToEditor::CodeEdited { .. })]) || edits(&posted) == 1, "{uri}: {again:?}");
        assert_eq!(edits(&posted), 1, "{uri}: the old text sent again makes no second edit");
        code.set_text_for(uri, edited.clone(), 8);
        // Write pressed again: the code has it already, nothing goes to the page.
        ed(&mut core, editor, set(3));
        assert_eq!(edits(&posted), 1, "{uri}: Write again changes nothing");
        assert_eq!(code.read(uri).map(|b| String::from_utf8(b).unwrap()), Ok(edited), "{uri}");
    }
}
