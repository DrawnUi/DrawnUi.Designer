# HelloWpf runtime trees

Real `tree` messages (`AppMsg::Tree`, protocol 0.1.0, pretty-printed) recorded from the running HelloWpf
sample, for testing the code matcher.

| File | Screen |
|---|---|
| `root.json` | start screen (catalog) |
| `layouts.json` | `openScreen("layouts")` |
| `text.json` | `openScreen("text")` |
| `shapes.json` | `openScreen("shapes")` |
| `cells.json` | `openScreen("cells")` |

- Recorded: 2026-10-31
- DrawnUi commit: `f5555dc4` (no uncommitted changes in `src`)
- Build: HelloWpf `Release`, net10.0-windows, default rendering mode, window at its default size
  (canvas 1207x978 px at 125% display scale)
- `root.json` recorded again on 2026-11-03 for the root page with its cards in a masonry (DrawnUi `9f074960`,
  HelloWpf `Debug` build of that source, same window size and scale).

## How they were made

Each screen comes from a fresh app start:

1. `agents/dotnet/TestHub` in dump mode: `TestHub dump <out.json> <route|root> <port>`.
2. HelloWpf started with `DOTNET_STARTUP_HOOKS=agents\dotnet\Agent\bin\Release\net8.0-windows\DrawnUi.Designer.Agent.dll`
   and `DRAWNUI_DESIGNER_URL=ws://127.0.0.1:<port>`.
3. The hub waits for Hello, Screens and the first Tree, sends `openScreen` (not for `root`), waits 1.5 s for the
   page transition to settle, sends `getTree` and writes the answer.

Node fields filled by the agent:

- `assembly`: the declaring assembly of the control's type (`DrawnUi.Wpf` or `HelloWpf` here).
- `internal`: the library made the control for its own parts. True for a type nested in a library type
  (`SkiaButton+ButtonLabel`), and for a library-type control inside a subtree the library made: the children a
  control built itself in `CreateDefaultContent` (DrawnUi's `SkiaControl._ownsDefaultContent`, such as a button's
  shape and label) and everything a `SkiaShell` adds (tab container, page hosts, nav bar, back button, title)
  except the app views it holds (root content, pages, popup / modal / toast content). An instance of an app class,
  or a recycled cell, is never internal.
- `templateIndex`: the item index (`ContextIndex`) of a recycled cell of a templated layout.
- `field`: `Namespace.Class._field` when an instance field of an app class (the app's windows, the Application,
  or an app-class control in the tree, nearest owner winning) holds the control.

| File | nodes | internal | templateIndex | field |
|---|---|---|---|---|
| `root.json` | 111 | 1 | 0 | 1 |
| `layouts.json` | 490 | 35 | 22 | 53 |
| `text.json` | 209 | 14 | 0 | 3 |
| `shapes.json` | 263 | 14 | 0 | 1 |
| `cells.json` | 210 | 29 | 10 | 35 |

A pushed page sits above the start screen in the shell, so every page fixture also holds the `RootPage`
subtree (108 nodes, all `visible: false`).
