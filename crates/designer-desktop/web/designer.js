// Starts the designer UI in a page: the wasm build, drawnui_host.js, and its link to a hub.
//
//   DrawnUiDesigner.start({ canvas, url: "ws://127.0.0.1:54270", assetBase, log })
//     The VS Code webview: a WebSocket to the hub, owned by this script. Frames from the hub go into
//     the wasm with _designer_receive, then a frame is requested; nothing polls.
//
//   DrawnUiDesigner.start({ canvas, designerHub: "page", assetBase, log })
//     An iframe in DrawFiddle: the hub runs in the wasm, the app and the code are in the parent page.
//     Every message both ways is { channel: "drawnui-designer", type, ... }; only the parent window's
//     are taken (docs/designer.md, "Hosting the editor in a page").
//
// assetBase: where the app's files are served (fonts: "assets/OpenSans-Regular.ttf" is asked as
// assetBase + "assets/OpenSans-Regular.ttf"). log(text): the UI's log lines and the link's events.
window.DrawnUiDesigner = {
  async start({ canvas, url, designerHub, assetBase, log = () => {} }) {
    const inPage = designerHub === "page";
    let socket = null;
    let pumping = false;
    let module, requestFrame;
    // Page mode: the parent holds its messages until the iframe's first one, so the listener and
    // code.get go out before the wasm loads; what arrives meanwhile waits for the hub.
    let early = inPage ? [] : null;
    if (inPage) {
      window.addEventListener("message", (event) => {
        if (event.source !== window.parent || event.data?.channel !== "drawnui-designer") return;
        const text = JSON.stringify(event.data);
        if (early) early.push(text);
        else deliver(module._designer_page, text);
      });
      window.parent.postMessage({ channel: "drawnui-designer", type: "code.get" }, "*");
    }
    ({ module, requestFrame } = await DrawnUi.start({
      canvas,
      history: false,
      create: () =>
        createDrawnUi({
          designerSend: (text) => socket?.readyState === WebSocket.OPEN && socket.send(text),
          designerLog: log,
          // Inside a page (DrawFiddle): only Layers and Properties, no title bar, Screens or status line.
          designerEmbedded: inPage,
          designerPost: (text) => window.parent.postMessage(JSON.parse(text), "*"),
          // The editor's messages run right after the UI's turn, once however many were sent.
          designerDefer: () => {
            if (pumping) return;
            pumping = true;
            queueMicrotask(() => {
              pumping = false;
              module._designer_pump();
              requestFrame();
            });
          },
          printErr: (text) => log("[wasm] " + text),
        }),
      resolveAsset: assetBase ? (asset) => assetBase + asset : undefined,
    }));

    const encoder = new TextEncoder();
    const deliver = (into, text) => {
      const bytes = encoder.encode(text);
      const ptr = module._dui_alloc(bytes.length);
      module.HEAPU8.set(bytes, ptr);
      into(ptr, bytes.length);
      module._dui_free(ptr, bytes.length);
      requestFrame();
    };

    if (inPage) {
      module._designer_page_start();
      for (const text of early) deliver(module._designer_page, text);
      early = null;
      requestFrame();
      return;
    }

    let delay = 500;
    const connect = () => {
      const ws = new WebSocket(url);
      let open = false;
      ws.onopen = () => {
        open = true;
        socket = ws;
        delay = 500;
        log("connected to " + url);
        module._designer_connected(1);
        requestFrame();
      };
      ws.onmessage = (event) => deliver(module._designer_receive, event.data);
      ws.onclose = () => {
        if (open) {
          log("disconnected from " + url);
          socket = null;
          module._designer_connected(0);
          requestFrame();
        }
        setTimeout(connect, delay);
        delay = Math.min(delay * 2, 5000);
      };
    };
    connect();
  },
};
