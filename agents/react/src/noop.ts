// Production entry: the agent does nothing and costs nothing (the package's "development" condition picks index.ts).
import type { AgentOptions, SourceMapLookup } from "./index";
export type { AgentOptions, SourceMapLookup };
export function attach(_canvas: unknown, _options?: AgentOptions): () => void { return () => {}; }
export function attachPage(_canvas: unknown, _send: (text: string) => void, _options?: AgentOptions): (message: string | object) => void { return () => {}; }
export function detach(): void {}
export function sourceMapLookup(_map: unknown, _moduleUrl: string): SourceMapLookup { return () => undefined; }
