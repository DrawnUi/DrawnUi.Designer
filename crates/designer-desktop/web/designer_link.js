// Linked into the web build with --js-library (build.rs): the functions the wasm calls to reach the
// page. The page passes `designerSend` / `designerLog` / `designerPost` / `designerDefer` in the
// module options (web/designer.js).
addToLibrary({
  designer_send: (text, len) => Module.designerSend?.(UTF8ToString(text, len)),
  designer_log: (text, len) => Module.designerLog?.(UTF8ToString(text, len)),
  designer_post: (text, len) => Module.designerPost?.(UTF8ToString(text, len)),
  designer_defer: () => Module.designerDefer?.(),
  designer_embedded: () => (Module.designerEmbedded ? 1 : 0),
});
