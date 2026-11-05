//! The editor's connection to the hub: one thread with a small tokio runtime. Messages from the
//! hub go into a channel the UI drains, and each one wakes the UI's inbox control; messages from
//! the UI come through another channel. Reconnects forever, so the window can start before the hub
//! or outlive one.

use std::sync::mpsc::Sender;
use std::time::Duration;

use designer_protocol::{EditorMsg, Hello, HubToEditor, PROTOCOL_VERSION};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_tungstenite::tungstenite::Message;

use crate::ui::Incoming;

/// Starts the connection thread. `wake` runs after each message is queued.
pub fn start(port: u16, to_ui: Sender<Incoming>, mut from_ui: UnboundedReceiver<EditorMsg>, wake: impl Fn() + Send + 'static) {
    std::thread::Builder::new()
        .name("designer-editor-link".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
            runtime.block_on(async move {
                let url = format!("ws://127.0.0.1:{port}");
                loop {
                    if let Ok((ws, _)) = tokio_tungstenite::connect_async(&url).await {
                        let (mut sink, mut source) = ws.split();
                        let hello = serde_json::to_string(&Hello::Editor { protocol: PROTOCOL_VERSION.into() }).expect("serializable");
                        if sink.send(Message::text(hello)).await.is_ok() {
                            let _ = to_ui.send(Incoming::Connected(true));
                            wake();
                            loop {
                                tokio::select! {
                                    frame = source.next() => match frame {
                                        Some(Ok(Message::Text(text))) => {
                                            if let Ok(msg) = serde_json::from_str::<HubToEditor>(&text) {
                                                let _ = to_ui.send(Incoming::Hub(msg));
                                                wake();
                                            }
                                        }
                                        Some(Ok(_)) => {}
                                        _ => break,
                                    },
                                    out = from_ui.recv() => match out {
                                        Some(msg) => {
                                            let text = serde_json::to_string(&msg).expect("serializable");
                                            if sink.send(Message::text(text)).await.is_err() {
                                                break;
                                            }
                                        }
                                        None => return, // the window closed
                                    },
                                }
                            }
                            let _ = to_ui.send(Incoming::Connected(false));
                            wake();
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            });
        })
        .expect("editor link thread");
}
