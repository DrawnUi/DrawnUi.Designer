// DrawnUI Designer for VS Code: "DrawnUI: Open Designer" starts the hub (drawnui-designer --sidecar)
// and opens the designer UI, the same Rust UI as the desktop window built for the web, in a webview.
// The webview talks to the hub over a WebSocket on 127.0.0.1; this file only hosts it, keeps its
// settings in the desktop window's settings file, and says so when the panel closes with changes
// not in code.

import * as vscode from "vscode";
import { spawn, ChildProcess } from "child_process";
import * as fs from "fs";
import * as os from "os";
import * as path from "path";

let log: vscode.LogOutputChannel;
let sidecar: ChildProcess | undefined;
let panel: vscode.WebviewPanel | undefined;
/** Changes not in code yet, as the designer last told. */
let unwritten = 0;

export function activate(context: vscode.ExtensionContext) {
  log = vscode.window.createOutputChannel("DrawnUI Designer", { log: true });
  context.subscriptions.push(
    log,
    vscode.commands.registerCommand("drawnuiDesigner.open", () => open(context)),
    // The designer's own keys (Pick, Restart, Settings, Help, the menus) while its panel is active: taken
    // here so that VS Code does not run its own command for them too (see contributes.keybindings).
    vscode.commands.registerCommand("drawnuiDesigner.designerKey", () => {}),
  );
  // For test runs: open at startup without anyone running the command.
  if (process.env.DRAWNUI_DESIGNER_AUTOOPEN === "1") {
    open(context);
  }
}

export function deactivate() {
  sidecar?.kill();
}

/** The desktop window's settings file: the panel shares it (theme, write mode, language, panels). */
function settingsFile(): string {
  const base = process.env.APPDATA || process.env.HOME || os.homedir();
  return path.join(base, "DrawnUi.Designer", "settings.json");
}

function readSettings(): string | null {
  try {
    return fs.readFileSync(settingsFile(), "utf8");
  } catch {
    return null;
  }
}

function writeSettings(text: string) {
  const file = settingsFile();
  try {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    const temp = `${file}.${process.pid}.tmp`;
    fs.writeFileSync(temp, text);
    fs.renameSync(temp, file);
  } catch (error) {
    log.warn(`settings not saved: ${error}`);
  }
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
  unwritten = 0;
  panel.webview.html = page(panel.webview, media, port, readSettings());
  panel.webview.onDidReceiveMessage((message) => {
    if (typeof message?.log === "string") {
      log.info("[ui] " + message.log);
    } else if (typeof message?.settings === "string") {
      writeSettings(message.settings);
    } else if (typeof message?.changes === "number") {
      unwritten = message.changes;
    }
  });
  panel.onDidDispose(() => {
    panel = undefined;
    // A panel closes without asking: the changes stay live in the app (the hub keeps them while VS
    // Code runs), and reopening the designer writes or undoes them.
    if (unwritten > 0) {
      const count = unwritten;
      unwritten = 0;
      const text = count === 1 ? "1 change is" : `${count} changes are`;
      vscode.window.showWarningMessage(`DrawnUI Designer closed: ${text} not in code yet. They stay live in the app until it restarts.`, "Reopen").then((choice) => {
        if (choice === "Reopen") {
          open(context);
        }
      });
    }
  });
}

function page(webview: vscode.Webview, media: vscode.Uri, port: number, settings: string | null): string {
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
      settings: ${JSON.stringify(settings).replace(/</g, "\\u003c")},
      saveSettings: (text) => vscode.postMessage({ settings: text }),
      onChanges: (count) => vscode.postMessage({ changes: count }),
    }).catch((e) => log("[error] start: " + e));
  </script>
</body>
</html>`;
}
