// DrawnUI Designer for VS Code: "DrawnUI: Open Designer" starts the hub (drawnui-designer --sidecar)
// and opens the designer UI, the same Rust UI as the desktop window built for the web, in a webview.
// The webview talks to the hub over a WebSocket on 127.0.0.1; this file only hosts it.

import * as vscode from "vscode";
import { spawn, ChildProcess } from "child_process";
import * as fs from "fs";
import * as path from "path";

let log: vscode.LogOutputChannel;
let sidecar: ChildProcess | undefined;
let panel: vscode.WebviewPanel | undefined;

export function activate(context: vscode.ExtensionContext) {
  log = vscode.window.createOutputChannel("DrawnUI Designer", { log: true });
  context.subscriptions.push(log, vscode.commands.registerCommand("drawnuiDesigner.open", () => open(context)));
  // For test runs: open at startup without anyone running the command.
  if (process.env.DRAWNUI_DESIGNER_AUTOOPEN === "1") {
    open(context);
  }
}

export function deactivate() {
  sidecar?.kill();
}

function hubPort(): number {
  const fromEnv = Number(process.env.DRAWNUI_DESIGNER_PORT);
  return fromEnv > 0 ? fromEnv : vscode.workspace.getConfiguration("drawnuiDesigner").get<number>("port", 54270);
}

/** The sidecar: an explicit path, the one bundled in bin/, else the workspace build next to this folder. */
function sidecarPath(context: vscode.ExtensionContext): string | undefined {
  const exe = process.platform === "win32" ? "drawnui-designer.exe" : "drawnui-designer";
  const candidates = [
    process.env.DRAWNUI_DESIGNER_SIDECAR,
    vscode.workspace.getConfiguration("drawnuiDesigner").get<string>("sidecarPath"),
    context.asAbsolutePath(path.join("bin", exe)),
    context.asAbsolutePath(path.join("..", "target", "release", exe)),
    context.asAbsolutePath(path.join("..", "target", "debug", exe)),
  ];
  return candidates.find((candidate) => candidate && fs.existsSync(candidate));
}

function startSidecar(context: vscode.ExtensionContext, port: number) {
  if (sidecar && sidecar.exitCode === null) {
    return;
  }
  const exe = sidecarPath(context);
  if (!exe) {
    log.warn(`drawnui-designer not found: the designer uses a hub already running on port ${port}`);
    return;
  }
  log.info(`starting the hub: ${exe} --sidecar (port ${port})`);
  // stdin stays a pipe: the sidecar exits when it closes, also when this host dies.
  const child = spawn(exe, ["--sidecar"], {
    env: { ...process.env, DRAWNUI_DESIGNER_PORT: String(port) },
    stdio: ["pipe", "ignore", "pipe"],
    windowsHide: true,
  });
  child.stderr?.on("data", (data: Buffer) => log.info("[hub] " + data.toString().trim()));
  child.on("error", (error) => log.error(`hub failed to start: ${error.message}`));
  child.on("exit", (code) => log.info(`hub exited (${code})`));
  sidecar = child;
}

function open(context: vscode.ExtensionContext) {
  const port = hubPort();
  startSidecar(context, port);
  if (panel) {
    panel.reveal();
    return;
  }
  const media = vscode.Uri.joinPath(context.extensionUri, "media");
  panel = vscode.window.createWebviewPanel("drawnuiDesigner", "DrawnUI Designer", vscode.ViewColumn.Beside, {
    enableScripts: true,
    retainContextWhenHidden: true, // the wasm app keeps its state while the tab is in the background
    localResourceRoots: [media],
  });
  panel.webview.html = page(panel.webview, media, port);
  panel.webview.onDidReceiveMessage((message) => {
    if (typeof message?.log === "string") {
      log.info("[ui] " + message.log);
    }
  });
  panel.onDidDispose(() => (panel = undefined));
}

function page(webview: vscode.Webview, media: vscode.Uri, port: number): string {
  const uri = (file: string) => webview.asWebviewUri(vscode.Uri.joinPath(media, file)).toString();
  const nonce = Array.from({ length: 32 }, () => Math.floor(Math.random() * 36).toString(36)).join("");
  const source = webview.cspSource;
  const hub = `ws://127.0.0.1:${port}`;
  const csp = [
    "default-src 'none'",
    `script-src ${source} 'nonce-${nonce}' 'wasm-unsafe-eval'`,
    `connect-src ${hub} ${source}`,
    `img-src ${source} data: blob:`,
    `font-src ${source}`,
    `worker-src ${source} blob:`,
    "style-src 'unsafe-inline'",
  ].join("; ");
  return `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta http-equiv="Content-Security-Policy" content="${csp}" />
  <style>
    html, body { margin: 0; height: 100%; overflow: hidden; background: #16181d; }
    canvas { width: 100vw; height: 100vh; display: block; touch-action: none; }
  </style>
</head>
<body>
  <canvas id="canvas"></canvas>
  <script nonce="${nonce}" src="${uri("drawnui-designer.js")}"></script>
  <script nonce="${nonce}" src="${uri("drawnui_host.js")}"></script>
  <script nonce="${nonce}" src="${uri("designer.js")}"></script>
  <script nonce="${nonce}">
    const vscode = acquireVsCodeApi();
    const log = (text) => vscode.postMessage({ log: String(text) });
    window.addEventListener("error", (e) => log("[error] " + e.message));
    window.addEventListener("unhandledrejection", (e) => log("[error] " + e.reason));
    DrawnUiDesigner.start({
      canvas: document.getElementById("canvas"),
      url: "${hub}",
      assetBase: "${webview.asWebviewUri(media)}/",
      log,
    }).catch((e) => log("[error] start: " + e));
  </script>
</body>
</html>`;
}
