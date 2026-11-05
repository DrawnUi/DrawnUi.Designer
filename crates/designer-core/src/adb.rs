//! Apps on Android phones over the USB cable: for every phone adb reports, the hub runs
//! `adb reverse tcp:54270 tcp:54270`, so the app's agent on the phone reaches the hub at
//! 127.0.0.1:54270 as on the computer, and nothing listens inside the app.
//!
//! Only the hub on the default port does this: a hub on another port (a test) would take the
//! phone's port away from the designer the developer uses.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;

/// adb from PATH, else the Android SDK (ANDROID_HOME, ANDROID_SDK_ROOT, the default install folder).
fn adb() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "adb.exe" } else { "adb" };
    let on_path = std::env::var_os("PATH").into_iter().flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).map(|d| d.join(exe));
    let sdk = ["ANDROID_HOME", "ANDROID_SDK_ROOT"].into_iter().filter_map(std::env::var_os).map(PathBuf::from);
    let default_sdk = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Android").join("Sdk"))
    } else {
        std::env::var_os("HOME").map(|d| PathBuf::from(d).join(if cfg!(target_os = "macos") { "Library/Android/sdk" } else { "Android/Sdk" }))
    };
    on_path.chain(sdk.chain(default_sdk).map(|d| d.join("platform-tools").join(exe))).find(|p| p.is_file())
}

/// An adb command that opens no console window on Windows.
fn command(adb: &std::path::Path) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(adb);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    cmd
}

/// The serials in state "device" in one `adb track-devices` report ("serial\tstate\n" lines).
fn ready(report: &str) -> Vec<String> {
    report.lines().filter_map(|l| l.split_once('\t')).filter(|(_, state)| state.trim() == "device").map(|(serial, _)| serial.to_owned()).collect()
}

/// Follows adb's device list (events, no polling) and gives every phone that becomes ready the
/// reverse port. Without adb it does nothing; when adb's tracking ends it starts again after 10 s.
pub(crate) async fn watch(hub: crate::Shared, port: u16) {
    let Some(adb) = adb() else { return };
    loop {
        if let Ok(mut child) = command(&adb).arg("track-devices").stdout(Stdio::piped()).kill_on_drop(true).spawn() {
            let mut out = child.stdout.take().expect("piped");
            let mut known = Vec::<String>::new();
            loop {
                // Each report: its length as 4 hex digits, then the list.
                let mut len = [0u8; 4];
                if out.read_exact(&mut len).await.is_err() {
                    break;
                }
                let Some(n) = std::str::from_utf8(&len).ok().and_then(|h| usize::from_str_radix(h, 16).ok()) else { break };
                let mut body = vec![0u8; n];
                if out.read_exact(&mut body).await.is_err() {
                    break;
                }
                let now = ready(&String::from_utf8_lossy(&body));
                for serial in now.iter().filter(|s| !known.contains(s)) {
                    let _ = command(&adb).args(["-s", serial, "reverse", &format!("tcp:{port}"), &format!("tcp:{port}")]).status().await;
                }
                crate::set_cables(&hub, Some(now.len() as u32), None);
                known = now;
            }
            let _ = child.kill().await;
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_the_ready_phones() {
        assert_eq!(super::ready("R5CX12ABCDE\tdevice\nemulator-5554\toffline\nX\tunauthorized\n"), ["R5CX12ABCDE"]);
        assert!(super::ready("").is_empty());
    }
}
