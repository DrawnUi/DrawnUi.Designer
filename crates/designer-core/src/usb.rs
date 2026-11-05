//! Apps on iPhones over the USB cable, the way the .NET debugger reaches them: the app's agent waits
//! on a port on the phone ([`APP_PORTS`]), and the hub connects to it through Apple's usbmux service
//! (`/var/run/usbmuxd` on macOS and Linux, TCP 27015 with Apple's device service on Windows). Once
//! connected, the stream is an ordinary app connection.
//!
//! usbmux speaks a 16-byte header plus an XML property list. Only three messages are used: Listen
//! (device attached / detached events), Connect (a device port) and its Result.

use crate::Shared;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The ports an app's agent waits on, on the phone: the first free one of these.
pub const APP_PORTS: std::ops::RangeInclusive<u16> = 54273..=54282;

/// Where usbmux is reached.
#[derive(Clone, Debug)]
pub enum MuxAddr {
    #[cfg(unix)]
    Unix(std::path::PathBuf),
    Tcp(std::net::SocketAddr),
}

impl MuxAddr {
    /// The system's usbmux service.
    pub fn system() -> MuxAddr {
        #[cfg(unix)]
        return MuxAddr::Unix("/var/run/usbmuxd".into());
        #[cfg(not(unix))]
        MuxAddr::Tcp(([127, 0, 0, 1], 27015).into())
    }
}

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
type Stream = Box<dyn Io>;

async fn open(addr: &MuxAddr) -> std::io::Result<Stream> {
    Ok(match addr {
        #[cfg(unix)]
        MuxAddr::Unix(path) => Box::new(tokio::net::UnixStream::connect(path).await?),
        MuxAddr::Tcp(at) => Box::new(tokio::net::TcpStream::connect(at).await?),
    })
}

fn plist(body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>ClientVersionString</key><string>drawnui-designer</string><key>ProgName</key><string>drawnui-designer</string>{body}</dict></plist>\n"
    )
}

async fn send(stream: &mut Stream, body: &str) -> std::io::Result<()> {
    let payload = plist(body);
    let mut frame = Vec::with_capacity(16 + payload.len());
    for word in [16 + payload.len() as u32, 1, 8, 1] {
        frame.extend_from_slice(&word.to_le_bytes());
    }
    frame.extend_from_slice(payload.as_bytes());
    stream.write_all(&frame).await
}

async fn receive(stream: &mut Stream) -> std::io::Result<String> {
    let mut header = [0u8; 16];
    stream.read_exact(&mut header).await?;
    let length = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
    if !(16..=1 << 20).contains(&length) {
        return Err(std::io::Error::other("usbmux: bad length"));
    }
    let mut body = vec![0u8; length - 16];
    stream.read_exact(&mut body).await?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// The value after `<key>name</key>` in a usbmux property list (the first one), without its tag.
// ponytail: a scan for the few flat keys usbmux sends, not a plist parser; take the plist crate if more is needed.
fn value<'a>(plist: &'a str, name: &str) -> Option<&'a str> {
    let after = &plist[plist.find(&format!("<key>{name}</key>"))? + name.len() + 11..];
    let open = after.find('<')?;
    let tag_end = open + after[open..].find('>')? + 1;
    let close = tag_end + after[tag_end..].find("</")?;
    Some(after[tag_end..close].trim())
}

/// Connects to `port` on device `device`; the stream then is a plain pipe to that port.
async fn connect(addr: &MuxAddr, device: u64, port: u16) -> std::io::Result<Stream> {
    let mut stream = open(addr).await?;
    // usbmux wants the port in network byte order inside the integer.
    let body = format!("<key>MessageType</key><string>Connect</string><key>DeviceID</key><integer>{device}</integer><key>PortNumber</key><integer>{}</integer>", port.swap_bytes());
    send(&mut stream, &body).await?;
    let answer = receive(&mut stream).await?;
    match value(&answer, "Number") {
        Some("0") => Ok(stream),
        n => Err(std::io::Error::other(format!("usbmux refused the port: {n:?}"))),
    }
}

/// Watches usbmux for iPhones on the cable and connects to every app waiting on one, until the
/// hub ends. Without usbmux (no Apple device service) it looks again every 10 s.
pub(crate) async fn watch(hub: Shared, addr: MuxAddr) {
    loop {
        if let Ok(mut events) = open(&addr).await {
            let mut devices = std::collections::HashMap::<u64, tokio::task::JoinHandle<()>>::new();
            if send(&mut events, "<key>MessageType</key><string>Listen</string>").await.is_ok() {
                while let Ok(msg) = receive(&mut events).await {
                    let Some(id) = value(&msg, "DeviceID").and_then(|d| d.parse::<u64>().ok()) else { continue };
                    match value(&msg, "MessageType") {
                        // Wi-Fi paired phones are listed too: only the cable.
                        Some("Attached") if value(&msg, "ConnectionType") == Some("USB") => {
                            devices.entry(id).or_insert_with(|| tokio::spawn(device(hub.clone(), addr.clone(), id)));
                        }
                        Some("Detached") => {
                            if let Some(task) = devices.remove(&id) {
                                task.abort();
                            }
                        }
                        _ => {}
                    }
                    crate::set_cables(&hub, None, Some(devices.len() as u32));
                }
            }
            devices.into_values().for_each(|task| task.abort());
            crate::set_cables(&hub, None, Some(0));
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

/// While a phone is on the cable: tries its app ports every 2 s and serves each app found there.
async fn device(hub: Shared, addr: MuxAddr, id: u64) {
    let busy = Arc::new(Mutex::new(HashSet::<u16>::new()));
    loop {
        for port in APP_PORTS {
            if busy.lock().unwrap().contains(&port) {
                continue;
            }
            let Ok(stream) = connect(&addr, id, port).await else { continue };
            let Ok((ws, _)) = tokio_tungstenite::client_async(format!("ws://localhost:{port}/"), stream).await else { continue };
            busy.lock().unwrap().insert(port);
            let (hub, busy) = (hub.clone(), busy.clone());
            tokio::spawn(async move {
                crate::session(hub, ws, true).await;
                busy.lock().unwrap().remove(&port);
            });
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_usbmux_values() {
        let msg = "<plist><dict><key>DeviceID</key>\n\t<integer>7</integer><key>MessageType</key><string>Attached</string><key>Properties</key><dict><key>ConnectionType</key><string>USB</string></dict></dict></plist>";
        assert_eq!(value(msg, "DeviceID"), Some("7"));
        assert_eq!(value(msg, "MessageType"), Some("Attached"));
        assert_eq!(value(msg, "ConnectionType"), Some("USB"));
        assert_eq!(value(msg, "Number"), None);
    }

    async fn reply(stream: &mut tokio::net::TcpStream, body: &str) {
        let payload = plist(body);
        let mut frame = vec![];
        for word in [16 + payload.len() as u32, 1, 8, 1] {
            frame.extend_from_slice(&word.to_le_bytes());
        }
        frame.extend_from_slice(payload.as_bytes());
        stream.write_all(&frame).await.unwrap();
    }

    async fn request(stream: &mut tokio::net::TcpStream) -> String {
        let mut header = [0u8; 16];
        stream.read_exact(&mut header).await.unwrap();
        let mut body = vec![0u8; u32::from_le_bytes(header[..4].try_into().unwrap()) as usize - 16];
        stream.read_exact(&mut body).await.unwrap();
        String::from_utf8(body).unwrap()
    }

    /// A fake usbmux with one phone on the cable whose app waits on the second app port: the hub
    /// finds it there and the app appears like any other.
    #[tokio::test]
    async fn an_app_on_a_phone_on_the_cable_connects() {
        use designer_protocol::*;
        use futures_util::SinkExt;
        let mux = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = MuxAddr::Tcp(mux.local_addr().unwrap());
        let app_port = *APP_PORTS.start() + 1;
        tokio::spawn(async move {
            while let Ok((mut s, _)) = mux.accept().await {
                tokio::spawn(async move {
                    let req = request(&mut s).await;
                    match value(&req, "MessageType") {
                        Some("Listen") => {
                            reply(&mut s, "<key>MessageType</key><string>Result</string><key>Number</key><integer>0</integer>").await;
                            reply(&mut s, "<key>DeviceID</key><integer>7</integer><key>MessageType</key><string>Attached</string><key>Properties</key><dict><key>ConnectionType</key><string>USB</string></dict>").await;
                            let mut rest = [0u8; 1];
                            let _ = s.read(&mut rest).await;
                        }
                        Some("Connect") if value(&req, "DeviceID") == Some("7") && value(&req, "PortNumber") == Some(&app_port.swap_bytes().to_string()) => {
                            reply(&mut s, "<key>MessageType</key><string>Result</string><key>Number</key><integer>0</integer>").await;
                            let mut ws = tokio_tungstenite::accept_async(s).await.unwrap();
                            let info = AppInfo { protocol: PROTOCOL_VERSION.into(), name: "Phone".into(), engine: "dotnet-maui".into(), pid: 9, project: None, agent: None, platform: None };
                            ws.send(tokio_tungstenite::tungstenite::Message::text(serde_json::to_string(&Hello::App(info)).unwrap())).await.unwrap();
                            while futures_util::StreamExt::next(&mut ws).await.is_some() {}
                        }
                        _ => reply(&mut s, "<key>MessageType</key><string>Result</string><key>Number</key><integer>3</integer>").await,
                    }
                });
            }
        });
        let hub = Shared::new(Mutex::new(crate::Hub::default()));
        tokio::spawn(watch(hub.clone(), addr));
        for _ in 0..50 {
            if hub.lock().unwrap().core.apps.values().any(|a| a.info.name == "Phone") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("the phone's app did not connect");
    }
}
