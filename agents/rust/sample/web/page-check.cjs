// Headless check of the Rust agent's page link (src/page.rs): the sample's browser build in Node,
// drawn on the CPU, with this script as the page and the designer.
//   cargo build -j 12 --target wasm32-unknown-emscripten -p designer-agent-sample   (in agents/rust)
//   node sample/web/page-check.cjs target/wasm32-unknown-emscripten/debug/designer-agent-sample.js
const path = require("node:path");
const createDrawnUi = require(path.resolve(process.argv[2]));

(async () => {
  const m = await createDrawnUi();
  const W = 600, H = 700;
  let time = 0;
  const open = () => {
    const app = m._dui_create();
    m._dui_attach(app, W, H, 1, 0); // 0: a 2D context, the frames are drawn on the CPU
    return app;
  };
  let app = open();
  const frame = () => m._dui_frame(app, (time += 16));
  const utf8 = (s) => {
    const bytes = new TextEncoder().encode(s);
    const ptr = m._dui_alloc(bytes.length);
    m.HEAPU8.set(bytes, ptr);
    return [ptr, bytes.length];
  };
  const out = [];
  const take = (type) => {
    const i = out.findIndex((o) => (o.type ?? o.role) === type);
    if (i < 0) fail(`no ${type} among ${JSON.stringify(out.map((o) => o.type ?? o.role))}`);
    return out.splice(i, 1)[0];
  };
  const fail = (what) => {
    console.error("FAIL", what);
    process.exit(1);
  };
  const check = (ok, what) => (ok ? console.log("ok  ", what) : fail(what));
  const receive = (msg) => {
    const [ptr, len] = utf8(JSON.stringify(msg));
    m._dui_designer_receive(ptr, len);
    m._dui_free(ptr, len);
    frame(); // the page asks for a frame after each message (requestFrame)
  };
  const pixel = (x, y) => {
    const at = m._dui_pixels(app) + (y * W + x) * 4;
    return Array.from(m.HEAPU8.subarray(at, at + 4)).join(",");
  };

  for (let i = 0; i < 3; i++) frame();
  check(out.length === 0, "no page attached: nothing goes out");

  const sink = m.addFunction((ptr, len) => out.push(JSON.parse(m.UTF8ToString(ptr, len))), "vii");
  const [name, nameLen] = utf8("Designer sample");
  m._dui_designer_attach(sink, name, nameLen);
  m._dui_free(name, nameLen);
  frame();
  const hello = take("app");
  check(hello.engine === "rust" && hello.platform === "browser" && hello.pid === 0 && hello.project == null && hello.name === "Designer sample", `hello ${JSON.stringify(hello)}`);
  const tree = take("tree");
  const title = tree.nodes.find((n) => n.tag === "title");
  check(title && tree.nodes.some((n) => n.internal), `tree: ${tree.nodes.length} nodes, the title and internal parts`);

  receive({ type: "setProperty", request: 1, node: title.id, name: "font_size", value: "40" });
  receive({ type: "getProps", node: title.id });
  const fontSize = take("props").props.find((p) => p.name === "font_size");
  check(fontSize.value === "40", `setProperty applied: font_size ${fontSize.value}`);

  receive({ type: "getTree" });
  const r = take("tree").nodes.find((n) => n.id === title.id).rect;
  receive({ type: "highlight", node: title.id, color: "#00C853" });
  const [x, y] = [Math.round(r[0] - 1), Math.round(r[1] + r[3] / 2)];
  check(pixel(x, y) === "0,200,83,255", `highlight drawn in the designer's color at (${x},${y}): ${pixel(x, y)}`);

  receive({ type: "snapshot", request: 7 });
  const shot = take("snapshot");
  check(shot.request === 7 && shot.width === W && shot.pngBase64.startsWith("iVBOR"), `snapshot ${shot.width}x${shot.height} PNG`);

  receive({ type: "getControls" });
  const kinds = take("controls").controls.map((k) => k.typeName);
  const root = tree.nodes.find((n) => n.parent == null).id;
  receive({ type: "insert", request: 8, parent: root, index: 0, typeName: "SkiaLabel" });
  const inserted = take("inserted");
  const after = take("tree").nodes.find((n) => n.id === inserted.node);
  check(kinds.includes("SkiaLabel") && after?.text === "Label" && after.rect[3] > 0, `getControls (${kinds.length} kinds), insert: ${JSON.stringify(after?.text)} laid out`);
  receive({ type: "remove", request: 9, node: inserted.node });
  check(!take("tree").nodes.some((n) => n.id === inserted.node), "remove");

  receive({ type: "rebuilt" });
  check(take("hotReload").stage === "reloaded", "rebuilt: the designer hears its ids are old");
  check(take("tree").nodes.length === tree.nodes.length, "rebuilt: a new tree");

  // A new app under the attached page (what a hot swap does): it says so, its tree goes out, no
  // second hello.
  app = open();
  frame();
  check(take("hotReload").stage === "reloaded", "a new app: the designer hears its ids are old");
  const swapped = take("tree");
  check(swapped.nodes.length === tree.nodes.length && !out.some((o) => o.role === "app"), "a new app: attached, its tree, no hello");

  // One move with the new tree's ids: one answer, the same control. An id the new tree does not
  // have (one from before, whose index another control may hold now): refused, nothing moves.
  const newRoot = swapped.nodes.find((n) => n.parent == null).id;
  const newTitle = swapped.nodes.find((n) => n.tag === "title").id;
  receive({ type: "move", request: 10, node: newTitle, parent: newRoot, index: 1 });
  const moved = take("moved");
  check(moved.node === newTitle && moved.index === 1 && !out.some((o) => o.type === "moved"), `one move, the current ids: ${JSON.stringify(moved)}`);
  take("tree");
  receive({ type: "move", request: 11, node: newTitle + 2 ** 40, parent: newRoot, index: 0 });
  const error = take("error");
  check(take("moved").node == null && error.message.includes("not in the app's tree now"), `a stale id refused: ${error.message}`);
  // The title's id from the first app's tree: the swapped-in app never gave it, refused.
  receive({ type: "move", request: 12, node: title.id, parent: root, index: 0 });
  const old = take("error");
  check(take("moved").node == null && title.id !== newTitle && old.message.includes("not in the app's tree now"), `the old app's id refused: ${title.id} (now ${newTitle})`);

  check(out.length === 0, `nothing unasked: ${JSON.stringify(out.map((o) => o.type ?? o.role))}`);
  m._dui_designer_attach(0, 0, 0);
  receive({ type: "getTree" });
  frame();
  check(out.length === 0, "detached: nothing goes out");
  console.log("page check passed");
})().catch((e) => fail(e.stack ?? e));
