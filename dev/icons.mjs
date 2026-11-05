import { pathToFileURL } from 'node:url';
import fs from 'node:fs';
// node dev/icons.mjs <out.rs> <the lucide package's dist/esm/icons folder> (npm i lucide@1.61.0 anywhere)
const dir = process.argv[3].replace(/[\\/]?$/, '/');
const names = ['layers','rows-3','columns-3','type','mouse-pointer-click','square','image','scroll-text','shapes','app-window','file-code','sliders-horizontal','text-cursor-input','grid-3x3','text-wrap','box','toggle-left','code','crosshair','unplug','circle-check','circle-alert','layout-panel-top','sparkles','rotate-cw','play','check','undo-2','settings','palette','info','plug','copy','arrow-left','circle-question-mark','keyboard','bot','chevron-down','smartphone','monitor','tablet','search','square-plus'];
let out = `//! Lucide icons (https://lucide.dev, ISC license, v1.61.0) as SVG strings, drawn with SkiaSvg and\n//! tinted. Generated from the lucide package; add a name to dev/icons.mjs to add an icon.\n\n`;
for (const n of names) {
  const node = (await import(pathToFileURL(dir + n + '.mjs'))).default;
  const body = node.map(([tag, attrs]) => `<${tag} ${Object.entries(attrs).map(([k,v]) => `${k}="${v}"`).join(' ')}/>`).join('');
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`;
  out += `pub const ${n.toUpperCase().replace(/-/g,'_')}: &str = r##"${svg}"##;\n`;
}
fs.writeFileSync(process.argv[2], out);
console.log('icons', names.length);
