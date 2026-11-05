// DrawnUI Designer agent for DrawnUi.React apps (development builds only; the package's production entry is noop.ts).
// Speaks the designer protocol (crates/designer-protocol) over a WebSocket to the hub, or over a page channel that a
// host page relays (DrawFiddle). Reads the engine by duck typing so it works across drawnui-react versions; the engine
// additions (Canvas.PopupWindowRoots, Canvas.WasDrawnListeners, DesignerSourceOf) are used when present.
import * as Core from "drawnui-react/core";
import * as ReactEntry from "drawnui-react";

/* eslint-disable @typescript-eslint/no-explicit-any */
type Ctl = any;
type Ctor = new () => Ctl;
type EngineCanvas = any;

export interface AgentOptions {
  /** Hub address (WebSocket transport). Default `ws://127.0.0.1:54270`, or `globalThis.DRAWNUI_DESIGNER_URL`. */
  url?: string;
  /** Name shown in the designer. Default: the page title, else the host. */
  name?: string;
  /** The project the app is built from, when known (folder of its package.json). */
  project?: string;
  /** The app's screens, when the agent cannot find a SkiaShell by itself. */
  screens?: { routes: string[]; titles?: Record<string, string>; open: (route: string) => unknown };
  /** The app's own control classes for the Controls panel (classes seen in the tree are added by themselves). */
  controls?: Record<string, Ctor>;
  /**
   * Maps a served-module position (where DesignerSourceOf says the JSX is) to the original one, before the agent tries
   * the module's own source map; undefined = not this one. A page that evaluates code itself (DrawFiddle) maps its
   * snippet here, e.g. to `fiddle:///snippet.tsx` minus its preamble lines.
   */
  mapSource?: (file: string, line: number, column: number) => { file: string; line: number; column: number } | undefined;
}

const PROTOCOL = "0.1.0";
const DEFAULT_URL = "ws://127.0.0.1:54270";

// ---------------------------------------------------------------- public API

let wsStop: (() => void) | undefined;

/**
 * Connects the app to the designer's hub over a WebSocket and keeps trying quietly (1 s doubling to 30 s) while no
 * designer runs. Returns a function that disconnects. Attaching again replaces the previous attach.
 */
export function attach(canvas: EngineCanvas, options: AgentOptions = {}): () => void {
  detach();
  if (!canvas) return () => {};
  const url = options.url ?? (globalThis as any).DRAWNUI_DESIGNER_URL ?? DEFAULT_URL;
  state.canvas = canvas;
  state.options = options;
  let stopped = false, delay = 1000, timer: any, ws: WebSocket | undefined;
  console.info(`DrawnUI Designer agent: waiting for a designer at ${url}`);
  const retry = () => { if (!stopped) { timer = setTimeout(open, delay); delay = Math.min(delay * 2, 30000); } };
  const open = () => {
    if (stopped) return;
    let socket: WebSocket;
    try { socket = new WebSocket(url); } catch { retry(); return; }
    ws = socket;
    socket.onopen = () => { delay = 1000; startSession((text) => { if (socket.readyState === 1) socket.send(text); }); };
    socket.onmessage = (e) => receive(String(e.data));
    socket.onerror = () => {};
    socket.onclose = () => { if (ws === socket) { ws = undefined; endSession(); retry(); } };
  };
  open();
  const stop = () => {
    if (stopped) return;
    stopped = true;
    clearTimeout(timer);
    const s = ws; ws = undefined;
    endSession();
    try { s?.close(); } catch { /* closed already */ }
  };
  wsStop = stop;
  return stop;
}

/**
 * Page channel (DrawFiddle): the page relays the agent's messages to the designer iframe (`send` gets JSON text) and
 * hands back the designer's messages through the returned function (JSON text or the parsed object). The page sends
 * `{"type":"rebuilt"}` after it rebuilt the controls inside the same canvas; a new canvas = attachPage again.
 */
export function attachPage(canvas: EngineCanvas, send: (text: string) => void, options: AgentOptions = {}): (message: string | object) => void {
  detach();
  state.canvas = canvas;
  state.options = options;
  startSession(send);
  return receive;
}

/** Ends the current session, whichever transport: hooks off, highlight and pick mode gone. */
export function detach(): void {
  const stop = wsStop; wsStop = undefined;
  if (stop) stop(); else endSession();
}

// ---------------------------------------------------------------- session state

interface Pin { value: unknown; app: string; kind: Kind; accessor?: PropertyDescriptor; data?: PropertyDescriptor }

const state = {
  canvas: undefined as EngineCanvas,
  options: {} as AgentOptions,
  out: undefined as ((text: string) => void) | undefined,
  queue: Promise.resolve() as Promise<void>,
  ids: new Map<number, WeakRef<Ctl>>(),
  revision: 0,
  highlight: undefined as WeakRef<Ctl> | undefined,
  boxColor: "#FFFF00FF",
  dirty: false,
  frameWaiters: [] as (() => void)[],
  frameHook: undefined as (() => void) | undefined,
  lastCheck: 0,
  signature: "",
  pick: undefined as undefined | { place?: string; hover?: number; unhook: () => void },
  appClasses: new Map<string, Ctor>(),
};
const pins = new WeakMap<Ctl, Map<string, Pin>>();
/** The pinned controls, to take every pin off when the designer leaves. */
const pinned = new Set<WeakRef<Ctl>>();

/** Every pin off (the designer left): live values stay as they are, the next code value goes through as usual. */
function unpinAll(): void {
  for (const r of [...pinned]) { const c = r.deref(); if (c) for (const name of [...(pins.get(c)?.keys() ?? [])]) unpin(c, name); }
  pinned.clear();
}
/** React-owned controls the designer took out: hidden (IsVisible pinned false) until the code changes. */
const removed = new WeakSet<Ctl>();
/** Controls the designer inserted: the only ones it takes out (and moves) for real. */
const inserted = new WeakSet<Ctl>();
/**
 * Moves of React-owned controls: the original stays where React put it, hidden, and a stand-in (its copy, made by the
 * designer) shows at the new place under the original's id. React never sees a structural change, so its next commit
 * (a re-render, a reorder, an unmount) still finds every control where it left it. original -> stand-in.
 */
const moved = new Map<Ctl, Ctl>();
const standInOf = new WeakMap<Ctl, Ctl>();
const movedAway = new WeakSet<Ctl>();
/** The id a control has in the tree: a stand-in answers with its original's. */
const idOf = (c: Ctl): number => (standInOf.get(c) ?? c).AccessibilityId;
/** Left out of the tree, picks and child counts: taken out, or moved away (its stand-in shows instead). */
const gone = (c: Ctl) => removed.has(c) || movedAway.has(c);

function startSession(send: (text: string) => void): void {
  endSession();
  state.out = send;
  hookFrames(true);
  enqueue(async () => {
    const o = state.options;
    sendMsg({
      role: "app", protocol: PROTOCOL, engine: "react", platform: "browser", pid: 0, agent: null,
      name: o.name ?? ((typeof document !== "undefined" && document.title) || (typeof location !== "undefined" && location.host) || "app"),
      project: o.project ?? null,
    });
    await settle();
    const screens = readScreens();
    if (screens) sendMsg({ type: "screens", screens: screens.list });
    await sendTree();
  });
}

function endSession(): void {
  if (!state.out) return;
  try { setPick(undefined); setHighlight(undefined); hookFrames(false); dropStandIns(); unpinAll(); } catch { /* never into the app */ }
  state.out = undefined;
  state.queue = Promise.resolve();
  for (const w of state.frameWaiters.splice(0)) w();
}

function sendMsg(msg: object): void {
  try { state.out?.(JSON.stringify(msg)); } catch { /* a closed channel */ }
}

function sendError(request: number | null | undefined, message: string): void {
  sendMsg({ type: "error", request: request ?? null, message });
}

/** One message at a time, in order; a handler never throws into the transport. */
function receive(message: string | object): void {
  let msg: any;
  try { msg = typeof message === "string" ? JSON.parse(message) : message; } catch { sendError(null, "not JSON: " + String(message).slice(0, 80)); return; }
  enqueue(() => handle(msg));
}

function enqueue(work: () => Promise<void> | void): void {
  state.queue = state.queue.then(work).catch((e) => sendError(null, "agent: " + errorText(e)));
}

const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

// ---------------------------------------------------------------- messages

async function handle(msg: any): Promise<void> {
  const request: number | undefined = typeof msg?.request === "number" ? msg.request : undefined;
  try {
    switch (msg?.type) {
      case "getTree":
        await settle();
        await sendTree();
        break;
      case "rebuilt": {
        await settle(true);
        const shown = state.highlight?.deref();
        if (shown && !walk().some((v) => v.control === shown)) setHighlight(undefined);
        await sendTree();
        break;
      }
      case "getProps": {
        await settle();
        sendMsg({ type: "props", node: msg.node, props: readProps(find(msg.node)) });
        break;
      }
      case "setProperty":
        setLive(find(msg.node), String(msg.name), String(msg.value ?? ""));
        break;
      case "pick":
        await settle();
        sendMsg({ type: "picked", request: request ?? 0, node: pickAt(msg.x, msg.y) ?? null });
        break;
      case "pickMode":
        setPick(msg.on ? {} : undefined);
        break;
      case "placeMode":
        if (typeof msg.typeName === "string") { classOf(msg.typeName); setPick({ place: msg.typeName }); }
        else if (state.pick?.place) setPick(undefined);
        break;
      case "highlight":
        state.boxColor = typeof msg.color === "string" ? msg.color : "#FFFF00FF";
        setHighlight(msg.node == null ? undefined : find(msg.node));
        break;
      case "snapshot":
        sendMsg({ type: "snapshot", request: request ?? 0, ...snapshot() });
        break;
      case "getControls":
        sendMsg({ type: "controls", controls: controlKinds() });
        break;
      case "insert": {
        let node: number | null = null, index: number | null = msg.index ?? null;
        try {
          const skipped: string[] = [];
          const r = insertControl(find(msg.parent), msg.index ?? undefined, String(msg.typeName), pairs(msg.props), Array.isArray(msg.children) ? msg.children : [], skipped);
          node = r.id; index = r.index;
          if (skipped.length) sendError(null, `inserted ${msg.typeName} without ${skipped.join("; ")}`);
        } catch (e) { sendError(request, errorText(e)); }
        sendMsg({ type: "inserted", request: request ?? 0, node, parent: msg.parent ?? null, index });
        if (node != null) { await settle(); await sendTree(); }
        break;
      }
      case "remove":
        removeControl(find(msg.node));
        await settle();
        await sendTree();
        break;
      case "move": {
        let at: number;
        try { at = moveControl(find(msg.node), find(msg.parent), typeof msg.index === "number" ? msg.index : undefined); }
        catch (e) {
          sendError(request, errorText(e));
          sendMsg({ type: "moved", request: request ?? 0, node: null, parent: msg.parent ?? null, index: msg.index ?? null });
          break;
        }
        sendMsg({ type: "moved", request: request ?? 0, node: msg.node, parent: msg.parent, index: at });
        await settle();
        await sendTree();
        break;
      }
      case "copy": {
        const c = state.ids.get(msg.node)?.deref();
        sendMsg({ type: "copied", request: request ?? 0, spec: c && !c.IsDisposed ? specOf(c) : null });
        break;
      }
      case "openScreen":
        await openScreen(String(msg.route));
        break;
      case "focus":
        break; // a browser tab cannot bring itself to the front
      default:
        sendError(request, "unknown message: " + JSON.stringify(msg).slice(0, 200));
    }
  } catch (e) {
    sendError(request, errorText(e));
  }
}

const pairs = (v: unknown): [string, string][] =>
  Array.isArray(v) ? v.filter((p) => Array.isArray(p) && p.length === 2).map((p) => [String(p[0]), typeof p[1] === "string" ? p[1] : JSON.stringify(p[1])]) : [];

// ---------------------------------------------------------------- frames

/**
 * The agent's after-frame hook, on while a designer is connected: the highlight box, waiters for a fresh layout, and
 * every 500 ms a cheap check whether the tree's structure changed (a navigation, a hot reload) to push a new tree.
 */
function onFrame(): void {
  try {
    drawHighlight();
    state.dirty = false;
    for (const w of state.frameWaiters.splice(0)) w();
    const now = Date.now();
    if (state.out && now - state.lastCheck > 500) {
      state.lastCheck = now;
      setTimeout(() => enqueue(checkStructure), 0);
    }
  } catch { /* never into the app's frame */ }
}

function hookFrames(on: boolean): void {
  const canvas = state.canvas;
  if (!on) { state.frameHook?.(); state.frameHook = undefined; return; }
  if (state.frameHook || !canvas) return;
  const listeners: Set<(c: EngineCanvas) => void> | undefined = canvas.WasDrawnListeners;
  if (listeners && typeof listeners.add === "function") {
    listeners.add(onFrame);
    state.frameHook = () => listeners.delete(onFrame);
    return;
  }
  // drawnui-react without WasDrawnListeners: one WasDrawn slot, which <Canvas WasDrawn> rewrites on every render. An
  // accessor keeps the app's handler (what the app sets goes there) and runs the agent's after it.
  let app = canvas.WasDrawn;
  const run = (c: EngineCanvas) => { try { app?.(c); } finally { onFrame(); } };
  Object.defineProperty(canvas, "WasDrawn", { configurable: true, enumerable: true, get: () => run, set: (v) => { app = v; } });
  state.frameHook = () => { delete canvas.WasDrawn; canvas.WasDrawn = app; };
}

/** Rects are current only after a frame: when the agent changed something (or `force`), ask for one and wait for it
 * (at most 250 ms: a background tab draws no frames). */
function settle(force = false): Promise<void> {
  if (!force && !state.dirty) return Promise.resolve();
  return new Promise((resolve) => {
    const t = setTimeout(done, 250);
    function done() { clearTimeout(t); resolve(); }
    state.frameWaiters.push(done);
    state.canvas?.Update?.();
  });
}

function changed(): void {
  state.dirty = true;
  state.canvas?.Update?.();
}

async function checkStructure(): Promise<void> {
  if (!state.out) return;
  const sig = signature(walk());
  if (sig !== state.signature) await sendTree();
}

const signature = (visits: Visit[]) => visits.map((v) => `${v.id}<${v.parent ?? ""}${v.visible ? "" : "h"}`).join(",");

// ---------------------------------------------------------------- tree

interface Visit { control: Ctl; id: number; parent?: number; internal: boolean; templateIndex?: number; rect: number[]; visible: boolean; host?: string }

/**
 * Class -> name from the library's export tables, whose keys survive minification (DrawFiddle loads a minified build:
 * `constructor.name` is "ae" there). Built once. A class not exported keeps its own name, unless the library is
 * minified and the name looks mangled: then it is a library class (only the library's bundle is minified) and is named
 * after its nearest exported base.
 */
let exportNames: Map<Function, string> | undefined;
let minifiedLibrary = false;
function names(): Map<Function, string> {
  if (exportNames) return exportNames;
  exportNames = new Map();
  const add = (table: unknown) => {
    if (!table || typeof table !== "object") return;
    for (const [name, value] of Object.entries(table)) if (typeof value === "function" && !exportNames!.has(value)) exportNames!.set(value, name);
  };
  add(Core);
  add(ReactEntry);
  minifiedLibrary = (Core as any).SkiaControl?.name !== "SkiaControl";
  return exportNames;
}
const mangled = (ctor: Function) => !names().has(ctor) && minifiedLibrary && /^[A-Za-z_$][\w$]{0,2}$|^$/.test(ctor.name ?? "");

function nameOf(ctor: Function | undefined): string {
  for (let k: any = ctor; k && k !== Object && k !== Function.prototype; k = Object.getPrototypeOf(k)) {
    const exported = names().get(k);
    if (exported) return exported;
    if (!mangled(k)) return String(k.name || "Control").replace(/^_+/, "");
  }
  return "Control";
}

const typeOf = (c: Ctl): string => nameOf(c?.constructor);

/** Classes the library exports (by export name), for the Controls panel and to create controls by type name. */
let libraryClasses: Map<string, Ctor> | undefined;
function library(): Map<string, Ctor> {
  if (libraryClasses) return libraryClasses;
  libraryClasses = new Map();
  const base = (Core as any).SkiaControl;
  for (const [name, value] of Object.entries(Core)) {
    if (typeof value === "function" && base && (value === base || value.prototype instanceof base)) libraryClasses.set(name, value as Ctor);
  }
  return libraryClasses;
}
/** The library's class (exported, or a minified one it does not export), not the app's. */
const isLibrary = (c: Ctl) => !!c?.constructor && (names().has(c.constructor) || mangled(c.constructor));

/** The children a control shows: a layout's Views (a templated one lists its realized cells), else its Content. */
function childrenOf(c: Ctl): Ctl[] {
  const views = c?.Views;
  if (Array.isArray(views)) return views.filter((v: Ctl) => v && !v.IsDisposed);
  const content = c?.Content;
  return content && typeof content === "object" && "AccessibilityId" in content ? [content] : [];
}

function popupRoots(canvas: EngineCanvas): Ctl[] {
  try { return Array.isArray(canvas?.PopupWindowRoots) ? [...canvas.PopupWindowRoots] : []; } catch { return []; }
}

function walk(): Visit[] {
  const canvas = state.canvas, out: Visit[] = [];
  if (!canvas) return out;
  // React unmounted a moved original (its code changed: the move was written, or the component mounted again): its
  // stand-in goes, the code's own control (or the hub's replay of the move) takes the place
  for (const [original, standIn] of [...moved]) if (original.IsDisposed) dropStandIn(original, standIn, false);
  const w = canvas.Element?.width ?? 0, h = canvas.Element?.height ?? 0;
  const seen = new Set<Ctl>();
  const visit = (c: Ctl, parent: Ctl | undefined, parentVisible: boolean, parentInternal: boolean, host?: string) => {
    if (!c || seen.has(c) || gone(c)) return;
    seen.add(c);
    const id = idOf(c);
    state.ids.set(id, new WeakRef(c));
    const r = rectOf(c);
    const shown = parentVisible && c.IsVisible !== false;
    const visible = shown && r[2] > 0 && r[3] > 0 && r[0] < w && r[1] < h && r[0] + r[2] > 0 && r[1] + r[3] > 0;
    const templateIndex = parent?.IsTemplated && typeof c.ContextIndex === "number" && c.ContextIndex >= 0 ? c.ContextIndex : undefined;
    const src = sourceOf(c);
    // the library made it for its own parts: a library class (never the app's) the app did not add as a child
    // (JSX children and Children are IsChildrenItem), not a cell, not inserted by the designer, not from app JSX
    const fromLibraryJsx = !!src?.File && /node_modules|drawnui-react\/(dist|src)/.test(String(src.File));
    const internal = !!parent && templateIndex === undefined && !inserted.has(c) && isLibrary(c)
      && (fromLibraryJsx || (!src && (parentInternal || !c.IsChildrenItem)));
    if (!isLibrary(c)) state.appClasses.set(typeOf(c), c.constructor);
    out.push({ control: c, id, parent: parent ? idOf(parent) : undefined, internal, templateIndex, rect: r, visible, host });
    for (const child of childrenOf(c)) visit(child, c, shown, internal);
  };
  visit(canvas.Content, undefined, true, false, hostName(canvas.Content));
  popupRoots(canvas).forEach((p, i) => visit(p, undefined, true, false, `popup ${i + 1}`));
  // forget controls that are gone
  for (const [id, ref] of state.ids) { const c = ref.deref(); if (!c || c.IsDisposed) { state.ids.delete(id); if (c) for (const n of [...(pins.get(c)?.keys() ?? [])]) unpin(c, n); } }
  return out;
}

function rectOf(c: Ctl): number[] {
  try {
    const r = c.GetAccessibilityPixelRect?.() ?? c.DrawingRect;
    const v = [r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top].map((x: number) => (Number.isFinite(x) ? x : 0));
    return v;
  } catch { return [0, 0, 0, 0]; }
}

function textPreview(c: Ctl): string | undefined {
  if (!["SkiaLabel", "SkiaButton", "SkiaEditor", "SkiaRichLabel"].some((n) => inherits(c, n))) return undefined;
  let text = typeof c.Text === "string" ? c.Text : "";
  if (!text.trim() && typeof c.Placeholder === "string") text = c.Placeholder;
  text = text.split(/\s+/).filter(Boolean).join(" ");
  if (!text) return undefined;
  return text.length > 40 ? text.slice(0, 39) + "…" : text;
}

function inherits(c: Ctl, name: string): boolean {
  for (let p = c ? Object.getPrototypeOf(c) : null; p && p !== Object.prototype; p = Object.getPrototypeOf(p)) if (names().get(p.constructor) === name || (!minifiedLibrary && p.constructor?.name === name)) return true;
  return false;
}

async function sendTree(): Promise<void> {
  const canvas = state.canvas;
  if (!state.out || !canvas) return;
  let visits = walk();
  // controls not laid out yet (attached before the first frame, React still mounting): their rects are empty and an
  // editor would take them as hidden (folded, for good); wait for the frame that lays them out
  if (canvas.WasRendered === false || visits.some((v) => v.control.NeedMeasure === true)) { await settle(true); visits = walk(); }
  state.signature = signature(visits);
  const sources = await locate(visits);
  sendMsg({
    type: "tree",
    revision: ++state.revision,
    width: canvas.Element?.width ?? 0,
    height: canvas.Element?.height ?? 0,
    nodes: visits.map((v) => {
      const node: any = {
        id: v.id, parent: v.parent ?? null, typeName: typeOf(v.control), assembly: isLibrary(v.control) ? "drawnui-react" : "app",
        tag: typeof v.control.Tag === "string" && v.control.Tag ? v.control.Tag : null, text: textPreview(v.control) ?? null,
        internal: v.internal, templateIndex: v.templateIndex ?? null, field: null, host: v.host ?? null, rect: v.rect, visible: v.visible,
      };
      // a layout that makes its children from ItemsSource / ItemTemplate, even with no items yet
      if (v.control.IsTemplated || v.control.ItemTemplate) node.templated = true;
      const s = sources.get(v.id);
      if (s) node.source = s;
      return node;
    }),
  });
}

function find(id: unknown): Ctl {
  const c = typeof id === "number" ? state.ids.get(id)?.deref() : undefined;
  if (!c || c.IsDisposed) throw new Error(`node ${String(id)} not found (request a new tree)`);
  return c;
}

// ---------------------------------------------------------------- where JSX made a control (DesignerSourceOf)

interface DesignerSource { Fiber?: any; Component?: string; Stack?: string; File?: string; Line?: number; Column?: number }

// looked up by a computed name: bundlers warn on a missing static export (drawnui-react before the accessor). Absent,
// or undefined for a control (React's production build has no debug info: DrawFiddle), the node goes without a source.
let designerSourceOf = (ReactEntry as Record<string, unknown>)[["Designer", "SourceOf"].join("")] as ((c: Ctl) => DesignerSource | undefined) | undefined;

function sourceOf(c: Ctl): DesignerSource | undefined {
  if (typeof designerSourceOf !== "function" || !c) return undefined;
  try { return designerSourceOf(c) ?? undefined; } catch { return undefined; }
}

/** The nearest function component above the root control (the page), for the root's `host`. */
function hostName(root: Ctl): string | undefined {
  for (let f = sourceOf(root)?.Fiber?.return, n = 0; f && n < 40; f = f.return, n++) {
    const t = f.type;
    const name = typeof t === "function" ? t.displayName ?? t.name : t?.render ? t.displayName ?? t.render.name : undefined;
    if (name && name !== "Canvas") return name;
  }
  return undefined;
}

/** "file:line:column" (1-based, the original .tsx) per node, from DesignerSourceOf's served-module position. */
async function locate(visits: Visit[]): Promise<Map<number, string>> {
  const found = new Map<number, string>();
  const wanted: [number, DesignerSource][] = [];
  for (const v of visits) { const s = sourceOf(v.control); if (s?.File && s.Line) wanted.push([v.id, s]); }
  if (!wanted.length) return found;
  const mapped = (s: DesignerSource) => { try { return state.options.mapSource?.(s.File!, s.Line!, s.Column ?? 1); } catch { return undefined; } };
  const files = [...new Set(wanted.filter(([, s]) => !mapped(s)).map(([, s]) => s.File!))];
  const maps = new Map<string, SourceMapLookup | null>();
  await Promise.race([
    Promise.all(files.map(async (f) => { maps.set(f, await sourceMapOf(f)); })),
    new Promise((r) => setTimeout(r, 1500)),
  ]);
  for (const [id, s] of wanted) {
    const map = maps.get(s.File!);
    const at = mapped(s) ?? (map ? map(s.Line!, s.Column ?? 1) : undefined);
    found.set(id, at ? `${at.file}:${at.line}:${at.column}` : `${s.File}:${s.Line}:${s.Column ?? 1}`);
  }
  return found;
}

export type SourceMapLookup = (line: number, column: number) => { file: string; line: number; column: number } | undefined;
const sourceMaps = new Map<string, Promise<SourceMapLookup | null>>();

/** The served module's source map (inline data URL or a sibling file), fetched once per file. */
function sourceMapOf(file: string): Promise<SourceMapLookup | null> {
  let p = sourceMaps.get(file);
  if (!p) {
    p = (async () => {
      try {
        const url = file.replace(/\?.*$/, "") === file ? file : file;
        const code = await (await fetch(url)).text();
        const m = /\/\/[#@] sourceMappingURL=(\S+)\s*$/.exec(code);
        if (!m) return null;
        const mapUrl = new URL(m[1], url).href;
        const json = mapUrl.startsWith("data:") ? JSON.parse(decodeData(mapUrl)) : await (await fetch(mapUrl)).json();
        return sourceMapLookup(json, url);
      } catch { return null; }
    })();
    sourceMaps.set(file, p);
  }
  return p;
}

function decodeData(url: string): string {
  const comma = url.indexOf(",");
  const meta = url.slice(0, comma), body = url.slice(comma + 1);
  if (!/;base64/.test(meta)) return decodeURIComponent(body);
  const bin = atob(body), bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return new TextDecoder().decode(bytes);
}

const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/** A lookup over a v3 source map: generated (line, column), 1-based, to the original position (the last mapping at
 * or before that column on that line). The file is the map's source as the bundler names it, made absolute against
 * the module URL and reduced to its path. */
export function sourceMapLookup(map: any, moduleUrl: string): SourceMapLookup {
  const lines: number[][][] = [];
  let src = 0, oLine = 0, oCol = 0;
  for (const text of String(map.mappings ?? "").split(";")) {
    const segs: number[][] = [];
    let gCol = 0;
    for (const seg of text.split(",")) {
      if (!seg) continue;
      const v: number[] = [];
      let value = 0, shift = 0;
      for (const ch of seg) {
        const d = B64.indexOf(ch);
        value += (d & 31) << shift;
        if (d & 32) { shift += 5; continue; }
        v.push(value & 1 ? -(value >>> 1) : value >>> 1);
        value = 0; shift = 0;
      }
      gCol += v[0];
      if (v.length >= 4) { src += v[1]; oLine += v[2]; oCol += v[3]; segs.push([gCol, src, oLine, oCol]); }
    }
    lines.push(segs);
  }
  const root = map.sourceRoot ? String(map.sourceRoot).replace(/\/?$/, "/") : "";
  const names = (map.sources ?? []).map((s: string) => {
    const s2 = root + s;
    if (/^[a-z]+:\/\/\//i.test(s2) || /^[A-Za-z]:[\\/]/.test(s2)) return s2;
    try { const u = new URL(s2, moduleUrl); return u.protocol === "http:" || u.protocol === "https:" ? decodeURIComponent(u.pathname) : u.href; } catch { return s2; }
  });
  return (line, column) => {
    const segs = lines[line - 1];
    if (!segs?.length) return undefined;
    let hit = segs[0];
    for (const s of segs) { if (s[0] <= column - 1) hit = s; else break; }
    return { file: names[hit[1]] ?? "?", line: hit[2] + 1, column: hit[3] + 1 };
  };
}

// ---------------------------------------------------------------- properties

type Kind = "number" | "bool" | "string" | "color" | "thickness" | "corner";

/** Engine state the app does not set (the engine writes it), and runtime lists / handlers. */
const NOT_EDITABLE = new Set([
  "RenderingScale", "NeedMeasure", "AccessibilityId", "IsDisposed", "IsParentIndependent", "IsChildrenItem", "ContextIndex",
  "VirtualisationInflated", "IsRenderingWithComposition", "FirstVisibleIndex", "LastVisibleIndex", "LastMeasuredIndex",
  "LayoutVersion", "NeedsGlyphPositions", "IsPressed", "IsUserPanning", "IsUserFocused", "IsScrolling", "TotalDown", "TotalTapped",
  "StartThumbX", "EndThumbX", "IsLoading", "ViewportOffsetX", "ViewportOffsetY", "ContentOffset", "IsRefreshing", "Value1", "Value2",
  "IsHovered", "BackgroundMeasurementBatchSize", "ItemsSource", "ItemTemplate", "Children", "Views", "Parent", "BindingContext",
]);
/** Strings whose default is undefined (the type is not readable from the value). */
const STRING_NAMES = new Set(["Tag", "Text", "Source", "Tooltip", "AccessibilityLabel", "AccessibilityHint", "AccessibilityRole", "AccessibilityLive", "FontFamily", "FontFamilyFallback", "ColumnDefinitions", "RowDefinitions", "Placeholder"]);

const LAYOUT_OPTIONS = ["Start", "Center", "End", "Fill"];
const CHOICES: Record<string, string[]> = {
  HorizontalOptions: LAYOUT_OPTIONS, VerticalOptions: LAYOUT_OPTIONS,
  HorizontalTextAlignment: ["Start", "Center", "End", "FillWords", "FillWordsFull", "FillCharacters", "FillCharactersFull"],
  VerticalTextAlignment: ["Start", "Center", "End"],
  LineBreakMode: ["NoWrap", "WordWrap", "CharacterWrap", "HeadTruncation", "MiddleTruncation", "TailTruncation"],
  FontAttributes: ["None", "Bold", "Italic", "BoldItalic"], TextTransform: ["None", "Lowercase", "Uppercase", "Titlecase"],
  UseCache: ["None", "Operations", "OperationsFull", "Image", "ImageDoubleBuffered", "ImageComposite", "ImageCompositeGPU", "GPU"],
  Orientation: ["Vertical", "Horizontal", "Both", "Neither"], ScrollBarsVisibility: ["None", "Vertical", "Horizontal", "Both"],
  Aspect: ["None", "Fill", "Fit", "AspectFit", "AspectFill", "AspectFitFill", "FitFill", "Cover", "AspectCover", "Tile"],
  HorizontalAlignment: ["Start", "Center", "End"], VerticalAlignment: ["Start", "Center", "End"],
  LockChildrenGestures: ["Disabled", "Enabled", "PassNone", "PassTap", "PassTapAndLongPress"],
  AnimationTapped: ["None", "Ripple", "Shimmer"], ApplyEffect: ["None", "Ripple", "Shimmer"],
  RecyclingTemplate: ["Enabled", "Disabled"], MeasureItemsStrategy: ["MeasureAll", "MeasureFirst", "MeasureVisible"],
  SnapToChildren: ["Disabled", "Center", "Side"], StrokeCap: ["Butt", "Round", "Square"], BevelType: ["None", "Bevel", "Emboss"],
  ControlStyle: ["Unset", "Platform", "Cupertino", "Material", "Material3", "Windows"], LoadPriority: ["Low", "Normal", "High"],
};
const LAYOUT_TYPES = ["Absolute", "Column", "Row", "Wrap", "Grid", "Masonry"];
const SHAPE_TYPES = ["Rectangle", "Circle", "Ellipse", "Arc", "Squricle", "Path", "Polygon", "Line", "Custom"];
const TYPE_NAMES: Record<Kind, string> = {
  number: "System.Double", bool: "System.Boolean", string: "System.String", color: "DrawnUi.Color",
  thickness: "DrawnUi.Draw.Thickness", corner: "DrawnUi.Draw.CornerRadius",
};

/** As DrawnUi.Net's agent: where an editor lists a property, from its name. */
function groupOf(name: string): string {
  const starts = (...p: string[]) => p.some((x) => name.startsWith(x));
  const has = (...p: string[]) => p.some((x) => name.includes(x));
  if (starts("Accessibility", "UpdateWhen")) return "Behavior";
  if (starts("Translation", "Rotation", "Scale", "Skew", "Anchor", "Perspective")) return "Transform";
  if (has("Text", "Font") || starts("Line", "MaxLines", "Character", "Paragraph", "Placeholder")) return "Text";
  if (has("Color", "Stroke", "Corner", "Opacity", "Shadow", "Blur", "Gradient", "Tint", "Aspect", "BlendMode") ||
    starts("Background", "Brightness", "Contrast", "Saturation", "Gamma", "Lighten", "Darken", "Shape", "Bevel")) return "Look";
  if (has("Width", "Height", "Margin", "Padding", "Options", "Spacing", "FillRatio", "Row", "Column") ||
    starts("Split", "LockRatio", "ZIndex", "Left", "Top", "Type")) return "Layout";
  return "Behavior";
}

/** One default instance per class: the values a control starts with (never disposed, never attached). */
const defaults = new Map<Function, Ctl | null>();
function defaultOf(c: Ctl): Ctl | null {
  const ctor = c.constructor;
  if (!defaults.has(ctor)) { try { defaults.set(ctor, new ctor()); } catch { defaults.set(ctor, null); } }
  return defaults.get(ctor)!;
}

function descriptorOf(o: object, name: string): PropertyDescriptor | undefined {
  for (let p: object | null = o; p && p !== Object.prototype; p = Object.getPrototypeOf(p)) {
    const d = Object.getOwnPropertyDescriptor(p, name);
    if (d) return d;
  }
  return undefined;
}

/** PascalCase properties a person can set: own fields and accessors with a setter, per class. */
const propNames = new Map<Function, string[]>();
function namesOf(c: Ctl): string[] {
  let names = propNames.get(c.constructor);
  if (names) return names;
  const all = new Set<string>();
  const sample = defaultOf(c) ?? c;
  for (let o = sample; o && o !== Object.prototype; o = Object.getPrototypeOf(o)) for (const k of Object.getOwnPropertyNames(o)) if (/^[A-Z]/.test(k)) all.add(k);
  names = [...all].filter((k) => {
    if (NOT_EDITABLE.has(k)) return false;
    const d = descriptorOf(sample, k);
    return !!d && (d.get ? !!d.set : d.writable !== false) && typeof (d.get ? undefined : d.value) !== "function";
  }).sort();
  propNames.set(c.constructor, names);
  return names;
}

function kindOf(c: Ctl, name: string): Kind | undefined {
  const pin = pins.get(c)?.get(name);
  if (pin) return pin.kind;
  const v = c[name], d = defaultOf(c)?.[name];
  const value = v ?? d;
  if (typeof value === "function") return undefined;
  if (typeof value === "number") return "number";
  if (typeof value === "boolean") return "bool";
  if (typeof value === "string") return name.endsWith("Color") ? "color" : "string";
  if (value === undefined || value === null) return name.endsWith("Color") ? "color" : STRING_NAMES.has(name) ? "string" : undefined;
  if (value instanceof (Core as any).Thickness) return "thickness";
  if (value instanceof (Core as any).CornerRadius) return "corner";
  return undefined;
}

function hex2(n: number): string { return Math.round(Math.min(1, Math.max(0, n)) * 255).toString(16).padStart(2, "0").toUpperCase(); }

/** #AARRGGBB, whatever color text the app used (DrawnUi strings are alpha-first). */
function colorHex(v: unknown): string {
  if (typeof v !== "string" || !v) return "";
  try {
    const c = (Core as any).Super.ParseColor(v);
    return `#${hex2(c[3])}${hex2(c[0])}${hex2(c[1])}${hex2(c[2])}`;
  } catch { return v; }
}

const num = (n: number) => String(Math.round(n * 10000) / 10000);

function format(kind: Kind, v: any): string {
  switch (kind) {
    case "number": return typeof v === "number" ? num(v) : "";
    case "bool": return v ? "true" : "false";
    case "color": return colorHex(v);
    case "thickness": return v ? [v.Left, v.Top, v.Right, v.Bottom].map(num).join(",") : "";
    case "corner": {
      if (!v) return "";
      const n = [v.TopLeft, v.TopRight, v.BottomLeft, v.BottomRight];
      return n.every((x) => x === n[0]) ? num(n[0]) : n.map(num).join(",");
    }
    default: return v == null ? "" : String(v);
  }
}

function parse(kind: Kind, text: string): unknown {
  const t = text.trim();
  switch (kind) {
    case "number": { const n = Number(t); if (!t || !Number.isFinite(n)) throw new Error(`"${text}" is not a number`); return n; }
    case "bool": if (/^(true|false)$/i.test(t)) return /^true$/i.test(t); throw new Error(`"${text}" is not true or false`);
    case "color": return t ? t : undefined;
    case "thickness": { const v = (Core as any).Thickness.From(t); if (!v || !t) throw new Error(`"${text}" is not a thickness (l,t,r,b)`); return v; }
    case "corner": {
      const n = t.split(",").map((s) => Number(s.trim()));
      if (!t || !n.every(Number.isFinite) || n.length > 4) throw new Error(`"${text}" is not a corner radius`);
      return new (Core as any).CornerRadius(n[0], n[1], n[2], n[3]);
    }
    default: return text;
  }
}

function readProps(c: Ctl): object[] {
  const def = defaultOf(c);
  const rows: object[] = [];
  for (const name of namesOf(c)) {
    const kind = kindOf(c, name);
    if (!kind) continue;
    let value: string, isDefault: boolean | null = null;
    try {
      value = format(kind, c[name]);
      if (def) isDefault = value === format(kind, def[name]);
    } catch (e) { value = `<${errorText(e)}>`; }
    const choices = name === "Type" ? (inherits(c, "SkiaShape") ? SHAPE_TYPES : LAYOUT_TYPES) : CHOICES[name] ?? null;
    rows.push({
      name, typeName: TYPE_NAMES[kind], value, isSet: isDefault === false || !!pins.get(c)?.has(name),
      ...(isDefault === null ? {} : { isDefault }), group: groupOf(name), choices, flags: false,
    });
  }
  return rows;
}

/** As the reconciler's applyProps: transforms repaint, looks redraw their area, anything else measures again. */
const REPAINT_ONLY = new Set(["TranslationX", "TranslationY", "TranslationZ", "Rotation", "RotationX", "RotationY", "RotationZ", "ScaleX", "ScaleY", "Scale", "SkewX", "SkewY", "AnchorX", "AnchorY", "Opacity"]);
function invalidate(c: Ctl, name: string): void {
  if (REPAINT_ONLY.has(name)) c.RepaintComposition?.();
  else if (name.endsWith("Color") || name === "FillGradient" || name === "StrokeGradient") (c.UpdateDraw ?? c.Update)?.call(c);
  else c.Update?.();
  changed();
}

function setLive(c: Ctl, name: string, text: string): void {
  if (!namesOf(c).includes(name)) throw new Error(`${typeOf(c)} has no settable property ${name}`);
  const kind = kindOf(c, name);
  if (!kind) throw new Error(`${typeOf(c)}.${name} cannot be set from text`);
  const value = parse(kind, text);
  const p = pins.get(c)?.get(name);
  // back to the code's value (a live Undo), a control the designer made, or no change: no pin
  if (p ? format(kind, value) === p.app : inserted.has(c) || format(kind, value) === format(kind, c[name])) { unpin(c, name); c[name] = value; }
  else pin(c, name, kind, value);
  invalidate(c, name);
}

/**
 * A live value stays while React renders the component again: inline props (`Margin={new Thickness(..)}`) are written
 * again on every render, with the value the code had. The pin takes the property over on this one instance: a write
 * of the app's old value is dropped (a re-render), any other value (the code changed and hot reload rendered it, or the
 * engine set it) ends the pin and goes through. No work per frame.
 */
function pin(c: Ctl, name: string, kind: Kind, value: unknown): void {
  let map = pins.get(c);
  if (!map) { pins.set(c, (map = new Map())); pinned.add(new WeakRef(c)); }
  let p = map.get(name);
  if (!p) {
    const own = Object.getOwnPropertyDescriptor(c, name);
    const accessor = own ? undefined : descriptorOf(c, name);
    const entry: Pin = { value, app: format(kind, c[name]), kind, accessor: accessor?.set ? accessor : undefined, data: own };
    p = entry;
    map.set(name, entry);
    Object.defineProperty(c, name, {
      configurable: true, enumerable: own?.enumerable ?? false,
      get: () => (entry.accessor ? entry.accessor.get!.call(c) : entry.value),
      set: (v) => {
        if (format(kind, v) === entry.app) return; // a render writing the old code value again
        unpin(c, name);
        c[name] = v;
      },
    });
  }
  p.value = value;
  if (p.accessor) p.accessor.set!.call(c, value);
}

/** Takes the pin off: the class's accessor (or a plain field) is back, holding the value the control has now. */
function unpin(c: Ctl, name: string): void {
  const map = pins.get(c), p = map?.get(name);
  if (!p) return;
  map!.delete(name);
  if (!map!.size) { pins.delete(c); pinned.forEach((r) => { if (r.deref() === c) pinned.delete(r); }); }
  const current = p.accessor ? undefined : p.value;
  delete c[name];
  if (!p.accessor) Object.defineProperty(c, name, { ...(p.data ?? { enumerable: true }), configurable: true, writable: true, value: current });
}

// ---------------------------------------------------------------- pick, place, highlight

/** Deepest control at a canvas pixel, children top-most first in their own (transformed) space. Without `layers`,
 * a layout that draws nothing of its own (no background) is looked through when no child is hit there. */
function hitPath(x: number, y: number, layers: boolean): Ctl[] {
  const canvas = state.canvas;
  const hit = (c: Ctl, px: number, py: number, path: Ctl[]): boolean => {
    if (!c || c.IsVisible === false || gone(c) || c.IsDisposed) return false;
    let p = { X: px, Y: py };
    try { if (c.TransformPointToLocalSpace) p = c.TransformPointToLocalSpace(new (Core as any).SKPoint(px, py)); } catch { /* keep */ }
    const inside = typeof c.HitIsInside === "function" ? c.HitIsInside(p.X, p.Y) : false;
    if (!inside) return false;
    path.push(c);
    const kids = [...childrenOf(c)].map((v, i) => ({ v, i })).sort((a, b) => (a.v.ZIndex ?? 0) - (b.v.ZIndex ?? 0) || a.i - b.i);
    for (let i = kids.length - 1; i >= 0; i--) if (hit(kids[i].v, p.X, p.Y, path)) return true;
    if (layers || drawsSomething(c)) return true;
    path.pop();
    return false;
  };
  for (const root of [...popupRoots(canvas)].reverse()) { const path: Ctl[] = []; if (hit(root, x, y, path)) return path; }
  const path: Ctl[] = [];
  hit(canvas?.Content, x, y, path);
  return path;
}

function drawsSomething(c: Ctl): boolean {
  if (!Array.isArray(c.Views) || inherits(c, "SkiaShape") || inherits(c, "SkiaButton")) return true;
  const bg = c.BackgroundColor;
  return (!!bg && colorHex(bg).slice(1, 3) !== "00") || !!c.FillGradient;
}

/** The control a person points at: a part the library made answers as its owner. */
function pickAt(x: number, y: number): number | undefined {
  const path = hitPath(x, y, false);
  if (!path.length) return undefined;
  const internal = new Map(walk().map((v) => [v.control, v.internal]));
  for (let i = path.length - 1; i >= 0; i--) if (!internal.get(path[i])) return idOf(path[i]);
  return idOf(path[0]);
}

function notAContainer(c: Ctl): string | undefined {
  const name = typeOf(c);
  if (!Array.isArray(c.Views) || typeof c.InsertChild !== "function" || ["Input", "Text", "Media"].includes(groupOfClass(c))) return `${name} does not hold children`;
  if (c.IsTemplated || c.ItemTemplate) return `${name} makes its children from data (ItemsSource / ItemTemplate)`;
  const items = itemsAbove(c);
  if (items) return `${name} is made from ${typeOf(items)}'s template: nothing goes into it`;
  return undefined;
}

/** The layout whose template made the cell that is `c` or holds it: every cell is the same template code, so nothing
 * inside one is changed one by one. */
function itemsAbove(c: Ctl): Ctl | undefined {
  for (let p = c.Parent; p; p = p.Parent) if (p.IsTemplated || p.ItemTemplate) return p;
  return undefined;
}

/** The deepest container under the point and the index after the child under the point. */
function placeTarget(x: number, y: number): { container?: Ctl; index?: number } {
  const path = hitPath(x, y, true);
  for (let i = path.length - 1; i >= 0; i--) {
    if (notAContainer(path[i])) continue;
    const below = path[i + 1];
    const at = below ? appChildren(path[i]).indexOf(below) : -1;
    return { container: path[i], index: at >= 0 ? at + 1 : undefined };
  }
  return {};
}

/** Pick / place mode: pointer input over the canvas goes to the agent, never to the app; the wheel passes; Esc cancels. */
function setPick(mode: { place?: string } | undefined): void {
  const old = state.pick;
  if (old) { state.pick = undefined; old.unhook(); if (old.hover !== undefined) setHighlight(undefined); }
  if (!mode) return;
  const el: HTMLElement | undefined = state.canvas?.Element;
  if (!el || typeof window === "undefined") throw new Error("the canvas has no element");
  const host = el.parentElement ?? el;
  const at = (e: PointerEvent | MouseEvent) => {
    const r = el.getBoundingClientRect(), s = state.canvas.RenderingScale ?? 1;
    return { x: (e.clientX - r.left) * s, y: (e.clientY - r.top) * s };
  };
  let queued = false;
  const onPointer = (e: Event) => {
    if (!(e.target instanceof Node) || !host.contains(e.target)) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    const p = at(e as PointerEvent);
    if (e.type === "pointermove" && !queued) {
      queued = true;
      setTimeout(() => { queued = false; hover(p.x, p.y); }, 0);
    } else if (e.type === "pointerup") setTimeout(() => click(p.x, p.y), 0);
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key !== "Escape") return;
    e.preventDefault(); e.stopImmediatePropagation();
    const place = state.pick?.place;
    setPick(undefined);
    sendMsg(place ? { type: "inserted", request: 0, node: null, parent: null, index: null } : { type: "picked", request: 0, node: null });
  };
  const types = ["pointerdown", "pointermove", "pointerup", "pointercancel", "click", "dblclick", "contextmenu", "mousedown", "mouseup", "touchstart", "touchend"];
  for (const t of types) window.addEventListener(t, onPointer, true);
  window.addEventListener("keydown", onKey, true);
  state.pick = { place: mode.place, unhook: () => { for (const t of types) window.removeEventListener(t, onPointer, true); window.removeEventListener("keydown", onKey, true); } };
}

function hover(x: number, y: number): void {
  const pick = state.pick;
  if (!pick) return;
  try {
    const node = pick.place ? placeTarget(x, y).container : undefined; const nodeId = pick.place ? (node ? idOf(node) : undefined) : pickAt(x, y);
    if (node !== pick.hover) { pick.hover = node; setHighlight(node === undefined ? undefined : find(node)); }
  } catch { /* never into the app */ }
}

function click(x: number, y: number): void {
  const pick = state.pick;
  if (!pick) return;
  try {
    if (pick.place) {
      const type = pick.place;
      const { container, index } = placeTarget(x, y);
      pick.hover = undefined;
      setPick(undefined);
      if (!container) {
        sendError(0, "no container under the pointer");
        sendMsg({ type: "inserted", request: 0, node: null, parent: null, index: null });
        return;
      }
      const r = insertControl(container, index, type, [], [], []);
      setHighlight(find(r.id));
      sendMsg({ type: "inserted", request: 0, node: r.id, parent: container.AccessibilityId, index: r.index });
      enqueue(async () => { await settle(); await sendTree(); });
      return;
    }
    const node = pickAt(x, y);
    pick.hover = undefined; // the picked control keeps its highlight
    setPick(undefined);
    setHighlight(node === undefined ? undefined : find(node));
    sendMsg({ type: "picked", request: 0, node: node ?? null });
  } catch (e) { sendError(0, errorText(e)); }
}

function setHighlight(c: Ctl | undefined): void {
  if (!c && !state.highlight) return;
  state.highlight = c ? new WeakRef(c) : undefined;
  state.canvas?.Update?.(); // frames come on demand: draw the box (or the frame without it) now
}

let paint: any;
function drawHighlight(): void {
  const c = state.highlight?.deref();
  if (!c || c.IsDisposed) return;
  const surface = state.canvas?.Surface;
  const CK = (Core as any).Super.CK;
  if (!surface || !CK) return;
  const r = rectOf(c);
  if (r[2] <= 0 || r[3] <= 0) return;
  if (!paint) { paint = new CK.Paint(); paint.setStyle(CK.PaintStyle.Stroke); paint.setStrokeWidth(2); paint.setAntiAlias(false); }
  let color: Float32Array;
  try { color = (Core as any).Super.ParseColor(/^#?[0-9a-f]{6}([0-9a-f]{2})?$/i.test(state.boxColor) ? (state.boxColor.startsWith("#") ? state.boxColor : "#" + state.boxColor) : "#FFFF00FF"); }
  catch { color = (Core as any).Super.ParseColor("#FFFF00FF"); }
  paint.setColor(color);
  // a 2 px stroke centered 1 px outside the box
  surface.getCanvas().drawRect(CK.LTRBRect(r[0] - 1, r[1] - 1, r[0] + r[2] + 1, r[1] + r[3] + 1), paint);
}

// ---------------------------------------------------------------- snapshot

function snapshot(): { pngBase64: string; width: number; height: number } {
  const bytes: Uint8Array | null = state.canvas?.TakeScreenShot?.();
  if (!bytes) throw new Error("the canvas could not be read");
  let bin = "";
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return { pngBase64: btoa(bin), width: state.canvas.Element?.width ?? 0, height: state.canvas.Element?.height ?? 0 };
}

// ---------------------------------------------------------------- controls panel, insert, remove, copy

/** As DrawnUi.Net's agent: the panel's group from the class's ancestry (a button is a layout too: inputs first). */
function groupOfClass(c: Ctl): string {
  const is = (...n: string[]) => n.some((x) => typeOf(c) === x || inherits(c, x));
  if (is("SkiaButton", "SkiaToggle", "SkiaSwitch", "SkiaCheckbox", "SkiaRadioButton", "SkiaSlider", "SkiaProgress")) return "Input";
  if (is("SkiaLabel", "SkiaEditor", "SkiaRichLabel")) return "Text";
  if (is("SkiaImage", "SkiaSvg", "SkiaLottie", "SkiaGif", "AnimatedFramesRenderer", "SkiaSprite", "SkiaImageTiles")) return "Media";
  if (is("SkiaShape")) return "Shapes";
  if (is("SkiaLayout", "SkiaScroll")) return "Layout";
  return "Other";
}

/** Library classes that are parts of others, bases or app roots: not offered for placing. */
const LIBRARY_PARTS = new Set(["SkiaControl", "SkiaDynamicDrawnCell", "SkiaDrawnCell", "SkiaScrollBar", "RefreshIndicator", "SkiaToggle", "SnappingLayout", "DrawnGame", "AnimatedFramesRenderer", "TextSpan"]);

function classOf(typeName: string): Ctor {
  const ctor = library().get(typeName) ?? state.appClasses.get(typeName) ?? state.options.controls?.[typeName];
  if (!ctor) throw new Error(`no control type ${typeName}`);
  return ctor;
}

function defaultsFor(sample: Ctl, group: string): [string, string][] {
  const empty: [string, string] = ["BackgroundColor", "#22FFFFFF"];
  const wanted: [string, string][] =
    group === "Text" ? [inherits(sample, "SkiaEditor") || typeOf(sample) === "SkiaEditor" ? ["WidthRequest", "200"] : ["Text", "Label"]]
    : group === "Input" ? (typeOf(sample) === "SkiaButton" || inherits(sample, "SkiaButton") ? [["Text", "Button"]] : typeOf(sample) === "SkiaSlider" ? [["WidthRequest", "200"]] : [])
    : group === "Media" ? [["WidthRequest", "120"], ["HeightRequest", "80"], empty]
    : group === "Shapes" ? [["WidthRequest", "80"], ["HeightRequest", "80"], ["BackgroundColor", "#33FFFFFF"]]
    : group === "Layout" ? [["HeightRequest", "80"], ["MinimumWidthRequest", "120"], empty]
    : [["WidthRequest", "80"], ["HeightRequest", "40"], empty];
  const has = new Set(namesOf(sample));
  return wanted.filter(([n]) => has.has(n));
}

function controlKinds(): object[] {
  const kinds: { typeName: string; group: string; own: boolean; defaults: [string, string][]; container: boolean }[] = [];
  const add = (name: string, ctor: Ctor, own: boolean) => {
    if (!own && LIBRARY_PARTS.has(name)) return;
    let sample: Ctl;
    try { sample = defaultOf({ constructor: ctor }); } catch { return; }
    if (!sample) return;
    const group = groupOfClass(sample);
    kinds.push({ typeName: name, group, own, defaults: defaultsFor(sample, group), container: (group === "Layout" || group === "Shapes") && !notAContainer(sample) });
  };
  for (const [name, ctor] of library()) add(name, ctor, false);
  const own = new Map([...state.appClasses, ...Object.entries(state.options.controls ?? {})]);
  for (const [name, ctor] of own) if (!library().has(name)) add(name, ctor, true);
  return kinds.sort((a, b) => Number(a.own) - Number(b.own) || (a.typeName < b.typeName ? -1 : a.typeName > b.typeName ? 1 : 0));
}

/** A parent's children the app (or the designer) added, in order: what insert indexes count. */
function appChildren(parent: Ctl): Ctl[] {
  return childrenOf(parent).filter((v) => !gone(v) && (v.IsChildrenItem || inserted.has(v)));
}

function insertControl(parent: Ctl, index: number | undefined, typeName: string, props: [string, string][], children: any[], skipped: string[]): { id: number; index: number } {
  const why = notAContainer(parent);
  if (why) throw new Error(why);
  // a React-owned control the designer took out comes back (Restore): shown again, not a copy
  const hidden = childrenOf(parent).find((v) => removed.has(v) && typeOf(v) === typeName);
  if (hidden) {
    removed.delete(hidden);
    unpin(hidden, "IsVisible");
    hidden.IsVisible = true;
    for (const [n, v] of props) { try { setLive(hidden, n, v); } catch (e) { skipped.push(`${n} (${errorText(e)})`); } }
    hidden.Update?.();
    changed();
    return { id: hidden.AccessibilityId, index: appChildren(parent).indexOf(hidden) };
  }
  const ctor = classOf(typeName);
  const control = build(ctor, props.length || children.length ? props : defaultsFor(defaultOf({ constructor: ctor }) ?? new ctor(), groupOfClass(defaultOf({ constructor: ctor }))), "", skipped);
  const own = appChildren(parent);
  const at = Math.min(Math.max(0, index ?? own.length), own.length);
  parent.InsertChild(control, own[at]);
  inserted.add(control);
  for (const spec of children) buildChild(control, spec, typeName, skipped);
  parent.Update?.();
  changed();
  return { id: control.AccessibilityId, index: at };
}

function build(ctor: Ctor, props: [string, string][], where: string, skipped: string[]): Ctl {
  const c = new ctor();
  c.ApplyInitialStyles?.(true); // the app's ConfigureStyles defaults, as for a JSX control
  for (const [n, v] of props) {
    try {
      const kind = kindOf(c, n);
      if (!kind || !namesOf(c).includes(n)) throw new Error("no such property");
      c[n] = parse(kind, v);
    } catch (e) { skipped.push(`${where}${n} (${errorText(e)})`); }
  }
  return c;
}

function buildChild(parent: Ctl, spec: any, path: string, skipped: string[]): void {
  const typeName = typeof spec?.typeName === "string" ? spec.typeName : "?";
  const where = `${path} > ${typeName}`;
  let c: Ctl;
  try {
    const props: [string, string][] = Array.isArray(spec.props) ? spec.props.filter((p: any) => typeof p?.name === "string").map((p: any) => [p.name, typeof p.value === "string" ? p.value : ""]) : [];
    c = build(classOf(typeName), props, where + ".", skipped);
    if (notAContainer(parent)) throw new Error(notAContainer(parent));
    parent.InsertChild(c);
    inserted.add(c);
  } catch (e) { skipped.push(`${where} (${errorText(e)})`); return; }
  for (const child of Array.isArray(spec.children) ? spec.children : []) buildChild(c, child, where, skipped);
}

function removeControl(c: Ctl): void {
  const parent = c.Parent;
  if (!parent) throw new Error("the control is the canvas's root");
  const items = itemsAbove(c);
  if (items) throw new Error(`${typeOf(c)} is made from ${typeOf(items)}'s template`);
  if (state.highlight?.deref() === c) setHighlight(undefined);
  const original = standInOf.get(c);
  if (original) {
    // a moved control: its stand-in goes, the hidden original counts as taken out where React has it
    dropStandIn(original, c, false);
    movedAway.delete(original);
    removed.add(original);
  } else if (inserted.has(c)) {
    parent.RemoveSubView(c);
    c.Dispose?.();
  } else {
    // React owns it: hidden until the code drops it (a hot reload then unmounts it)
    removed.add(c);
    pin(c, "IsVisible", "bool", false);
    c.Update?.();
  }
  parent.Update?.();
  changed();
}

/**
 * Moves a control into `parent` at `index` (among its app children, counted without the control; .NET's rules and
 * refusals). A control the designer made moves itself. A React-owned one is never moved behind React's back: it stays,
 * hidden, where React has it, and a stand-in (its copy) shows at the new place under its id; moving that back to the
 * original's place shows the original again. The stand-in goes when React unmounts the original (the code changed)
 * and when the designer leaves. The index it got.
 */
function moveControl(c: Ctl, parent: Ctl, index: number | undefined): number {
  const from = c.Parent, name = typeOf(c);
  if (!from) throw new Error(`${name} is the canvas's root`);
  const items = itemsAbove(c);
  if (items) throw new Error(`${name} is made from ${typeOf(items)}'s template`);
  if (!appChildren(from).includes(c)) throw new Error(`${name} is a part of ${typeOf(from)}, not in the app's code`);
  for (let p = parent; p; p = p.Parent) if (p === c) throw new Error(`${name} cannot go inside itself or its own children`);
  const why = notAContainer(parent);
  if (why) throw new Error(why);
  if (state.highlight?.deref() === c) setHighlight(undefined);
  const own = appChildren(parent).filter((v) => v !== c);
  const at = Math.min(Math.max(0, index ?? own.length), own.length);
  const original = standInOf.get(c);
  if (original && original.Parent === parent && !original.IsDisposed && at === placeOf(original)) {
    dropStandIn(original, c, true); // back home: the original again
    changed();
    return at;
  }
  if (inserted.has(c)) {
    from.RemoveSubView(c);
    parent.InsertChild(c, own[at]);
    from.Update?.();
    parent.Update?.();
    changed();
    return at;
  }
  const spec: any = specOf(c);
  const skipped: string[] = [];
  const standIn = build(classOf(spec.typeName), spec.props.map((p: any) => [p.name, p.value]), "", skipped);
  parent.InsertChild(standIn, own[at]);
  inserted.add(standIn);
  for (const child of spec.children) buildChild(standIn, child, spec.typeName, skipped);
  standInOf.set(standIn, c);
  moved.set(c, standIn);
  movedAway.add(c);
  pin(c, "IsVisible", "bool", false);
  c.Update?.();
  parent.Update?.();
  changed();
  if (skipped.length) sendError(null, `moved ${name} without ${skipped.join("; ")}`);
  return at;
}

/** Where a hidden original stands among its parent's shown app children. */
function placeOf(original: Ctl): number {
  let at = 0;
  for (const v of childrenOf(original.Parent)) {
    if (v === original) return at;
    if (!gone(v) && (v.IsChildrenItem || inserted.has(v))) at++;
  }
  return at;
}

/** The stand-in goes; `show`: its original shows again in its own place. */
function dropStandIn(original: Ctl, standIn: Ctl, show: boolean): void {
  moved.delete(original);
  standInOf.delete(standIn);
  if (state.highlight?.deref() === standIn) setHighlight(undefined);
  const parent = standIn.Parent;
  parent?.RemoveSubView(standIn);
  standIn.Dispose?.();
  parent?.Update?.();
  if (show && !original.IsDisposed) {
    movedAway.delete(original);
    unpin(original, "IsVisible");
    original.IsVisible = true;
    original.Update?.();
  }
}

/** The designer left: every moved control is back in its own place (nothing would clean up a stand-in later). */
function dropStandIns(): void {
  for (const [original, standIn] of [...moved]) dropStandIn(original, standIn, true);
}

function specOf(c: Ctl): object {
  const props = (readProps(c) as any[]).filter((p) => p.isSet && p.isDefault !== true && !String(p.value).startsWith("<")).map((p) => ({ name: p.name, value: p.value, typeName: p.typeName }));
  const children = c.IsTemplated || c.ItemTemplate ? [] : appChildren(c).map(specOf);
  return { typeName: typeOf(c), props, children };
}

// ---------------------------------------------------------------- screens

interface Screens { list: { route: string; title: string | null }[]; open: (route: string) => unknown; current?: () => string | undefined }

/** The app's screens: from the options, else the SkiaShell above the root control (its Routes, Titles and navigation). */
function readScreens(): Screens | undefined {
  const s = state.options.screens;
  if (s) return { list: s.routes.map((r) => ({ route: r, title: s.titles?.[r] ?? null })), open: s.open };
  let routes: Record<string, unknown> | undefined, titles: Record<string, string> | undefined, nav: any;
  for (let f = sourceOf(state.canvas?.Content)?.Fiber, n = 0; f && n < 60; f = f.return, n++) {
    const p = f.memoizedProps;
    if (!nav && p?.value && typeof p.value.GoToAsync === "function") nav = p.value;
    if (!routes && p?.Routes && typeof p.Routes === "object") { routes = p.Routes; titles = p.Titles; }
    if (routes && nav) break;
  }
  if (!routes || !nav) return undefined;
  return {
    list: Object.keys(routes).map((r) => ({ route: r, title: titles?.[r] ?? null })),
    open: (route) => (nav.Route === route ? undefined : nav.GoToAsync(route)),
  };
}

async function openScreen(route: string): Promise<void> {
  const screens = readScreens();
  if (!screens) throw new Error("the app has no SkiaShell the agent can reach (pass `screens` to attach)");
  await Promise.race([Promise.resolve(screens.open(route)), new Promise((r) => setTimeout(r, 3000))]);
  await settle(true);
  await sendTree();
}

/** For tests: the agent's state. */
export const __test = { state, walk, pickAt, hitPath, readProps, setLive, onFrame, sourceMapLookup, setSourceOf: (f: typeof designerSourceOf) => { designerSourceOf = f; } };
