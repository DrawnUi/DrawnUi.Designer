// Linked into the web build with --js-library (build.rs): the functions the wasm calls to reach the
// page. The page passes `designerSend` / `designerLog` / `designerPost` / `designerDefer` in the
// module options (web/designer.js).
addToLibrary({
  designer_send: (text, len) => Module.designerSend?.(UTF8ToString(text, len)),
  designer_log: (text, len) => Module.designerLog?.(UTF8ToString(text, len)),
  designer_post: (text, len) => Module.designerPost?.(UTF8ToString(text, len)),
  designer_defer: () => Module.designerDefer?.(),
  designer_embedded: () => (Module.designerEmbedded ? 1 : 0),
  // A parameter of the page's address into the wasm's buffer: its length in bytes, or -1 when absent.
  designer_param__deps: ["$UTF8ToString", "$stringToUTF8", "$lengthBytesUTF8"],
  designer_param: (name, len, out, cap) => {
    const value = new URLSearchParams(location.search).get(UTF8ToString(name, len));
    if (value === null) return -1;
    const bytes = lengthBytesUTF8(value);
    if (bytes < cap) stringToUTF8(value, out, cap);
    return bytes;
  },
});
