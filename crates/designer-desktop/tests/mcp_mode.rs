//! `--mcp`: a designer without a window serves the hub and MCP; a second one refuses with exit code 3
//! because a designer already runs (the agent then uses that one's MCP).

use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn a_second_mcp_designer_refuses_and_names_the_running_one() {
    // A free port pair, away from a designer someone may be running on 54270.
    let port = 47_311u16;
    let exe = env!("CARGO_BIN_EXE_drawnui-designer");
    let mut first = Command::new(exe).arg("--mcp").env("DRAWNUI_DESIGNER_PORT", port.to_string()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let up = Instant::now();
    while TcpStream::connect(("127.0.0.1", port + 1)).is_err() {
        assert!(up.elapsed() < Duration::from_secs(20), "the MCP port never opened");
        std::thread::sleep(Duration::from_millis(100));
    }
    let second = Command::new(exe).arg("--mcp").env("DRAWNUI_DESIGNER_PORT", port.to_string()).output().unwrap();
    let _ = first.kill();
    let _ = first.wait();
    assert_eq!(second.status.code(), Some(3), "{second:?}");
    let said = String::from_utf8_lossy(&second.stderr);
    assert!(said.contains(&format!("http://127.0.0.1:{}/mcp", port + 1)), "{said}");
}
