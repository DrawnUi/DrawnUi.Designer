//! Asks GitHub for the newest release at start and once a day (Settings, About, Check for updates);
//! the title bar offers it when it is newer than this build. Nothing is sent but the request itself.
//! The same flow as Agent Signals: "Update and restart" downloads this system's file, puts it in
//! place of the running designer and starts it once this process has ended.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Every release, newest first. Designer releases are previews, which `releases/latest` leaves out.
const RELEASES: &str = "https://api.github.com/repos/DrawnUi/DrawnUi.Designer/releases?per_page=10";
const EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// The setting, read by the checking thread once a minute.
pub static CHECK: AtomicBool = AtomicBool::new(true);

type Result<T> = std::result::Result<T, String>;

/// Where "Update and restart" is: nothing asked yet, downloading, or failed with a reason (the
/// menu then offers the download in the browser).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Install {
    #[default]
    Idle,
    Downloading,
    Failed(String),
}

/// A release newer than this build: its version, the download for this system (else its release
/// page) and the release page.
#[derive(Clone, Debug, PartialEq)]
pub struct Update {
    pub version: String,
    pub url: String,
    pub page: String,
}

/// How this build's download on a release is named at the end.
#[cfg(windows)]
const ASSET: &str = if cfg!(target_arch = "aarch64") { "-windows-arm64.exe" } else { "-windows-x64.exe" };
#[cfg(target_os = "macos")]
const ASSET: &str = "-macos-universal.zip";
#[cfg(not(any(windows, target_os = "macos")))]
const ASSET: &str = "\0";

/// Checks on a thread of its own: a slow or missing network never holds up the window. `found`
/// gets each newer release once; no answer, nothing shows and the next check is a day later.
pub fn start(found: impl Fn(Update) + Send + 'static) {
    std::thread::spawn(move || {
        // What an update left behind (the replaced program), once the old instance is gone.
        std::thread::sleep(Duration::from_secs(10));
        clean_after_update();
        let mut last: Option<std::time::Instant> = None;
        let mut told: Option<Update> = None;
        loop {
            if !CHECK.load(Ordering::Relaxed) {
                last = None;
            } else if last.is_none_or(|t| t.elapsed() >= EVERY) {
                last = Some(std::time::Instant::now());
                let update = http_get(RELEASES).ok().and_then(|json| newer(&json, env!("CARGO_PKG_VERSION"), ASSET));
                if let Some(update) = update.filter(|u| told.as_ref() != Some(u)) {
                    told = Some(update.clone());
                    found(update);
                }
            }
            std::thread::sleep(Duration::from_secs(60));
        }
    });
}

/// "Update and restart": downloads this system's file, puts it in place of the running designer
/// and starts it once this process has ended (this process ends here). `failed` gets why not.
pub fn install(update: Update, failed: impl Fn(String) + Send + 'static) {
    std::thread::spawn(move || {
        let name = update.url.rsplit('/').next().unwrap_or("designer-update");
        let file = std::env::temp_dir().join(format!("{}-{name}", std::process::id()));
        let done = if update.url.ends_with(ASSET) { download(&update.url, &file).and_then(|()| install_update(&file)) } else { Err("no download for this system".into()) };
        match done {
            Ok(()) => std::process::exit(0),
            Err(why) => failed(why),
        }
    });
}

/// The newest release in GitHub's answer when it is newer than `current`, with the download whose
/// name ends in `asset`, else its release page. Drafts are skipped.
fn newer(json: &str, current: &str, asset: &str) -> Option<Update> {
    let releases: serde_json::Value = serde_json::from_str(json).ok()?;
    let current = version(current)?;
    let release = releases
        .as_array()?
        .iter()
        .filter(|r| r.get("draft").and_then(|d| d.as_bool()) != Some(true))
        .filter_map(|r| Some((version(r.get("tag_name")?.as_str()?)?, r)))
        .filter(|(v, _)| *v > current)
        .max_by_key(|(v, _)| *v)?
        .1;
    let page = release.get("html_url")?.as_str()?.to_string();
    let download = release.get("assets").and_then(|a| a.as_array()).and_then(|assets| {
        assets
            .iter()
            .find(|a| a.get("name").and_then(|n| n.as_str()).is_some_and(|n| n.ends_with(asset)))
            .and_then(|a| a.get("browser_download_url")?.as_str())
    });
    let tag = release.get("tag_name")?.as_str()?;
    Some(Update { version: tag.trim_start_matches('v').to_string(), url: download.map_or_else(|| page.clone(), str::to_string), page })
}

/// "v0.6.5" or "0.6.5" to (0, 6, 5).
fn version(text: &str) -> Option<(u32, u32, u32)> {
    let mut parts = text.trim().trim_start_matches('v').split(['.', '-']).map(|p| p.parse::<u32>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

/// The system's curl (Windows 10 and 11 ship curl.exe), without a console window.
fn curl() -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = std::process::Command::new("curl.exe");
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        command
    }
    #[cfg(not(windows))]
    std::process::Command::new("/usr/bin/curl")
}

/// The body of a GET.
fn http_get(url: &str) -> Result<String> {
    let out = curl().args(["-sfL", "--max-time", "20", "-H", "User-Agent: DrawnUI-Designer", url]).output().map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("curl: {}", out.status));
    }
    String::from_utf8(out.stdout).map_err(|e| e.to_string())
}

/// Saves a download.
fn download(url: &str, file: &Path) -> Result<()> {
    let status = curl().args(["-sfL", "--max-time", "300", "-H", "User-Agent: DrawnUI-Designer", "-o"]).arg(file).arg(url).status().map_err(|e| format!("curl: {e}"))?;
    if status.success() { Ok(()) } else { Err("the download did not finish".into()) }
}

/// Windows: the running exe is renamed (Windows allows that), the new one takes its name, and a
/// hidden PowerShell starts it once this process is gone. The exe's folder must be writable.
#[cfg(windows)]
fn install_update(file: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let old = replaced(&exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("{} cannot be replaced: {e}", exe.display()))?;
    if let Err(e) = std::fs::copy(file, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("{} cannot be written: {e}", exe.display()));
    }
    let _ = std::fs::remove_file(file);
    let script = format!("Wait-Process -Id {} -Timeout 30 -ErrorAction SilentlyContinue; Start-Process -FilePath '{}'", std::process::id(), exe.display().to_string().replace('\'', "''"));
    std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &script])
        .creation_flags(0x0800_0000)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("the new version cannot be started: {e}"))
}

/// The exe an update replaced, next to the new one.
#[cfg(windows)]
fn replaced(exe: &Path) -> PathBuf {
    exe.with_extension("exe.old")
}

#[cfg(windows)]
fn clean_after_update() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(replaced(&exe));
    }
}

/// macOS: unzips the release into a hidden folder next to the running app bundle (the same disk,
/// so both moves are renames), moves the running bundle aside into it and the new one into its
/// place, and a shell opens it once this process is gone. Only for the app bundle.
#[cfg(target_os = "macos")]
fn install_update(zip: &Path) -> Result<()> {
    use std::process::Command;
    let bundle = running_bundle().ok_or("not running from the app")?;
    let work = update_folder(&bundle);
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| format!("{} cannot be written: {e}", work.display()))?;
    let unpacked = work.join("new");
    let ok = Command::new("/usr/bin/ditto").args(["-x", "-k"]).arg(zip).arg(&unpacked).status().is_ok_and(|s| s.success());
    let _ = std::fs::remove_file(zip);
    let new = std::fs::read_dir(&unpacked)
        .ok()
        .and_then(|d| d.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e == "app")))
        .filter(|_| ok)
        .ok_or("the download could not be unpacked")?;
    let old = work.join("replaced.app");
    std::fs::rename(&bundle, &old).map_err(|e| format!("{} cannot be replaced: {e}", bundle.display()))?;
    if let Err(e) = std::fs::rename(&new, &bundle) {
        let _ = std::fs::rename(&old, &bundle);
        return Err(format!("{} cannot be written: {e}", bundle.display()));
    }
    let script = format!("while kill -0 {} 2>/dev/null; do sleep 0.2; done; open -n \"$0\"", std::process::id());
    Command::new("/bin/sh").args(["-c", &script]).arg(&bundle).spawn().map(|_| ()).map_err(|e| format!("the new version cannot be started: {e}"))
}

/// The .app this process runs from (`X.app/Contents/MacOS/exe`).
#[cfg(target_os = "macos")]
fn running_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().nth(3).filter(|b| b.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf)
}

/// The hidden folder an update works in, next to the app bundle.
#[cfg(target_os = "macos")]
fn update_folder(bundle: &Path) -> PathBuf {
    bundle.with_file_name(".drawnui-designer-update")
}

/// The replaced bundle and the unpacked download, gone at the next start.
#[cfg(target_os = "macos")]
fn clean_after_update() {
    if let Some(bundle) = running_bundle() {
        let _ = std::fs::remove_dir_all(update_folder(&bundle));
    }
}

/// Elsewhere (Linux): the release page opens in the browser instead.
#[cfg(not(any(windows, target_os = "macos")))]
fn install_update(_file: &Path) -> Result<()> {
    Err("updates install by hand here".into())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn clean_after_update() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_newest_release_and_its_download() {
        let json = r#"[
            {"tag_name":"v0.7.0","draft":true,"html_url":"https://gh/v0.7.0","assets":[]},
            {"tag_name":"v0.6.0","draft":false,"html_url":"https://gh/v0.6.0","assets":[
                {"name":"drawnui-designer-0.6.0-windows-arm64.exe","browser_download_url":"https://dl/arm64.exe"},
                {"name":"drawnui-designer-0.6.0-windows-x64.exe","browser_download_url":"https://dl/x64.exe"}]},
            {"tag_name":"v0.5.0","draft":false,"html_url":"https://gh/v0.5.0","assets":[]}]"#;
        // The draft is skipped; the newest real release wins.
        assert_eq!(newer(json, "0.5.0", "-windows-x64.exe"), Some(Update { version: "0.6.0".into(), url: "https://dl/x64.exe".into(), page: "https://gh/v0.6.0".into() }));
        // No download for this system: the release page.
        assert_eq!(newer(json, "0.5.0", "-macos-universal.zip").unwrap().url, "https://gh/v0.6.0");
        // The same or a newer build: nothing.
        assert_eq!(newer(json, "0.6.0", "-windows-x64.exe"), None);
        assert_eq!(newer(json, "0.10.0", "-windows-x64.exe"), None);
        // Not GitHub's answer (rate limit, error page): nothing.
        assert_eq!(newer(r#"{"message":"API rate limit exceeded"}"#, "0.5.0", "x"), None);
        assert_eq!(newer("<html>", "0.5.0", "x"), None);
    }

    /// Asks GitHub for real: `cargo test -p designer-desktop -- --ignored github_answers`.
    #[test]
    #[ignore]
    fn github_answers() {
        let json = http_get(RELEASES).unwrap();
        let update = newer(&json, "0.0.1", ASSET).unwrap();
        assert!(update.page.contains("DrawnUi.Designer"), "{update:?}");
    }
}
