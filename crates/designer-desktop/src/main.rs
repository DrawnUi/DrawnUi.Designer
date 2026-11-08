//! DrawnUI Designer, the desktop window. It starts the hub in this process (or uses the one already
//! running on the port), connects to it as an editor, and shows the connected apps, their screens,
//! the control tree of the selected app and the properties of the selected control. Selecting a
//! control highlights it in the running app; tapping a screen opens it there.
//!
//! `--sidecar` runs only the hub, with no window, for the VS Code extension; it ends when its stdin
//! closes (the extension host that started it is gone). The web build (wasm32-unknown-emscripten,
//! the extension's webview) is the same UI without the hub: the page owns the socket.

// Release builds are a plain Windows app with no console window; debug builds keep the console.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(target_os = "emscripten"))]
mod net;
use designer_i18n as i18n;
mod icons;
#[cfg(not(target_os = "emscripten"))]
mod platform;
#[cfg(not(target_os = "emscripten"))]
mod update;
mod ui;
#[cfg(target_os = "emscripten")]
mod web;

#[cfg(not(target_os = "emscripten"))]
fn main() {
    let port = std::env::var("DRAWNUI_DESIGNER_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(designer_protocol::DEFAULT_PORT);
    if std::env::args().any(|arg| arg == "--sidecar") {
        return sidecar(port);
    }
    if std::env::args().any(|arg| arg == "--mcp") {
        mcp_only(port);
    }
    // Fails when a hub already runs on the port: then the window uses that one.
    let _ = designer_core::spawn(port);
    ui::run(port);
}

#[cfg(target_os = "emscripten")]
fn main() {
    ui::run(0);
}

/// Exit code of `--mcp` when a designer already runs on the port: use that one's MCP.
#[cfg(not(target_os = "emscripten"))]
const ALREADY_RUNNING: i32 = 3;

/// `--mcp`: the hub and its MCP server without a window, for AI agents (Claude Code) when no designer
/// window or VS Code panel runs. Runs until it is stopped. When a designer already runs on the port it
/// does not start: it says where that designer's MCP is and exits with [`ALREADY_RUNNING`], since the
/// window (or panel) serves MCP itself and the agent should use it.
#[cfg(not(target_os = "emscripten"))]
fn mcp_only(port: u16) -> ! {
    let mcp = format!("http://127.0.0.1:{}/mcp", port.wrapping_add(1));
    if !designer_core::spawn(port) {
        eprintln!("drawnui-designer: a DrawnUI Designer already runs on 127.0.0.1:{port}; use its MCP at {mcp}");
        std::process::exit(ALREADY_RUNNING);
    }
    println!("drawnui-designer: MCP at {mcp} (hub on 127.0.0.1:{port}), no window; stop this process to end it");
    loop {
        std::thread::park();
    }
}

/// Only the hub. Lives until stdin reaches its end: the extension host holds the other end of the
/// pipe, so it closes when the host exits or crashes, as well as when the host closes it.
#[cfg(not(target_os = "emscripten"))]
fn sidecar(port: u16) {
    use std::io::Read;
    if designer_core::spawn(port) {
        eprintln!("drawnui-designer: hub on 127.0.0.1:{port}");
    } else {
        eprintln!("drawnui-designer: a hub already runs on 127.0.0.1:{port}");
        return;
    }
    let mut buffer = [0u8; 256];
    let mut stdin = std::io::stdin();
    while matches!(stdin.read(&mut buffer), Ok(n) if n > 0) {}
}
