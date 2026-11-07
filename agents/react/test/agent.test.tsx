// The agent against the real engine (drawnui-react on CanvasKit in Node) and the real React reconciler, through the page
// channel: tree, props, a live set surviving a re-render, pick, the highlight's pixels, snapshot, insert / remove /
// restore, copy, the Controls panel, the source map lookup. Run: npm test
import CanvasKitInit from "canvaskit-wasm/bin/full/canvaskit.js";
import { transformSync } from "esbuild";
import type { ReactElement } from "react";
import * as Core from "drawnui-react/core";
import { SkiaButton, SkiaLabel, SkiaLayer, SkiaRow, SkiaStack } from "drawnui-react";
// the reconciler's root (not exported by the package; test/run.mjs resolves it): the same files as "drawnui-react"
import { createDrawnRoot } from "drawnui-react/reconciler";
import { attachPage, detach, __test } from "../src/index";
import * as Lib from "drawnui-react";

let failures = 0;
const check = (name: string, ok: boolean, detail = "") => { if (!ok) failures++; console.log(`${ok ? "ok  " : "FAIL"} ${name}${ok ? "" : " " + detail}`); };
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** The engine Canvas without a DOM: a raster surface, frames on demand, WasDrawn at the end of each, as Canvas.Draw. */
class TestCanvas {
  Element = { width: 400, height: 300, parentElement: null };
  RenderingScale = 1;
  BackgroundColor = "#FF000000";
  Content: any;
  WasDrawn?: (c: unknown) => void;
  WasDrawnListeners?: Set<(c: unknown) => void>;
  requested = false;
  frames = 0;
  constructor(readonly surface: any) {}
  get Surface() { return this.surface; }
  Update() { this.requested = true; }
  Draw() {
    this.requested = false;
    const CK = (Core.Super as any).CK, canvas = this.surface.getCanvas(), root = this.Content;
    canvas.clear(CK.BLACK);
    if (root) {
      root.Measure(400, 300, 1);
      root.Arrange(new Core.SKRect(0, 0, 400, 300), root.WidthRequest, root.HeightRequest, 1);
      root.Render({ Context: { Canvas: canvas, Surface: this.surface }, Destination: new Core.SKRect(0, 0, 400, 300), Scale: 1 });
    }
    this.WasDrawn?.(this);
    for (const l of this.WasDrawnListeners ?? []) l(this);
    this.surface.flush();
    this.frames++;
  }
  TakeScreenShot() { return (Core.Canvas.prototype as any).TakeScreenShot.call(this); }
}

(async () => {
  const CK = await (CanvasKitInit as any)({ locateFile: () => `${process.cwd()}/node_modules/canvaskit-wasm/bin/full/canvaskit.wasm` });
  (Core.Super as any).CK = CK;
  (globalThis as any).btoa ??= (s: string) => Buffer.from(s, "binary").toString("base64");

  for (const listeners of [false, true]) {
    console.log(listeners ? "\n-- with Canvas.WasDrawnListeners" : "-- with WasDrawn only (drawnui-react without WasDrawnListeners)");
    const canvas = new TestCanvas(CK.MakeSurface(400, 300));
    if (listeners) canvas.WasDrawnListeners = new Set();
    let appFrames = 0;
    canvas.WasDrawn = () => { appFrames++; };
    const pump = setInterval(() => { if (canvas.requested) canvas.Draw(); }, 2);
    const root = createDrawnRoot(canvas as any);
    let label: any, stack: any;
    const page = (text: string, margin: number, extra = false): ReactElement => (
      <SkiaStack ref={(c: any) => { if (c) stack = c; }} BackgroundColor="#FF202020" Padding={10} Spacing={10} HorizontalOptions="Fill" VerticalOptions="Fill">
        <SkiaLabel ref={(c: any) => { if (c) label = c; }} Text={text} TextColor="#FFFFFFFF" FontSize={20} WidthRequest={200} HeightRequest={30} Margin={new Core.Thickness(margin)} />
        <SkiaButton Text="Tap" WidthRequest={120} HeightRequest={40} />
        <SkiaLayer WidthRequest={100} HeightRequest={50} />
        {extra ? <SkiaLabel Text="Extra" /> : null}
      </SkiaStack>
    );
    const render = async (el: ReactElement) => { root.render(el); await wait(30); canvas.Draw(); };
    await render(page("Hello", 4));

    const got: any[] = [];
    const receive = attachPage(canvas, (text) => got.push(JSON.parse(text)));
    const ask = async (msg: object, type: string, ms = 1500) => {
      const from = got.length;
      receive(msg);
      for (let t = 0; t < ms; t += 5) { const m = got.slice(from).find((x) => x.type === type); if (m) return m; await wait(5); }
      throw new Error(`no ${type} for ${JSON.stringify(msg)}; got ${JSON.stringify(got.slice(from)).slice(0, 300)}`);
    };
    for (let t = 0; t < 1000 && !got.some((m) => m.type === "tree"); t += 5) await wait(5);

    // hello + tree
    const hello = got[0];
    check("hello first", hello?.role === "app" && hello.engine === "react" && hello.platform === "browser" && hello.pid === 0 && hello.protocol === "0.1.0", JSON.stringify(hello));
    let tree = got.find((m) => m.type === "tree");
    const byId = (t: any, id: number) => t.nodes.find((n: any) => n.id === id);
    const top = tree.nodes.filter((n: any) => n.parent === null);
    check("one root, the stack", top.length === 1 && top[0].typeName === "SkiaStack" && top[0].id === stack.AccessibilityId, JSON.stringify(top));
    const kids = tree.nodes.filter((n: any) => n.parent === stack.AccessibilityId);
    check("the app's children in order", kids.map((n: any) => n.typeName).join(",") === "SkiaLabel,SkiaButton,SkiaLayer", kids.map((n: any) => n.typeName).join(","));
    check("app children are not internal", kids.every((n: any) => !n.internal));
    const button = kids[1];
    const parts = tree.nodes.filter((n: any) => n.parent === button.id);
    check("a button's own parts are internal", parts.length > 0 && parts.every((n: any) => n.internal), JSON.stringify(parts.map((p: any) => [p.typeName, p.internal])));
    const ln = byId(tree, label.AccessibilityId);
    check("label text preview and rect", ln.text === "Hello" && ln.visible && ln.rect[0] === 14 && ln.rect[1] === 14 && ln.rect[2] > 0, JSON.stringify(ln));
    check("canvas size", tree.width === 400 && tree.height === 300);

    // props
    const props = (await ask({ type: "getProps", node: label.AccessibilityId }, "props")).props;
    const prop = (name: string) => props.find((p: any) => p.name === name);
    check("Text is a set string", prop("Text")?.value === "Hello" && prop("Text").isSet && prop("Text").typeName === "System.String");
    check("TextColor is a color", prop("TextColor")?.typeName === "DrawnUi.Color" && prop("TextColor").value === "#FFFFFFFF");
    check("Margin is a thickness", prop("Margin")?.value === "4,4,4,4" && prop("Margin").typeName === "DrawnUi.Draw.Thickness");
    check("an untouched number is default", prop("Rotation")?.value === "0" && prop("Rotation").isDefault === true && !prop("Rotation").isSet);
    check("enum-like strings offer choices", prop("HorizontalOptions")?.choices?.join() === "Start,Center,End,Fill");
    check("engine state is not listed", !prop("AccessibilityId") && !prop("RenderingScale") && !prop("DrawingRect") && !prop("Tapped"));

    // designer actions (a drawnui-react with DesignerActions): sent when declared, run, left out when removed
    if (!listeners && (Core as any).DesignerActions) {
      let ran = "";
      const lastActions = async (from: number) => {
        for (let t = 0; t < 500 && !got.slice(from).some((m) => m.type === "actions"); t += 5) await wait(5);
        return got.slice(from).filter((m) => m.type === "actions").pop();
      };
      let from = got.length;
      const remove = (Lib as any).designerAction("Levels/Level 3", () => { ran = "level 3"; });
      const list = await lastActions(from);
      check("actions: a declared one is sent", !!list?.actions?.some((a: any) => a.path === "Levels/Level 3"), JSON.stringify(list));
      await ask({ type: "runAction", path: "Levels/Level 3" }, "tree");
      check("runAction runs it", ran === "level 3");
      const err = await ask({ type: "runAction", path: "Nope" }, "error");
      check("an unknown action is an error", /no action/.test(err.message), JSON.stringify(err));
      from = got.length;
      remove();
      const after = await lastActions(from);
      check("a removed action leaves the list", !!after && !after.actions.some((a: any) => a.path === "Levels/Level 3"), JSON.stringify(after));
    } else if (!listeners) console.log("skip actions: this drawnui-react has no DesignerActions");

    // a live set survives a re-render; a code change ends it
    receive({ type: "setProperty", request: 1, node: label.AccessibilityId, name: "Margin", value: "5,6,7,8" });
    receive({ type: "setProperty", request: 2, node: label.AccessibilityId, name: "FontSize", value: "28" });
    await ask({ type: "getTree" }, "tree");
    check("live set applied", label.Margin.Left === 5 && label.Margin.Bottom === 8 && label.FontSize === 28);
    await render(page("Hello again", 4)); // re-render: a new Thickness(4) object, Text changed
    check("live set survives a re-render", label.Margin.Left === 5 && label.Margin.Top === 6 && label.FontSize === 28 && label.Text === "Hello again",
      `${label.Margin.Left},${label.Margin.Top} ${label.FontSize} ${label.Text}`);
    check("drawn with the live value", byId(await ask({ type: "getTree" }, "tree"), label.AccessibilityId).rect[0] === 15);
    await render(page("Hello again", 9)); // the code changed (hot reload): its value wins
    check("a new code value ends the live set", label.Margin.Left === 9 && label.FontSize === 28);
    const pinnedNames = (c: any) => Object.getOwnPropertyNames(c).filter((n) => Object.getOwnPropertyDescriptor(c, n)?.get);
    check("pinned while live", pinnedNames(label).join() === "FontSize", pinnedNames(label).join());
    receive({ type: "setProperty", request: 4, node: label.AccessibilityId, name: "FontSize", value: "20" });
    await ask({ type: "getTree" }, "tree");
    check("back to the code value (Undo): the pin is gone, the class's accessor is back", label.FontSize === 20 && pinnedNames(label).length === 0
      && !Object.getOwnPropertyDescriptor(label, "FontSize"), pinnedNames(label).join());
    receive({ type: "setProperty", request: 6, node: label.AccessibilityId, name: "IsClippedToBounds", value: "true" });
    await ask({ type: "getTree" }, "tree");
    const field = () => Object.getOwnPropertyDescriptor(label, "IsClippedToBounds");
    const pinnedField = !!field()?.get;
    receive({ type: "setProperty", request: 7, node: label.AccessibilityId, name: "IsClippedToBounds", value: "false" });
    await ask({ type: "getTree" }, "tree");
    check("a pinned plain field becomes a plain field again", pinnedField && field()?.writable === true && field()?.value === false, JSON.stringify(field()));
    receive({ type: "setProperty", request: 5, node: label.AccessibilityId, name: "Margin", value: "1" });
    receive({ type: "setProperty", request: 3, node: label.AccessibilityId, name: "FontSize", value: "big" });
    const err = await ask({ type: "getTree" }, "error").catch(() => undefined);
    check("a bad value answers an error", !!err || got.some((m) => m.type === "error" && m.request === 3));

    // pick: the topmost control; a button's own label answers as the button; an empty layer is looked through
    tree = await ask({ type: "getTree" }, "tree");
    const center = (id: number) => { const r = byId(tree, id).rect; return { x: r[0] + r[2] / 2, y: r[1] + r[3] / 2 }; };
    const pick = async (p: { x: number; y: number }) => (await ask({ type: "pick", request: 7, x: p.x, y: p.y }, "picked")).node;
    check("pick a label", (await pick(center(label.AccessibilityId))) === label.AccessibilityId);
    check("pick a button's caption answers the button", (await pick(center(button.id))) === button.id);
    check("an empty layer is looked through", (await pick(center(kids[2].id))) === stack.AccessibilityId);
    check("pick outside everything", (await pick({ x: 399, y: 299 })) === stack.AccessibilityId);

    // highlight: a 2 px box in the given color just outside the control, drawn in the app's frame
    receive({ type: "highlight", node: label.AccessibilityId, color: "#FF00FF00" });
    await ask({ type: "getTree" }, "tree");
    canvas.Draw();
    const r = byId(tree, label.AccessibilityId).rect;
    const px = (x: number, y: number) => Array.from(canvas.surface.makeImageSnapshot().readPixels(x, y, { width: 1, height: 1, colorType: CK.ColorType.RGBA_8888, alphaType: CK.AlphaType.Unpremul, colorSpace: CK.ColorSpace.SRGB }) as Uint8Array).join(",");
    check("highlight pixel left of the box", px(Math.floor(r[0]) - 1, Math.floor(r[1] + r[3] / 2)) === "0,255,0,255", px(Math.floor(r[0]) - 1, Math.floor(r[1] + r[3] / 2)));
    check("the app's own WasDrawn still runs", appFrames > 0);
    receive({ type: "highlight", node: null });
    await wait(30);
    canvas.Draw();
    check("highlight cleared", px(Math.floor(r[0]) - 1, Math.floor(r[1] + r[3] / 2)) === "32,32,32,255", px(Math.floor(r[0]) - 1, Math.floor(r[1] + r[3] / 2)));

    // snapshot
    const snap = await ask({ type: "snapshot", request: 9 }, "snapshot");
    check("snapshot is a PNG of the canvas", snap.request === 9 && snap.pngBase64.startsWith("iVBORw0KGgo") && snap.width === 400 && snap.height === 300);

    // controls panel
    const controls = (await ask({ type: "getControls" }, "controls")).controls;
    const kind = (n: string) => controls.find((c: any) => c.typeName === n);
    check("controls: label, stack container, button input", kind("SkiaLabel")?.group === "Text" && kind("SkiaStack")?.container === true && kind("SkiaButton")?.group === "Input" && !kind("SkiaControl"),
      JSON.stringify([kind("SkiaLabel"), kind("SkiaStack"), kind("SkiaButton")]));

    // insert, remove, restore
    const ins = await ask({ type: "insert", request: 11, parent: stack.AccessibilityId, index: 1, typeName: "SkiaLabel", props: [], children: [] }, "inserted");
    tree = await ask({ type: "getTree" }, "tree");
    const order = () => tree.nodes.filter((n: any) => n.parent === stack.AccessibilityId).map((n: any) => n.id);
    check("inserted at index 1 with its defaults", ins.node && order()[1] === ins.node && byId(tree, ins.node).text === "Label", JSON.stringify(ins));
    await render(page("Hello again", 9, true)); // React renders around the inserted control
    tree = await ask({ type: "getTree" }, "tree");
    check("React's own children keep their order around it", order().length === 5 && order()[1] === ins.node);
    await ask({ type: "remove", request: 12, node: ins.node }, "tree").then((t) => { tree = t; });
    check("an inserted control is taken out", !byId(tree, ins.node) && order().length === 4);
    await ask({ type: "remove", request: 13, node: label.AccessibilityId }, "tree").then((t) => { tree = t; });
    check("a React control is hidden, not in the tree", !byId(tree, label.AccessibilityId) && label.IsVisible === false && !label.IsDisposed);
    await render(page("Hello again", 9, true));
    check("it stays hidden through a re-render", label.IsVisible === false);
    const back = await ask({ type: "insert", request: 14, parent: stack.AccessibilityId, index: 0, typeName: "SkiaLabel", props: [["FontSize", "28"]], children: [] }, "inserted");
    tree = await ask({ type: "getTree" }, "tree");
    check("restore shows the same control again", back.node === label.AccessibilityId && label.IsVisible === true && order()[0] === label.AccessibilityId);

    // copy: the Set props with their types, and the children
    const copy = (await ask({ type: "copy", request: 15, node: stack.AccessibilityId }, "copied")).spec;
    const kidSpec = copy.children.find((c: any) => c.typeName === "SkiaLabel");
    check("copy: type, set props, children", copy.typeName === "SkiaStack" && copy.props.some((p: any) => p.name === "Padding" && p.value === "10,10,10,10")
      && copy.children.length === 4 && kidSpec?.props.some((p: any) => p.name === "Text" && p.typeName === "System.String"), JSON.stringify(copy).slice(0, 400));
    const paste = await ask({ type: "insert", request: 16, parent: stack.AccessibilityId, index: null, typeName: "SkiaStack", props: copy.props.map((p: any) => [p.name, p.value]), children: copy.children }, "inserted");
    tree = await ask({ type: "getTree" }, "tree");
    check("paste builds the copy with its children", tree.nodes.filter((n: any) => n.parent === paste.node).length === 4);

    // move: a control the designer made moves itself; a React-owned one shows as a stand-in at the new place under
    // its id while React keeps the original (hidden) where it put it
    const move = (node: number, parent: number, index: number, request = 20) => ask({ type: "move", request, node, parent, index }, "moved");
    const mv1 = await move(paste.node, stack.AccessibilityId, 0);
    tree = await ask({ type: "getTree" }, "tree");
    check("move an inserted control: same id, new place", mv1.node === paste.node && mv1.index === 0 && order()[0] === paste.node, JSON.stringify(mv1));
    const layerId = tree.nodes.find((n: any) => n.parent === stack.AccessibilityId && n.typeName === "SkiaLayer").id;
    const original = label;
    const mv2 = await move(label.AccessibilityId, layerId, 0);
    tree = await ask({ type: "getTree" }, "tree");
    const movedNode = byId(tree, label.AccessibilityId);
    check("move a React control: its id now under the new parent", mv2.node === label.AccessibilityId && movedNode?.parent === layerId && movedNode.typeName === "SkiaLabel" && movedNode.visible,
      JSON.stringify([mv2, movedNode]));
    check("React's original stays where React put it, hidden", original.Parent === stack && original.IsVisible === false && !original.IsDisposed && !order().includes(label.AccessibilityId));
    receive({ type: "setProperty", request: 21, node: label.AccessibilityId, name: "FontSize", value: "18" });
    await ask({ type: "getTree" }, "tree");
    await render(page("After the move", 9, false)); // a re-render with a structure change in the stack (the extra label goes)
    tree = await ask({ type: "getTree" }, "tree");
    check("a re-render after a move: React's commit goes through, the moved control stays", original.Text === "After the move" && original.IsVisible === false
      && byId(tree, label.AccessibilityId)?.parent === layerId && order().length === 3, JSON.stringify(order()));
    const props2 = (await ask({ type: "getProps", node: label.AccessibilityId }, "props")).props;
    check("the moved control's props are the stand-in's", props2.find((p: any) => p.name === "FontSize")?.value === "18");
    const refused = await move(stack.AccessibilityId, layerId, 0, 22);
    check("move refused: the root", refused.node === null && got.some((m) => m.type === "error" && m.request === 22 && /root/.test(m.message)));
    const buttonId = tree.nodes.find((n: any) => n.parent === stack.AccessibilityId && n.typeName === "SkiaButton").id;
    const intoLabel = await move(buttonId, label.AccessibilityId, 0, 23);
    check("move refused: into a control that holds no children", intoLabel.node === null && got.some((m) => m.type === "error" && m.request === 23 && /does not hold children/.test(m.message)));
    const intoSelf = await move(paste.node, paste.node, 0, 24);
    check("move refused: into itself", intoSelf.node === null && got.some((m) => m.type === "error" && m.request === 24 && /inside itself/.test(m.message)));
    const back2 = await move(label.AccessibilityId, stack.AccessibilityId, 1, 25); // its own place: after the pasted stack
    tree = await ask({ type: "getTree" }, "tree");
    check("moved back home: the original shows again, the stand-in is gone", back2.node === label.AccessibilityId && original.IsVisible === true
      && byId(tree, label.AccessibilityId)?.parent === stack.AccessibilityId && order()[1] === label.AccessibilityId
      && !tree.nodes.some((n: any) => n.parent === layerId), JSON.stringify(order()));

    // a structure change the agent did not make (navigation, hot reload) sends a tree by itself
    const before = got.length;
    await render(page("Hello again", 9, true)); // the extra label comes back
    __test.state.lastCheck = 0;
    canvas.Draw();
    await wait(50);
    check("structure change pushes a tree", got.slice(before).some((m) => m.type === "tree"));

    // popup windows (Canvas.PopupWindowRoots): extra roots named "popup N", picked before the page under them
    const menu = new Core.SkiaStack();
    menu.BackgroundColor = "#FF445566"; menu.WidthRequest = 80; menu.HeightRequest = 60;
    menu.Measure(400, 300, 1); menu.Arrange(new Core.SKRect(300, 200, 380, 260), 80, 60, 1);
    (canvas as any).PopupWindowRoots = [menu];
    tree = await ask({ type: "getTree" }, "tree");
    const popupNode = tree.nodes.find((n: any) => n.id === menu.AccessibilityId);
    check("a popup window is a root named popup 1", popupNode?.parent === null && popupNode.host === "popup 1" && popupNode.rect.join() === "300,200,80,60", JSON.stringify(popupNode));
    check("pick finds the popup over the page", (await pick({ x: 340, y: 230 })) === menu.AccessibilityId);
    delete (canvas as any).PopupWindowRoots;

    receive({ type: "nonsense" });
    receive("{not json");
    await wait(30);
    check("bad messages answer errors, never throw", got.filter((m) => m.type === "error").length >= 2);

    check("pins before detach", pinnedNames(label).includes("Margin"), pinnedNames(label).join());
    detach();
    check("detach takes every pin off (the class's Margin accessor is back)", pinnedNames(label).length === 0 && !Object.getOwnPropertyDescriptor(label, "Margin") && label.Margin.Left === 1);
    const framesAfter = got.length;
    canvas.Draw(); canvas.Draw();
    check("detached: nothing more is sent", got.length === framesAfter);
    check("detached: WasDrawn back to the app's", listeners || typeof canvas.WasDrawn === "function" && !Object.getOwnPropertyDescriptor(canvas, "WasDrawn")?.get);
    clearInterval(pump);
    root.render(<SkiaLayer />);
    await wait(30);
  }

  // a moved React control whose code changed (React unmounts the original: the move was written, or a new run): the
  // stand-in goes too; a move still open when the designer leaves: the original is back in its place
  {
    const canvas = new TestCanvas(CK.MakeSurface(400, 300));
    const pump = setInterval(() => { if (canvas.requested) canvas.Draw(); }, 2);
    const root = createDrawnRoot(canvas as any);
    let label: any, box: any;
    const page = (key: string) => (
      <SkiaStack HorizontalOptions="Fill" VerticalOptions="Fill">
        <SkiaLabel key={key} ref={(c: any) => { if (c) label = c; }} Text="Moved" WidthRequest={60} HeightRequest={20} />
        <SkiaLayer ref={(c: any) => { if (c) box = c; }} WidthRequest={100} HeightRequest={50} />
      </SkiaStack>
    );
    root.render(page("a"));
    await wait(30); canvas.Draw();
    const got: any[] = [];
    const receive = attachPage(canvas, (t) => got.push(JSON.parse(t)));
    const ask = async (msg: object, type: string) => {
      const from = got.length;
      receive(msg);
      for (let t = 0; t < 1500; t += 5) { const m = got.slice(from).find((x) => x.type === type); if (m) return m; await wait(5); }
      throw new Error(`no ${type}`);
    };
    await ask({ type: "getTree" }, "tree");
    const first = label;
    await ask({ type: "move", request: 1, node: first.AccessibilityId, parent: box.AccessibilityId, index: 0 }, "moved");
    root.render(page("b")); // the code changed: React unmounts the original and makes a new label
    await wait(30); canvas.Draw();
    const tree = await ask({ type: "getTree" }, "tree");
    check("the original unmounted: its stand-in is gone, the code's new control shows", first.IsDisposed && !tree.nodes.some((n: any) => n.id === first.AccessibilityId)
      && !tree.nodes.some((n: any) => n.parent === box.AccessibilityId) && tree.nodes.some((n: any) => n.id === label.AccessibilityId && n.visible), JSON.stringify(tree.nodes.map((n: any) => [n.id, n.parent, n.typeName])));
    const second = label;
    await ask({ type: "move", request: 2, node: second.AccessibilityId, parent: box.AccessibilityId, index: 0 }, "moved");
    check("moved before the designer leaves", second.IsVisible === false && box.Views.length === 1);
    detach();
    check("the designer left: the original is back, the stand-in gone, no pin left", second.IsVisible === true && box.Views.length === 0
      && !Object.getOwnPropertyDescriptor(second, "IsVisible")?.get);
    clearInterval(pump);
  }

  // attached before the content mounted and before the first frame (DrawFiddle attaches from the <Canvas> ref): a
  // control is never reported before it is laid out (an editor folds a control first seen hidden, for good)
  {
    const canvas = new TestCanvas(CK.MakeSurface(400, 300)) as TestCanvas & { WasRendered: boolean };
    canvas.WasRendered = false;
    const draw = canvas.Draw.bind(canvas);
    canvas.Draw = () => { draw(); canvas.WasRendered = true; };
    const root = createDrawnRoot(canvas as any);
    root.render(<SkiaStack HorizontalOptions="Fill" VerticalOptions="Fill"><SkiaLabel Tag="late" Text="Late" WidthRequest={60} HeightRequest={20} /></SkiaStack>);
    await wait(30); // mounted, never drawn
    const trees: any[] = [];
    attachPage(canvas, (t) => { const m = JSON.parse(t); if (m.type === "tree") trees.push(m); });
    const pump = setInterval(() => { if (canvas.requested) canvas.Draw(); }, 2);
    canvas.Update();
    __test.state.lastCheck = 0;
    for (let t = 0; t < 2000 && !trees.some((tr) => tr.nodes.some((n: any) => n.tag === "late")); t += 10) { await wait(10); canvas.Update(); }
    const first = trees.map((tr) => tr.nodes.find((n: any) => n.tag === "late")).find(Boolean);
    check("a control is first reported laid out and visible", first?.visible === true && first.rect[2] === 60, JSON.stringify(trees.map((tr) => tr.nodes.length)) + JSON.stringify(first));
    detach();
    clearInterval(pump);
  }

  // DesignerSourceOf present: undefined for every control (React's production build, DrawFiddle) = the same tree as
  // without it, no source; a source for app JSX = "file:line:column" through mapSource, never internal
  {
    const canvas = new TestCanvas(CK.MakeSurface(400, 300));
    const pump = setInterval(() => { if (canvas.requested) canvas.Draw(); }, 2);
    const root = createDrawnRoot(canvas as any);
    root.render(<SkiaStack><SkiaLabel Text="A" WidthRequest={50} HeightRequest={20} /><SkiaButton Text="B" /></SkiaStack>);
    await wait(30); canvas.Draw();
    const trees: any[] = [];
    const run = async (fn: any, options = {}) => {
      __test.setSourceOf(fn);
      trees.length = 0;
      attachPage(canvas, (t) => { const m = JSON.parse(t); if (m.type === "tree") trees.push(m); }, options);
      for (let t = 0; t < 1000 && !trees.length; t += 5) await wait(5);
      detach();
      return trees[0];
    };
    const plain = await run(undefined);
    const prod = await run(() => undefined);
    const strip = (t: any) => JSON.stringify(t.nodes.map((n: any) => [n.typeName, n.internal, n.parent === null, "source" in n]));
    check("DesignerSourceOf returning undefined: same tree as without it, no source", strip(plain) === strip(prod) && prod.nodes.every((n: any) => !("source" in n)), strip(prod));
    const throwing = await run(() => { throw new Error("x"); });
    check("DesignerSourceOf throwing: same tree", strip(throwing) === strip(plain));
    const labelId = plain.nodes.find((n: any) => n.typeName === "SkiaLabel").id;
    const dev = await run((c: any) => c.AccessibilityId === labelId ? { File: "blob:http://x/1", Line: 5, Column: 7 } : c.constructor.name === "SkiaStack" ? undefined : { File: "/node_modules/drawnui-react/dist/react/x.js", Line: 1, Column: 1 },
      { mapSource: (f: string, l: number, c: number) => (f.startsWith("blob:") ? { file: "fiddle:///snippet.tsx", line: l - 1, column: c } : undefined) });
    const label = dev.nodes.find((n: any) => n.id === labelId), btn = dev.nodes.find((n: any) => n.typeName === "SkiaButton");
    check("a source through mapSource: file:line:column", label.source === "fiddle:///snippet.tsx:4:7" && !label.internal, JSON.stringify(label));
    check("JSX inside the library: internal parts", dev.nodes.filter((n: any) => n.parent === btn.id).every((n: any) => n.internal), JSON.stringify(dev.nodes.filter((n: any) => n.parent === btn.id)));
    __test.setSourceOf(undefined);
    clearInterval(pump);
  }

  // templated: a layout that makes its children from items says so, with items or none yet; a plain one does not
  {
    const canvas = new TestCanvas(CK.MakeSurface(400, 300));
    const pump = setInterval(() => { if (canvas.requested) canvas.Draw(); }, 2);
    const root = createDrawnRoot(canvas as any);
    const template = () => new (Core as any).SkiaLabel();
    let full: any, empty: any, plain: any;
    root.render(
      <SkiaStack>
        <SkiaStack ref={(c: any) => { if (c) full = c; }} ItemsSource={["a", "b"]} ItemTemplate={template} />
        <SkiaStack ref={(c: any) => { if (c) empty = c; }} ItemsSource={[]} ItemTemplate={template} />
        <SkiaLayer ref={(c: any) => { if (c) plain = c; }} WidthRequest={50} HeightRequest={20} />
      </SkiaStack>,
    );
    await wait(30); canvas.Draw();
    const trees: any[] = [];
    attachPage(canvas, (t) => { const m = JSON.parse(t); if (m.type === "tree") trees.push(m); });
    for (let t = 0; t < 1000 && !trees.length; t += 5) await wait(5);
    detach();
    const nodes = trees[0]?.nodes ?? [];
    const flagged = nodes.filter((n: any) => n.templated === true).map((n: any) => n.id).sort();
    check("templated on both items layouts (one empty), on no other node",
      flagged.join() === [full.AccessibilityId, empty.AccessibilityId].sort().join() && !flagged.includes(plain.AccessibilityId)
        && nodes.filter((n: any) => !flagged.includes(n.id)).every((n: any) => !("templated" in n)),
      JSON.stringify(nodes.map((n: any) => [n.id, n.typeName, n.templated])));
    root.render(<SkiaStack />);
    clearInterval(pump);
  }

  // inside a cell of a templated layout nothing goes in, out or away, at any depth: every cell is the same template
  {
    const canvas = new TestCanvas(CK.MakeSurface(400, 300));
    const pump = setInterval(() => { if (canvas.requested) canvas.Draw(); }, 2);
    const root = createDrawnRoot(canvas as any);
    const template = () => { const cell = new (Core as any).SkiaLayout(); cell.AddSubView(new (Core as any).SkiaLabel()); return cell; };
    let outside: any;
    root.render(
      <SkiaStack>
        <SkiaLabel ref={(c: any) => { if (c) outside = c; }} Text="out" WidthRequest={50} HeightRequest={20} />
        <SkiaStack ItemsSource={["a", "b"]} ItemTemplate={template} WidthRequest={200} HeightRequest={200} />
      </SkiaStack>,
    );
    await wait(30); canvas.Draw();
    const got: any[] = [];
    const receive = attachPage(canvas, (t) => got.push(JSON.parse(t)));
    for (let t = 0; t < 1000 && !got.some((m) => m.type === "tree"); t += 5) await wait(5);
    const nodes = got.find((m) => m.type === "tree")?.nodes ?? [];
    const cell = nodes.find((n: any) => n.templateIndex != null && nodes.some((m: any) => m.parent === n.id));
    const deep = cell && nodes.find((n: any) => n.parent === cell.id);
    const top = nodes.find((n: any) => n.parent === null);
    check("a cell with a control inside it", !!deep, JSON.stringify(nodes.map((n: any) => [n.id, n.typeName, n.parent, n.templateIndex])));
    const refused = async (what: string, msg: any) => {
      const from = got.length;
      receive(msg);
      let note: any;
      for (let t = 0; t < 1500 && !note; t += 5) { note = got.slice(from).find((m) => m.type === "error" && m.request === msg.request); if (!note) await wait(5); }
      check(`inside a cell, ${what}`, !!note && /template/.test(note.message), JSON.stringify(got.slice(from)).slice(0, 300));
    };
    if (deep) {
      await refused("insert into the cell", { type: "insert", request: 40, parent: cell.id, typeName: "SkiaLabel" });
      await refused("move into the cell", { type: "move", request: 41, node: outside.AccessibilityId, parent: cell.id, index: 0 });
      await refused("move out of the cell", { type: "move", request: 42, node: deep.id, parent: top.id, index: 0 });
      await refused("remove", { type: "remove", request: 43, node: deep.id });
    }
    detach();
    root.render(<SkiaStack />);
    clearInterval(pump);
  }

  // place mode right after the page mounted a new snippet: the hover boxes the layout under the pointer, the click
  // puts the control there and answers inserted with its id (the pointer events come through hover / click here:
  // Node has no DOM)
  {
    const canvas = new TestCanvas(CK.MakeSurface(400, 300));
    const pump = setInterval(() => { if (canvas.requested) canvas.Draw(); }, 2);
    const root = createDrawnRoot(canvas as any);
    let layer: any, caption: any;
    root.render(
      <SkiaLayer ref={(c: any) => { if (c) layer = c; }} HorizontalOptions="Fill" VerticalOptions="Fill">
        <SkiaLabel ref={(c: any) => { if (c) caption = c; }} Text="Hi" WidthRequest={100} HeightRequest={30} />
      </SkiaLayer>,
    );
    await wait(30); canvas.Draw();
    const got: any[] = [];
    attachPage(canvas, (t) => got.push(JSON.parse(t)));
    for (let t = 0; t < 1000 && !got.some((m) => m.type === "tree"); t += 5) await wait(5);
    __test.state.pick = { unhook: () => {} } as any;
    __test.hover(20, 10);
    check("pick mode: the hover boxes the control under the pointer", __test.state.highlight?.deref() === caption);
    __test.state.pick = undefined;
    __test.state.highlight = undefined;
    __test.state.pick = { place: "SkiaLabel", unhook: () => {} } as any;
    __test.hover(200, 150);
    check("place mode: the hover boxes the layout under the pointer", __test.state.highlight?.deref() === layer);
    const from = got.length;
    __test.click(200, 150);
    for (let t = 0; t < 1000 && !got.slice(from).some((m) => m.type === "tree"); t += 5) await wait(5);
    const after = got.slice(from);
    const ins = after.find((m) => m.type === "inserted");
    const tree = after.find((m) => m.type === "tree");
    check("place mode: the click inserts and answers its id", !!ins?.node && ins.parent === layer.AccessibilityId && !after.some((m) => m.type === "error")
      && tree?.nodes.some((n: any) => n.id === ins.node && n.parent === layer.AccessibilityId), JSON.stringify(after).slice(0, 300));
    detach();
    root.render(<SkiaStack />);
    clearInterval(pump);
  }

  // where a placed control lands: into the control under the pointer when it holds children, at the index the pointer
  // is at; else next to it, before or after by the half the pointer is in; nothing inside a template's cell
  {
    const canvas = new TestCanvas(CK.MakeSurface(400, 300));
    const pump = setInterval(() => { if (canvas.requested) canvas.Draw(); }, 2);
    const root = createDrawnRoot(canvas as any);
    const r: any = {};
    const ref = (k: string) => (c: any) => { if (c) r[k] = c; };
    const tpl = () => { const c = new (Core as any).SkiaLayout(); c.WidthRequest = 80; c.HeightRequest = 20; return c; };
    root.render(
      <SkiaStack ref={ref("root")} Padding={10} Spacing={10}>
        <SkiaLabel ref={ref("a")} Text="A" WidthRequest={60} HeightRequest={20} />
        <SkiaStack ref={ref("stack")} Spacing={20} WidthRequest={200}>
          <SkiaLabel ref={ref("s1")} Text="s1" WidthRequest={60} HeightRequest={20} />
          <SkiaLabel Text="s2" WidthRequest={60} HeightRequest={20} />
        </SkiaStack>
        <SkiaRow ref={ref("row")} Spacing={10}>
          <SkiaLabel ref={ref("r1")} Text="r1" WidthRequest={60} HeightRequest={20} />
          <SkiaLabel Text="r2" WidthRequest={60} HeightRequest={20} />
        </SkiaRow>
        <SkiaLayer ref={ref("layer")} WidthRequest={200} HeightRequest={40}>
          <SkiaLabel Text="in" WidthRequest={60} HeightRequest={20} />
        </SkiaLayer>
        <SkiaStack ref={ref("list")} ItemsSource={["x"]} ItemTemplate={tpl} WidthRequest={100} HeightRequest={30} />
      </SkiaStack>,
    );
    await wait(30); canvas.Draw();
    const got: any[] = [];
    const receive = attachPage(canvas, (t) => got.push(JSON.parse(t)));
    for (let t = 0; t < 1000 && !got.some((m) => m.type === "tree"); t += 5) await wait(5);
    const rect = (c: any) => __test.walk().find((v: any) => v.control === c)!.rect as number[];
    const place = async (x: number, y: number): Promise<[number, number] | undefined> => {
      __test.state.pick = { place: "SkiaLabel", unhook: () => {} } as any;
      const from = got.length;
      __test.click(x, y);
      for (let t = 0; t < 1000 && !got.slice(from).some((m) => m.type === "inserted"); t += 5) await wait(5);
      const ins = got.slice(from).find((m) => m.type === "inserted");
      if (!ins?.node) return undefined;
      receive({ type: "remove", request: 90, node: ins.node });
      for (let t = 0; t < 1000 && !got.slice(from).some((m) => m.type === "tree" && !m.nodes.some((n: any) => n.id === ins.node)); t += 5) await wait(5);
      return [ins.parent, ins.index];
    };
    const id = (k: string) => r[k].AccessibilityId;
    const same = (a: any, b: any) => JSON.stringify(a) === JSON.stringify(b);
    const st = rect(r.stack), s1 = rect(r.s1);
    let at = await place(st[0] + 150, s1[1] + s1[3] + 10);
    check("place: into a stack, at the index the pointer is at", same(at, [id("stack"), 1]), JSON.stringify(at));
    const a = rect(r.a);
    at = await place(a[0] + 30, a[1] + 3);
    check("place: beside a label in a column, top half = before", same(at, [id("root"), 0]), JSON.stringify(at));
    at = await place(a[0] + 30, a[1] + a[3] - 3);
    check("place: beside a label in a column, bottom half = after", same(at, [id("root"), 1]), JSON.stringify(at));
    const r1 = rect(r.r1);
    at = await place(r1[0] + 3, r1[1] + 10);
    check("place: beside a label in a row, left half = before", same(at, [id("row"), 0]), JSON.stringify(at));
    at = await place(r1[0] + r1[2] - 3, r1[1] + 10);
    check("place: beside a label in a row, right half = after", same(at, [id("row"), 1]), JSON.stringify(at));
    const l = rect(r.layer);
    at = await place(l[0] + l[2] - 5, l[1] + l[3] - 5);
    check("place: a layer's empty area, last", same(at, [id("layer"), 1]), JSON.stringify(at));
    const cell = __test.walk().find((v: any) => v.templateIndex != null);
    at = cell ? await place(cell.rect[0] + 10, cell.rect[1] + 10) : [0, 0];
    check("place: nothing inside a template's cell", at === undefined, JSON.stringify(at));
    // the hover over the stack's gap boxes the stack, with a line across it at the index
    __test.state.pick = { place: "SkiaLabel", unhook: () => {} } as any;
    __test.hover(st[0] + 150, s1[1] + s1[3] + 10);
    const line = __test.state.placeLine;
    check("place: the hover boxes the stack with a line at the index", __test.state.highlight?.deref() === r.stack && !!line && line[3] - line[1] <= 2 && line[2] - line[0] >= st[2] - 1, JSON.stringify(line));
    __test.state.pick = undefined;
    // a drop from the editor's Controls onto the canvas: the same place, with no place mode
    const from = got.length;
    receive({ type: "placeAt", typeName: "SkiaLabel", x: a[0] + 30, y: a[1] + 3 });
    for (let t = 0; t < 1000 && !got.slice(from).some((m) => m.type === "inserted"); t += 5) await wait(5);
    const dropped = got.slice(from).find((m) => m.type === "inserted");
    check("placeAt: inserted as a place click there", dropped?.request === 0 && !!dropped.node && dropped.parent === id("root") && dropped.index === 0, JSON.stringify(got.slice(from)).slice(0, 200));
    detach();
    root.render(<SkiaStack />);
    clearInterval(pump);
  }

  // a new snippet: one empty layer filling the canvas; the place hover boxes it, inside the canvas's edges
  {
    const canvas = new TestCanvas(CK.MakeSurface(400, 300));
    const pump = setInterval(() => { if (canvas.requested) canvas.Draw(); }, 2);
    const root = createDrawnRoot(canvas as any);
    let layer: any;
    root.render(<SkiaLayer ref={(c: any) => { if (c) layer = c; }} HorizontalOptions="Fill" VerticalOptions="Fill" />);
    await wait(30); canvas.Draw();
    const got: any[] = [];
    attachPage(canvas, (t) => got.push(JSON.parse(t)));
    for (let t = 0; t < 1000 && !got.some((m) => m.type === "tree"); t += 5) await wait(5);
    __test.state.pick = { place: "SkiaLabel", unhook: () => {} } as any;
    __test.hover(200, 150);
    await wait(30); canvas.Draw();
    const px = (x: number, y: number) => Array.from(canvas.surface.makeImageSnapshot().readPixels(x, y, { width: 1, height: 1, colorType: CK.ColorType.RGBA_8888, alphaType: CK.AlphaType.Unpremul, colorSpace: CK.ColorSpace.SRGB }) as Uint8Array).join(",");
    const edges = [px(0, 150), px(200, 0), px(399, 150), px(200, 299)];
    check("a lone empty root: the place hover boxes it inside the canvas", __test.state.highlight?.deref() === layer && edges.every((p) => p === "255,0,255,255"), JSON.stringify(edges));
    __test.state.pick = undefined;
    detach();
    root.render(<SkiaStack />);
    clearInterval(pump);
  }

  // the name the code gives a control: the component whose own element it is (from its fiber, development builds);
  // drawnui-react's own components are looked through, an element written inside another one has none
  {
    const canvas = new TestCanvas(CK.MakeSurface(400, 300));
    const pump = setInterval(() => { if (canvas.requested) canvas.Draw(); }, 2);
    const root = createDrawnRoot(canvas as any);
    const kids: any[] = [];
    let stack: any;
    root.render(
      <SkiaStack ref={(c: any) => { if (c) stack = c; }}>
        {[0, 1, 2].map((i) => <SkiaLabel key={i} ref={(c: any) => { if (c) kids[i] = c; }} Text={`c${i}`} />)}
        <SkiaLabel Text="inline" />
      </SkiaStack>,
    );
    await wait(30); canvas.Draw();
    function Circle() { return null; }
    function App() { return null; }
    // a component of drawnui-react's own (looked through, as a wrapper of the app's element would be)
    const libComponent = Object.entries(Lib).find(([k, v]) => typeof v === "function" && /^[A-Z]/.test(k))?.[1];
    // Circle renders <SkiaLabel> (a host element) through a library component; App renders the stack; the last
    // label is written inside the stack.
    const fiberOf = (c: any) => {
      if (kids.includes(c)) return { type: "SkiaLabel", return: { type: libComponent, return: { type: Circle, return: { type: "SkiaStack" } } } };
      if (c === stack) return { type: "SkiaStack", return: { type: App } };
      return { type: "SkiaLabel", return: { type: "SkiaStack" } };
    };
    __test.setSourceOf((c: any) => ({ Fiber: fiberOf(c) }));
    const trees: any[] = [];
    attachPage(canvas, (t) => { const m = JSON.parse(t); if (m.type === "tree") trees.push(m); });
    for (let t = 0; t < 1000 && !trees.length; t += 5) await wait(5);
    detach();
    __test.setSourceOf(undefined);
    const nodes = trees[0]?.nodes ?? [];
    const by = (c: any) => nodes.find((n: any) => n.id === c.AccessibilityId)?.madeBy;
    const inline = nodes.find((n: any) => n.text === "inline");
    check("madeBy: Circle on each of its three labels, App on its stack, none written in place",
      // (the bundler may rename the functions: their own names are what the fiber says)
      kids.every((k) => by(k) === Circle.name) && by(stack) === App.name && inline && !("madeBy" in inline), JSON.stringify(nodes.map((n: any) => [n.typeName, n.text, n.madeBy])));
    root.render(<SkiaStack />);
    clearInterval(pump);
  }

  // only what can be set: a getter without a setter (a computed value) is not listed, an accessor with
  // one and a plain field are
  {
    class Probe extends Core.SkiaLabel {
      private size2 = 1;
      get WillClipEverything() { return true; }
      get Size2() { return this.size2; }
      set Size2(v: number) { this.size2 = v; }
    }
    const names = (__test.readProps(new Probe()) as any[]).map((p) => p.name);
    check("getter-only not listed; setter accessor and fields listed", !names.includes("WillClipEverything") && names.includes("Size2") && names.includes("IsClippedToBounds"), names.join(","));
  }

  // source map lookup: a TSX file through esbuild's JSX transform; the generated jsx(...) call maps back to its "<"
  const tsx = "import { SkiaLabel } from 'x';\nexport const Page = () => (\n  <SkiaLabel Text=\"a\" />\n);\n";
  const out = transformSync(tsx, { loader: "tsx", jsx: "automatic", sourcemap: "external", sourcefile: "Page.tsx", format: "esm" });
  const lines = out.code.split("\n"), li = lines.findIndex((l) => l.includes("jsx(SkiaLabel")), col = lines[li].indexOf("jsx(SkiaLabel");
  const at = __test.sourceMapLookup(JSON.parse(out.map), "http://localhost:5173/src/Page.tsx")(li + 1, col + 1);
  check("source map: generated JSX call -> the original <", at?.file === "/src/Page.tsx" && at.line === 3 && at.column === 3, JSON.stringify(at));

  console.log(failures ? `\n${failures} FAILED` : "\nall passed");
  process.exit(failures ? 1 : 0);
})();
