//! Recent apps and running them again: the hub remembers every app that connected (by project), and
//! starts or restarts one with a [`Launcher`] command, reporting the run's progress until an app of
//! that project connects.

use designer_protocol::Refusal;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::{broadcast, mpsc};

use super::*;

/// How long a run may take until an app of its project connects, by default.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(180);

/// A command that starts an app: the program, its arguments, extra environment and folder.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchCommand {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
}

/// Builds the command that runs a recent app so that it connects to the hub at `hub_url`.
pub trait Launcher: Send + Sync + 'static {
    fn command(&self, app: &RecentApp, hub_url: &str) -> Result<LaunchCommand, Refusal>;
}

/// `dotnet watch run` in the project folder for `dotnet-*` engines, with the agent passed in
/// `DRAWNUI_DESIGNER_AGENT` (the project maps it to its startup hooks) and the hub in
/// `DRAWNUI_DESIGNER_URL`. `cargo run` in the crate's folder for a DrawnUI for Rust app (its project
/// is its Cargo.toml): a debug build, where the agent attaches by itself.
pub struct DefaultLauncher;

impl Launcher for DefaultLauncher {
    fn command(&self, app: &RecentApp, hub_url: &str) -> Result<LaunchCommand, Refusal> {
        if app.engine == "rust" && Path::new(&app.project).file_name().is_some_and(|n| n.eq_ignore_ascii_case("Cargo.toml")) {
            let mut env = vec![];
            if hub_url != format!("ws://127.0.0.1:{DEFAULT_PORT}") {
                env.push(("DRAWNUI_DESIGNER_URL".into(), hub_url.into()));
            }
            return Ok(LaunchCommand { program: "cargo".into(), args: vec!["run".into()], env, cwd: project_dir(&app.project).to_path_buf() });
        }
        if !app.engine.starts_with("dotnet-") {
            return Err(Refusal::new("run-no-way", &[("engine", &app.engine)]));
        }
        let agent = app.agent.clone().ok_or_else(|| Refusal::new("run-agent-unknown", &[]))?;
        // An agent from the ToolBox repo: DrawnUi.Designer.targets sits in a folder above its DLL
        // (agents\dotnet), and the build is pointed at it. Otherwise the app has the agent from its
        // NuGet package (DrawnUi.Designer.Agent), which attaches Debug builds by itself.
        // An agent the app copied into its own folder (agents before AgentSource reported that copy)
        // is no source: the repo this designer was built from gives the agent then, or the app
        // would keep its old copy forever.
        let own_copy = Path::new(&agent).starts_with(project_dir(&app.project));
        let targets = if own_copy { repo_targets() } else { Path::new(&agent).ancestors().map(|d| d.join("DrawnUi.Designer.targets")).find(|t| t.is_file()) };
        let mut env = vec![];
        if targets.is_some() {
            env.push(("DRAWNUI_DESIGNER_AGENT".to_string(), agent));
        }
        if hub_url != format!("ws://127.0.0.1:{DEFAULT_PORT}") {
            env.push(("DRAWNUI_DESIGNER_URL".into(), hub_url.into()));
        }
        let cwd = project_dir(&app.project).to_path_buf();
        let mut args = vec!["watch".into(), "run".into(), "--non-interactive".into()];
        // A project for several platforms (MAUI) runs this computer's one: Windows, or Mac Catalyst.
        if let Some(framework) = host_framework(&cwd) {
            args.extend(["-f".into(), framework.clone()]);
            // On a Mac, dotnet watch's project graph would also load every framework of the referenced
            // libraries (Android, iOS) and fail without their SDKs; only the one that runs here.
            if cfg!(target_os = "macos") {
                args.push(format!("--property:TargetFrameworks={framework}"));
            }
        }
        if let Some(targets) = targets {
            args.push(format!("--property:CustomAfterMicrosoftCommonTargets={}", targets.display()));
            // A build that attaches the agent at build time (Mac Catalyst, phones) takes this DLL:
            // the repo's agent, not the one an app's DrawnUi package brings along.
            let built = targets.with_file_name("Agent").join("bin").join("Release").join("net8.0").join("DrawnUi.Designer.Agent.dll");
            if built.is_file() {
                args.push(format!("--property:DrawnUiDesignerAgentAssembly={}", built.display()));
            }
        }
        Ok(LaunchCommand { program: "dotnet".into(), args, env, cwd })
    }
}

/// DrawnUi.Designer.targets of the ToolBox repo this designer runs from (its exe sits under
/// `target\<profile>`), when there is one.
fn repo_targets() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().map(|d| d.join("agents").join("dotnet").join("DrawnUi.Designer.targets")).find(|t| t.is_file())
}

/// The Windows target framework of the project in `dir` when it targets several frameworks
/// (`<TargetFrameworks>`, as MAUI apps do); `None` for a single-framework project.
fn host_framework(dir: &Path) -> Option<String> {
    let csproj = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "csproj"))?;
    framework_of(&std::fs::read_to_string(csproj).ok()?, HOST_FRAMEWORK)
}

/// The part of a target framework that names this computer's platform (`net10.0-windows10.0...`,
/// `net10.0-maccatalyst`).
const HOST_FRAMEWORK: &str = if cfg!(target_os = "macos") { "-maccatalyst" } else { "-windows" };

fn framework_of(csproj: &str, host: &str) -> Option<String> {
    let mut frameworks = vec![];
    for part in csproj.split("<TargetFrameworks").skip(1) {
        let Some(value) = part.split_once('>').and_then(|(_, rest)| rest.split_once("</TargetFrameworks>")).map(|(v, _)| v) else { continue };
        frameworks.extend(value.split(';').map(str::trim).filter(|f| !f.is_empty() && !f.starts_with("$(")).map(str::to_owned));
    }
    frameworks.into_iter().find(|f| f.contains(host))
}

/// Where the hub keeps recent apps and run logs, and what runs them.
pub struct HubOptions {
    /// `None`: recent apps live in memory only.
    pub recent_file: Option<PathBuf>,
    /// `None`: run output is not logged.
    pub log_dir: Option<PathBuf>,
    pub launcher: Arc<dyn Launcher>,
    /// How long a run may take until an app of its project connects; then its process tree is killed.
    pub run_timeout: Duration,
    /// Where usbmux is reached, for apps on iPhones over the USB cable; `None`: no cable apps.
    pub usbmux: Option<crate::usb::MuxAddr>,
}

impl Default for HubOptions {
    /// `%APPDATA%\DrawnUi.Designer\recent.json` and `...\logs`, [`DefaultLauncher`].
    fn default() -> Self {
        let dir = std::env::var_os("APPDATA").or_else(|| std::env::var_os("HOME")).map(|d| PathBuf::from(d).join("DrawnUi.Designer"));
        Self { recent_file: dir.as_ref().map(|d| d.join("recent.json")), log_dir: dir.map(|d| d.join("logs")), launcher: Arc::new(DefaultLauncher), run_timeout: RUN_TIMEOUT, usbmux: Some(crate::usb::MuxAddr::system()) }
    }
}

/// A run's progress, for whoever waits on it (MCP).
#[derive(Debug, Clone)]
pub(crate) struct RunEvent {
    pub project: String,
    pub state: RunState,
    pub message: String,
    pub app: Option<AppId>,
}

struct Run {
    generation: u64,
    pid: Option<u32>,
    state: RunState,
}

pub(crate) struct Runs {
    options: HubOptions,
    pub hub_url: String,
    active: HashMap<String, Run>,
    pub events: broadcast::Sender<RunEvent>,
    generation: u64,
}

impl Default for Runs {
    fn default() -> Self {
        Self::new(HubOptions { recent_file: None, log_dir: None, launcher: Arc::new(DefaultLauncher), run_timeout: RUN_TIMEOUT, usbmux: None }, String::new())
    }
}

impl Runs {
    pub fn new(options: HubOptions, hub_url: String) -> Self {
        Self { options, hub_url, active: HashMap::new(), events: broadcast::channel(64).0, generation: 0 }
    }

    pub fn run_timeout(&self) -> Duration {
        self.options.run_timeout
    }

    /// The recent apps kept in the recent file.
    pub fn load_recent(&self) -> Vec<RecentApp> {
        self.options.recent_file.as_deref().map(load_recent).unwrap_or_default()
    }
}

pub(crate) use designer_hubcore::project_key;

pub(crate) fn load_recent(path: &Path) -> Vec<RecentApp> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub(crate) fn save_recent(path: &Path, recent: &[RecentApp]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(recent).expect("serializable"))?;
    std::fs::rename(&tmp, path)
}

impl Hub {
    /// An app connected (the core made it the newest recent app): the recent file is saved, and a
    /// run of its project is done.
    pub(crate) fn app_connected(&mut self, id: AppId) {
        let Some(project) = self.core.apps.get(&id).and_then(|a| a.info.project.clone()) else { return };
        if let Some(path) = &self.runs.options.recent_file
            && let Err(e) = save_recent(path, &self.core.recent)
        {
            eprintln!("designer hub: could not save {}: {e}", path.display());
        }
        if let Some(run) = self.runs.active.get_mut(&project_key(&project))
            && run.state != RunState::Running
        {
            run.state = RunState::Running;
            self.run_state(&project, RunState::Running, format!("{} is running", project), None, Some(id));
        }
    }

    /// Tells editors and waiters where a run is.
    fn run_state(&self, project: &str, state: RunState, message: String, refusal: Option<Refusal>, app: Option<AppId>) {
        self.broadcast(&HubToEditor::AppRunState { project: project.into(), state, message: message.clone(), refusal });
        let _ = self.runs.events.send(RunEvent { project: project_key(project), state, message, app });
    }
}

#[cfg(test)]
impl Hub {
    pub(crate) fn runs_pid(&self, project: &str) -> Option<u32> {
        self.runs.active.get(&project_key(project)).and_then(|r| r.pid)
    }

    pub(crate) fn runs_pids(&self) -> Vec<u32> {
        self.runs.active.values().filter_map(|r| r.pid).collect()
    }
}

/// Starts a recent app. Progress goes to editors as `appRunState` and to [`Runs::events`].
pub(crate) fn run_app(hub: &Shared, project: &str) -> Result<(), Refusal> {
    let key = project_key(project);
    if let Some(id) = hub.lock().unwrap().core.apps.iter().find(|(_, a)| a.info.project.as_deref().is_some_and(|p| project_key(p) == key)).map(|(&id, _)| id) {
        return Err(Refusal::new("run-already", &[("project", &project), ("app", &id)]));
    }
    // A watch left in the folder (an app that is not connected) would lock the new build's files.
    for pid in project_processes(project) {
        kill_tree(pid);
    }
    start(hub, project)
}

fn start(hub: &Shared, project: &str) -> Result<(), Refusal> {
    let (recent, command, generation, log, timeout) = {
        let mut h = hub.lock().unwrap();
        let recent = h.core.recent_of(project).cloned().ok_or_else(|| Refusal::new("run-not-recent", &[("project", &project)]))?;
        let command = h.runs.options.launcher.command(&recent, &h.runs.hub_url)?;
        h.runs.generation += 1;
        let generation = h.runs.generation;
        let key = project_key(&recent.project);
        // A run of this project that is still going is replaced.
        if let Some(old) = h.runs.active.remove(&key)
            && let Some(pid) = old.pid
        {
            kill_tree(pid);
        }
        h.runs.active.insert(key, Run { generation, pid: None, state: RunState::Building });
        let log = h.runs.options.log_dir.as_ref().map(|d| d.join(format!("{}.log", file_name(&recent.name))));
        (recent, command, generation, log, h.runs.run_timeout())
    };
    let project = recent.project.clone();
    let mut cmd = tokio::process::Command::new(&command.program);
    cmd.args(&command.args).envs(command.env.iter().cloned()).current_dir(&command.cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let refusal = Refusal::new("run-cannot-start", &[("program", &command.program), ("error", &e)]);
            let mut h = hub.lock().unwrap();
            h.runs.active.remove(&project_key(&project));
            h.run_state(&project, RunState::Failed, refusal.to_string(), Some(refusal.clone()), None);
            return Err(refusal);
        }
    };
    {
        let mut h = hub.lock().unwrap();
        if let Some(run) = h.runs.active.get_mut(&project_key(&project)) {
            run.pid = child.id();
        }
        let shown = log.as_ref().map_or(String::new(), |l| format!(" (output: {})", l.display()));
        h.run_state(&project, RunState::Building, format!("building {}{shown}", recent.name), None, None);
    }

    let (tx, mut lines) = mpsc::unbounded_channel::<String>();
    forward_lines(child.stdout.take(), tx.clone());
    forward_lines(child.stderr.take(), tx);
    let hub = hub.clone();
    tokio::spawn(async move {
        let mut log = log.and_then(|l| {
            let _ = std::fs::create_dir_all(l.parent()?);
            std::fs::File::create(l).ok()
        });
        let mut errors: Vec<String> = vec![];
        let mut tail: Vec<String> = vec![];
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);
        // Lines until both streams end, then the exit code; the deadline cuts it short.
        let exit = loop {
            tokio::select! {
                line = lines.recv() => match line {
                    Some(line) => {
                        if let Some(f) = log.as_mut() {
                            use std::io::Write;
                            let _ = writeln!(f, "{line}");
                        }
                        if is_error_line(&line) && !errors.contains(&line) {
                            errors.push(line.clone());
                        }
                        tail.push(line.clone());
                        if tail.len() > 20 {
                            tail.remove(0);
                        }
                        // dotnet's and cargo's words for a failed build and a built app.
                        let next = if line.contains("Build failed") || line.contains("Build FAILED") || line.starts_with("error: could not compile") {
                            Some((RunState::Failed, failure(Refusal::new("run-build-failed", &[]), &errors, &tail)))
                        } else if line.contains("Build succeeded") || line.contains("Started") || line.trim_start().starts_with("Running `") {
                            Some((RunState::Starting, ("built, starting the app".to_string(), None)))
                        } else {
                            None
                        };
                        if let Some((state, (message, refusal))) = next {
                            update(&hub, &project, generation, state, message, refusal);
                        }
                    }
                    None => break child.wait().await.ok(),
                },
                _ = &mut deadline => {
                    if still(&hub, &project, generation, |s| s != RunState::Running) {
                        if let Some(pid) = child.id() {
                            kill_tree(pid);
                        }
                        let (message, refusal) = failure(Refusal::new("run-no-connect", &[("seconds", &timeout.as_secs())]), &errors, &tail);
                        update(&hub, &project, generation, RunState::Failed, message, refusal);
                        hub.lock().unwrap().runs.active.remove(&project_key(&project));
                    }
                    deadline.as_mut().reset(tokio::time::Instant::now() + Duration::from_secs(60 * 60 * 24 * 365));
                }
            }
        };
        // The process tree ended: a run that never connected failed.
        if still(&hub, &project, generation, |s| s != RunState::Running && s != RunState::Failed) {
            let ended = match exit.and_then(|e| e.code()) {
                Some(code) => Refusal::new("run-ended-code", &[("code", &code)]),
                None => Refusal::new("run-ended", &[]),
            };
            let (message, refusal) = failure(ended, &errors, &tail);
            update(&hub, &project, generation, RunState::Failed, message, refusal);
        }
        let mut h = hub.lock().unwrap();
        if h.runs.active.get(&project_key(&project)).is_some_and(|r| r.generation == generation) {
            h.runs.active.remove(&project_key(&project));
        }
    });
    Ok(())
}

/// Stops a connected app and runs it again. Killed first: the tree the hub started for that
/// project, every `dotnet watch` (or `dotnet run`) tree running the project, and the app itself.
pub(crate) fn restart_app(hub: &Shared, app: AppId) -> Result<String, Refusal> {
    let (project, started, pid) = {
        let mut h = hub.lock().unwrap();
        let info = h.core.apps.get(&app).map(|a| a.info.clone()).ok_or_else(|| Refusal::new("hub-no-app", &[("app", &app)]))?;
        let project = info.project.ok_or_else(|| Refusal::new("hub-no-project", &[]))?;
        let started = h.runs.active.remove(&project_key(&project)).and_then(|r| r.pid);
        (project, started, info.pid)
    };
    for pid in started.into_iter().chain(project_processes(&project)).chain([pid]) {
        kill_tree(pid);
    }
    start(hub, &project)?;
    Ok(project)
}

/// The process trees running a project, by their top-most `dotnet` process: `dotnet watch`
/// started in the project folder, and the ancestors of the app's own executable (built under the
/// folder: app <- `dotnet run` <- dotnet-watch.dll <- `dotnet watch`).
pub(crate) fn project_processes(project: &str) -> Vec<u32> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut sys = System::new();
    let kind = ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always).with_cwd(UpdateKind::Always).with_exe(UpdateKind::Always);
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
    let key = project_key(project);
    let inside = format!("{key}\\");
    let me = Pid::from_u32(std::process::id());
    let is_dotnet = |p: &sysinfo::Process| p.name().to_string_lossy().to_ascii_lowercase().starts_with("dotnet");
    let mut roots: Vec<u32> = vec![];
    for (&pid, p) in sys.processes() {
        // A watch working in the folder, or naming it (`--project <folder or .csproj>`).
        let names_it = || p.cmd().iter().any(|a| project_key(a.to_string_lossy().trim_start_matches("--project=")) == key);
        let watch_here = is_dotnet(p)
            && p.cmd().iter().any(|a| a == "watch")
            && (p.cwd().is_some_and(|d| project_key(&d.to_string_lossy()) == key) || names_it());
        let app_here = p.exe().is_some_and(|e| project_key(&e.to_string_lossy()).starts_with(&inside));
        if pid == me || !(watch_here || app_here) {
            continue;
        }
        let mut top = pid;
        while let Some(parent) = sys.process(top).and_then(|p| p.parent()).filter(|&pp| pp != me && sys.process(pp).is_some_and(is_dotnet)) {
            top = parent;
        }
        if !roots.contains(&top.as_u32()) {
            roots.push(top.as_u32());
        }
    }
    roots
}

fn still(hub: &Shared, project: &str, generation: u64, state: impl Fn(RunState) -> bool) -> bool {
    hub.lock().unwrap().runs.active.get(&project_key(project)).is_some_and(|r| r.generation == generation && state(r.state))
}

/// Moves the current run of a project to `state`; a run that was replaced says nothing.
fn update(hub: &Shared, project: &str, generation: u64, state: RunState, message: String, refusal: Option<Refusal>) {
    let mut h = hub.lock().unwrap();
    let Some(run) = h.runs.active.get_mut(&project_key(project)).filter(|r| r.generation == generation) else { return };
    if run.state == state || run.state == RunState::Running {
        return;
    }
    run.state = state;
    h.run_state(project, state, message, refusal, None);
}

/// What failed (an editor shows it in its language) and the full message with the build's last error
/// lines under it (AI agents and the log read those).
fn failure(what: Refusal, errors: &[String], tail: &[String]) -> (String, Option<Refusal>) {
    let lines = if errors.is_empty() { tail } else { &errors[errors.len().saturating_sub(10)..] };
    let message = if lines.is_empty() { what.to_string() } else { format!("{what}:\n{}", lines.join("\n")) };
    (message, Some(what))
}

/// An MSBuild / compiler error line (`File.cs(3,1): error CS1002: ; expected`), or rustc's
/// (`error[E0425]: cannot find value`, `error: expected one of ...`).
fn is_error_line(line: &str) -> bool {
    line.split(": error ").nth(1).is_some_and(|rest| rest.chars().next().is_some_and(|c| c.is_ascii_uppercase()))
        || (line.starts_with("error[E") || (line.starts_with("error: ") && !line.starts_with("error: could not compile")))
}

fn forward_lines(stream: Option<impl AsyncRead + Unpin + Send + 'static>, tx: mpsc::UnboundedSender<String>) {
    let Some(stream) = stream else { return };
    tokio::spawn(async move {
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
}

fn file_name(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect()
}

/// Ends a process and every process it started.
pub(crate) fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .creation_flags(0x0800_0000)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    // ponytail: the process group the hub made for its own runs; an app started elsewhere ends alone.
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill").args(["-9", &format!("-{pid}")]).status();
        let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).status();
    }
}

#[cfg(test)]
mod framework_tests {
    use super::*;

    /// An agent recorded inside the app's own folder (its old output copy) is no source: the run
    /// takes the repo's targets, so the app gets the current agent.
    #[test]
    fn an_agent_copy_in_the_app_folder_takes_the_repo_agent() {
        let app = RecentApp { name: "HelloWpf".into(), engine: "dotnet-wpf".into(), project: r"C:\Apps\HelloWpf".into(), agent: Some(r"C:\Apps\HelloWpf\bin\Debug\net10.0-windows\DrawnUi.Designer.Agent.dll".into()), last_seen: 0, platform: None };
        let command = DefaultLauncher.command(&app, &format!("ws://127.0.0.1:{DEFAULT_PORT}")).unwrap();
        let targets = command.args.iter().find(|a| a.starts_with("--property:CustomAfterMicrosoftCommonTargets=")).expect("the repo targets");
        assert!(targets.ends_with(r"agents\dotnet\DrawnUi.Designer.targets"), "{targets}");
    }

    /// A DrawnUI for Rust app runs with `cargo run` in its crate's folder; cargo's words are read.
    #[test]
    fn a_rust_app_runs_with_cargo() {
        let app = RecentApp { name: "designer-agent-sample".into(), engine: "rust".into(), project: "/apps/sample/Cargo.toml".into(), agent: None, last_seen: 0, platform: None };
        let command = DefaultLauncher.command(&app, &format!("ws://127.0.0.1:{DEFAULT_PORT}")).unwrap();
        assert_eq!((command.program.as_str(), command.args.as_slice(), command.cwd.as_path()), ("cargo", &["run".to_string()][..], Path::new("/apps/sample")));
        assert!(command.env.is_empty());
        let other = DefaultLauncher.command(&app, "ws://127.0.0.1:54400").unwrap();
        assert_eq!(other.env, [("DRAWNUI_DESIGNER_URL".to_string(), "ws://127.0.0.1:54400".to_string())]);
        assert!(super::is_error_line("error[E0425]: cannot find value `x` in this scope") && !super::is_error_line("   Compiling drawnui v0.1.0"));
    }

    #[test]
    fn a_maui_project_runs_its_windows_framework() {
        let maui = r#"<TargetFrameworks Condition="$([MSBuild]::IsOSPlatform('windows'))">net10.0-windows10.0.19041.0</TargetFrameworks>
        <TargetFrameworks>$(TargetFrameworks);net10.0-android;net10.0-ios;net10.0-maccatalyst</TargetFrameworks>"#;
        assert_eq!(super::framework_of(maui, "-windows").as_deref(), Some("net10.0-windows10.0.19041.0"));
        assert_eq!(super::framework_of("<TargetFramework>net8.0-windows</TargetFramework>", "-windows"), None, "one framework: no -f");
        assert_eq!(super::framework_of("<TargetFrameworks>net10.0-android;net10.0-ios;net10.0-maccatalyst</TargetFrameworks>", "-maccatalyst").as_deref(), Some("net10.0-maccatalyst"));
    }
}
