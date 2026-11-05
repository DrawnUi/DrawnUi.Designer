// Runs the agent's checks in Node (as DrawnUi.React's dev/check.mjs does): bundles a test file with esbuild against the
// drawnui-react package (output in node_modules/.cache, so esbuild itself stays external), CanvasKit's ?url wasm import
// stubbed. Usage: node test/run.mjs [test/agent.test.tsx]
// DRAWNUI_REACT=<a DrawnUi.React checkout or an npm package folder, with dist> runs them against that build instead.
// MINIFIED=1 loads the library as DrawFiddle does: one minified bundle (React included) that publishes its exports as
// globals (DrawnUi, DrawnUiCore); the agent and the checks reach it only through them.
import { execFileSync } from "node:child_process";
import { basename, join, resolve } from "node:path";
import esbuild from "esbuild";

const lib = resolve(process.env.DRAWNUI_REACT ?? "node_modules/drawnui-react");
const libFiles = { "drawnui-react": "dist/react/index.js", "drawnui-react/core": "dist/index.js", "drawnui-react/reconciler": "dist/react/reconciler.js" };
const minified = !!process.env.MINIFIED;
console.log(`drawnui-react: ${lib}${minified ? " (minified, through globals)" : ""}`);

const entry = process.argv[2] ?? "test/agent.test.tsx";
const cache = resolve("node_modules", ".cache");
const outfile = join(cache, `drawnui-designer-agent-${basename(entry).replace(/\.tsx?$/, "")}.cjs`);
const stubWasm = { name: "stub-wasm-url", setup(b) { b.onResolve({ filter: /\?url$/ }, (a) => ({ path: a.path, namespace: "stub" })); b.onLoad({ filter: /.*/, namespace: "stub" }, () => ({ contents: "export default ''", loader: "js" })); } };
const fromLib = { name: "drawnui-react", setup(b) { b.onResolve({ filter: /^drawnui-react(\/core|\/reconciler)?$/ }, (a) => ({ path: join(lib, libFiles[a.path]) })); } };
// no Vite here: DesignerSourceOf stays off
const common = { bundle: true, platform: "node", target: "node20", logLevel: "warning", jsx: "automatic", define: { "import.meta.env": "{}" }, nodePaths: [resolve("node_modules")] };

const globals = { "drawnui-react": "DrawnUi", "drawnui-react/core": "DrawnUiCore", "drawnui-react/reconciler": "__DrawnUiReconciler", react: "__React", "react/jsx-runtime": "__ReactJsx" };
const runtime = join(cache, "runtime.min.js");
if (minified) {
  const contents = Object.entries(globals).map(([spec, g], i) => `import * as m${i} from ${JSON.stringify(spec)}; globalThis.${g} = m${i};`).join("\n");
  await esbuild.build({ ...common, stdin: { contents, resolveDir: process.cwd(), loader: "js" }, outfile: runtime, format: "iife", minify: true, plugins: [fromLib, stubWasm] });
}
const toGlobals = { name: "globals", setup(b) {
  b.onResolve({ filter: /^(drawnui-react(\/core|\/reconciler)?|react|react\/jsx-runtime)$/ }, (a) => ({ path: a.path, namespace: "global" }));
  b.onLoad({ filter: /.*/, namespace: "global" }, (a) => ({ contents: `module.exports = globalThis.${globals[a.path]};`, loader: "js" }));
} };
await esbuild.build({
  ...common, entryPoints: [entry], outfile, format: "cjs", external: ["esbuild"], conditions: ["development"],
  banner: { js: "globalThis.window ??= globalThis;" + (minified ? ` require(${JSON.stringify(runtime)});` : "") },
  plugins: minified ? [toGlobals, stubWasm] : [fromLib, stubWasm],
});
execFileSync(process.execPath, [outfile], { stdio: "inherit" });
