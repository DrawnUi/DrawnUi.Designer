---
name: drawnui-designer
description: Work on a RUNNING DrawnUI app through the DrawnUI Designer's MCP server — list the connected apps, open screens, jump the app to a state it declares (a level, a dialog, an empty cart) with its designer actions, read the control tree and properties, find which code creates a control, change values live, write them into the app's C# code, undo, and check the result with a screenshot. Works with DrawnUi .NET apps (WPF, MAUI on Windows, Android and iPhone) in Debug builds, DrawnUI for Rust apps and DrawnUI for React apps in development builds. Trigger on "designer", "drawnui designer", "running app", "live edit", "write to code", "change this control", "what does the screen show", "designer mcp", "designer actions", "open the screen", "show level", "jump to state".
---

# DrawnUI Designer for AI agents

The DrawnUI Designer attaches to a running DrawnUI app and shows its screens, its control tree and every control's
properties. People use its window; agents use the same features through MCP. You see exactly what the app draws
now, change a value and watch it apply, then write it into the code that creates the control.

Load `drawnui` (engine semantics) and `drawnui-fluent` (C# composition) with this skill when you also edit code.

## Connect

- The designer serves MCP at `http://127.0.0.1:54271/mcp` while its window (or its VS Code panel) runs.
  Claude Code: `claude mcp add --transport http drawnui-designer http://127.0.0.1:54271/mcp`.
- The MCP does not answer (no designer open)? Start one without a window, in the background:
  `drawnui-designer --mcp`. Exit code **3** means a designer already runs: use its MCP at the address it prints
  (the user's window serves it and shows what you do). Never start a second designer on another port to get around
  it, and stop only a `--mcp` designer you started yourself, never the user's window.
- An app connects by itself when it runs in **Debug** with the designer agent: DrawnUi packages that carry
  `DrawnUi.Designer.Agent` attach it with nothing to set up; Release builds never contain it.
- A DrawnUI for React app connects in development builds once it calls `attach(canvas)` from
  `drawnui-react-designer-agent` with the `<Canvas>` ref's engine canvas (engine `react`). Its properties are the
  engine's PascalCase names with values as text; a live value stays through React re-renders until the code gives a
  new value. `write_property` edits the `.tsx` when the app passed its source folder as `project`; the tree's
  `source` (`/src/App.tsx:12:7`) points at the JSX element.
- Android phones and iPhones on their USB cable connect the same way. Apps on other devices can connect over the
  network after `allow_devices`.

## The tools

| Tool | Use |
|---|---|
| `list_apps` | The connected apps and their `id` (every other tool takes it), plus recent apps you can start. |
| `run_app` / `restart_app` | Start a recent app, or stop and start one again; both wait until it connects. |
| `list_screens` / `open_screen` | The app's shell routes; open one, then read the tree about a second later. |
| `list_actions` / `run_action` | The states the app declared for the designer (`Levels/Level 3`, `Dialogs/Options`); run one to put the app there, then read the tree or take a snapshot. |
| `get_tree` | The controls drawn now: `id`, `parent`, `type`, `field`, `rect` in canvas pixels, `visible`, `source` (file:line of the code that creates it). |
| `get_properties` | A control's properties: name, .NET type, value, whether it was set. |
| `pick` / `select` | The control at a canvas point; draw a box around a control in the app. |
| `set_property` | Change a value in the running app only. |
| `write_property` | Change it in the app AND in the code; returns the file and an `undo` id, or why it was refused. |
| `undo_write` | Take a write back, if that spot of the file was not changed since. |
| `snapshot` | A PNG of what the app draws now. |
| `focus` | Bring the app's window back from the taskbar and to the front. |
| `allow_devices` | Let apps on other devices connect; returns the address to build them with. |

## Workflow

1. `list_apps` → pick the `id`. Reach what the user means: `list_screens` + `open_screen` for a page,
   `list_actions` + `run_action` for a state no route opens (a game level, a dialog, a filled form). Never play
   through the app by taps to get somewhere an action or a screen already reaches.
2. `get_tree` → find the control by `type`, `field` (an `.Assign(out _field)` name), text or `rect`. To match what the
   user points at in a screenshot, use `pick` with the point.
3. `get_properties` → read before you change. Values travel as text: numbers, `True`/`False`, enum names,
   `#AARRGGBB` colors, thicknesses as `all`, `h,v` or `l,t,r,b`.
4. Try it live with `set_property`, check with `snapshot`. When it looks right, `write_property` it.
5. Report the file and line from `write_property`. Keep the `undo` id until the user is happy.

## Screens and actions: reaching what the user means

- **Screens** are the app's shell routes (a `SkiaShell`'s pages). `open_screen` navigates there; the transition
  takes about a second.
- **Actions** are states the app declares for the designer, so nobody has to play to them: a path with its groups
  (`Game/Paused`, `Levels/Level 12`, `Dialogs/Options`). `run_action` runs it in the app on its UI thread, then
  the app sends a fresh tree. A failure comes back in `list_apps` notes and the designer's status bar.
- An app with no actions returns an empty list. When the user keeps asking for a state that is hard to reach, offer
  to declare it as an action in the app's code (below), so the designer and agents get there in one step.

Declaring actions, per engine (nothing runs from them unless a designer is attached):

```csharp
// C#, DrawnUi 1.10.8.22 or newer, any head
using DrawnUi.Draw;
for (var i = 1; i <= 20; i++) { var level = i; DesignerActions.Add($"Levels/Level {level}", () => Game.StartLevel(level)); }
DesignerActions.Add("Dialogs/Options", () => Game.ShowOptions());   // Add returns an IDisposable that removes it
```

```tsx
// React, the drawnui-react release after 0.1.0-preview.40
import { designerAction, useDesignerAction, DesignerAction } from "drawnui-react";
designerAction("Levels/Level 3", () => startLevel(3));             // returns its remover
useDesignerAction("Dialogs/Options", () => showOptions());         // exists while the component is mounted
<DesignerAction Name="Game/Paused" onRun={() => pause()} />         // draws nothing
```

```rust
// DrawnUI for Rust, the drawnui crate release after 0.1.0-preview.36
Ui::new(state, build).designer_action("Levels/Level 3", |s: &mut Game, _cx| s.start_level(3));
// while running: cx.add_designer_action::<Game>(path, |s, cx| ..) / cx.remove_designer_action(path)
```

```
// NX (DrawFiddle): an element in a component; onRun runs like that component's onTapped
<DesignerAction Name="Levels/Level 3" onRun={<State.Update level=3 />} />
```

C# apps with Hot Preview commands (static `[Preview]` methods without parameters) also list them, under `Previews/`.

## When a write is refused

`write_property` changes one literal in fluent C#: an object initializer value (`FontSize = 18`), a constructor
argument (`new SkiaLabel("Text")`), `Color.Parse("#..")` / `Colors.X`, `new Thickness(..)`, fluent `.WithX(..)`
arguments, or the argument at the call of a small helper. It refuses, with the reason, for values that are computed,
bound, set in loops or templates, set in XAML, or set by the library for its own parts. Then edit the code yourself:
the tree's `source` gives the file and line.

## After a code edit

Under `dotnet watch` on Windows, a code change reaches the open screen by itself (hot reload). On phones hot
reload needs the IDE debugger; otherwise `open_screen` again (or `restart_app`) after editing, then `get_tree`
and `snapshot` to check.
