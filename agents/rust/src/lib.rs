//! DrawnUI Designer agent for DrawnUI for Rust apps.
//!
//! ```ignore
//! drawnui::run("My app", || drawnui_designer_agent::attach!(Ui::new(App::default(), build)));
//! ```
//!
//! In a debug build the app connects to the designer hub (`DRAWNUI_DESIGNER_URL`, default
//! `ws://127.0.0.1:54270`) from a background thread, every 2 s while no hub is there, and answers
//! the designer: its control tree, properties, live edits, a highlight box, the element picker and
//! snapshots. Nothing runs on the UI thread until a designer asks. `DRAWNUI_DESIGNER=0` turns it
//! off. In a release build `attach` returns the app as it is.
//!
//! In the browser (wasm32-unknown-emscripten, any build) there is no socket: the page carries the
//! messages (`dui_designer_attach` / `dui_designer_receive`, see `page.rs` and docs/designer.md
//! "Rust apps in a page"). Until a page attaches, a frame costs one call that compares two numbers.

use drawnui::{App, Ui};

#[cfg(any(debug_assertions, target_os = "emscripten"))]
mod agent;
#[cfg(any(debug_assertions, target_os = "emscripten"))]
mod controls;
#[cfg(all(debug_assertions, not(target_os = "emscripten")))]
mod link;
#[cfg(target_os = "emscripten")]
pub mod page;

/// `attach(ui, env!("CARGO_MANIFEST_DIR"))`: the app's own crate folder is the project the designer shows.
#[macro_export]
macro_rules! attach {
    ($ui:expr) => {
        $crate::attach($ui, env!("CARGO_MANIFEST_DIR"))
    };
}

/// The app with the designer agent around it, for `drawnui::run`. `project_dir`: the app crate's
/// folder (its `Cargo.toml` is reported as the project; not in the browser). Desktop release
/// builds: the app as it is.
pub fn attach<S: 'static>(ui: Ui<S>, project_dir: &str) -> Box<dyn App> {
    #[cfg(target_os = "emscripten")]
    {
        let _ = project_dir;
        Box::new(in_page(ui))
    }
    #[cfg(not(target_os = "emscripten"))]
    {
        #[cfg(debug_assertions)]
        if std::env::var_os("DRAWNUI_DESIGNER").is_none_or(|v| v != "0") {
            return Box::new(agent::Agent::start(ui, project_dir));
        }
        let _ = project_dir;
        Box::new(ui)
    }
}

/// Browser: the app with the agent around it, as a concrete `App` (for `drawnui::hot_reload!`, whose
/// maker returns the app by value: `hot_reload!(App, |s| drawnui_designer_agent::in_page(make(s)))`).
/// The state of the page link lives in the engine's module, so a hot-swapped app stays attached.
#[cfg(target_os = "emscripten")]
pub fn in_page<S: 'static>(ui: Ui<S>) -> impl App {
    agent::Agent::in_page(ui)
}
