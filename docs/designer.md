# DrawnUI Designer

DrawnUI Designer attaches to your running DrawnUI app and shows what is on screen: its screens, the tree of
controls and each control's properties. Change a property and the app shows it at once. Write it to code and
the designer changes the line in your C# file that creates that control, and leaves the rest of the file as it
was.

It works three ways, all on the same running apps:

- a desktop window,
- a panel inside VS Code,
- an MCP server, so an AI agent can see your app and change it the same way.

Today it attaches to DrawnUi.Net WPF apps and DrawnUi.Maui apps on Windows.

## Start the designer

**Desktop window.** Download it from the repository's Releases: `drawnui-designer-<version>-windows-x64.exe` or
`-windows-arm64.exe` (one file, run it), or `DrawnUI-Designer-<version>-macos-universal.zip` (unzip, move the app to
Applications). Or from this repository:

```powershell
cargo run -p designer-desktop --release
```

The window starts the hub, the small local server that apps and editors connect to. If a hub already runs
(another window, or VS Code), the window uses that one.

**VS Code.** Download `drawnui-designer-0.5.0.vsix` from the repository's Releases and install it (**Extensions:
Install from VSIX...**, or the command below); once it is on the Marketplace, search the Extensions view for
**DrawnUI Designer** (`drawnui.drawnui-designer`) instead.

```powershell
code --install-extension drawnui-designer-0.5.0.vsix
```

Then run **DrawnUI: Open Designer**. The designer opens beside your code and starts the hub if none is running. The
panel has the window's menu bar (no Exit: close its tab), keys and Settings, and keeps its settings in the window's
settings file. While it is the active tab its keys are its own: Ctrl+P (Pick), Ctrl+R (Restart app), Ctrl+,
(Settings), F1 (Help), Ctrl+D, Ctrl+Shift+C / Ctrl+Shift+V and Alt+F / Alt+E (the menus) do not reach VS Code; the
Command Palette is Ctrl+Shift+P there. A tab closes without asking: changes not in code yet stay live in the app (while
VS Code runs the hub), and VS Code says how many, with **Reopen**.

## Attach your app

**Debug builds of a DrawnUi app attach by themselves** when the app references the NuGet package
`DrawnUi.Designer.Agent`. DrawnUi.Maui and DrawnUi.Wpf 1.10.8.15 or newer bring it along: then there is nothing to do. Otherwise
add it to the app:

```xml
<PackageReference Include="DrawnUi.Designer.Agent" Version="0.1.0-preview.6" />
```

Run the app as usual (F5, `dotnet run`, `dotnet watch`, on any head). The package changes no code: in Debug builds
it adds the agent (a startup hook on Windows, a generated initializer on iPhone, iPad and Android); Release builds get
nothing. `DrawnUiDesigner=false` (an MSBuild property) turns it off.

**Without the package** (a DrawnUi version from before it, or to try an agent built from this repository), give the
build the agent's targets file; your project file stays as it is:

```powershell
dotnet build agents\dotnet\Agent -c Release
cd <your app>
dotnet watch run --non-interactive --property:CustomAfterMicrosoftCommonTargets=<this repository>\agents\dotnet\DrawnUi.Designer.targets
```

A .NET MAUI app on Windows also needs `-f net10.0-windows10.0.19041.0` (its Windows target). The agent then comes from
`agents\dotnet\Agent\bin\Release` (`net8.0-windows` for Windows, `net8.0` for phones); `DRAWNUI_DESIGNER_AGENT`
picks another DLL. On Windows the agent goes into the app's `runtimeconfig.json` as a startup hook, which the .NET
host loads next to `dotnet watch`'s own hook; a later build without the property writes the runtimeconfig without it.
If your build already sets `CustomAfterMicrosoftCommonTargets` itself, the property above replaces yours for this
run. With both the package and the property, the first one MSBuild imports is used.

With hot reload, a property you write to code shows in the running app a few seconds after the file is saved.

The agent works with whatever DrawnUi version your app uses, WPF or MAUI: one agent DLL for both. The app appears
in the designer as soon as it starts. The title bar shows one app, the last one: the app that connected last, or, with none connected,
the most recent one with a button to run it again. AI agents see every connected app (`list_apps`).

**MAUI apps.** The designer shows the canvas of the page the app shows now (the largest one when a page has several)
and the app's Shell routes as screens. Opening a screen goes there from the Shell's root, so screens never stack up.
A page built in code (`BasePageReloadable`) rebuilds itself after a hot reload while a debugger is attached; under
`dotnet watch` alone, open the screen again in the designer to see the new code.

**MAUI on iPhone, iPad, Mac Catalyst and Android.** These runtimes load no startup hook from the build machine, so the
agent goes into the app at build time instead: the Debug build references the agent and gets a generated initializer
that starts it. An iPhone or iPad then waits for the designer on the USB cable (see below), with no address in the
build. To reach the designer over the network instead, add
`"--property:DrawnUiDesignerUrl=ws://<the Mac's address>:54272/?token=<token>"`: the URL goes to the agent unchanged
and must reach the hub from the device. An Android Debug build without a URL connects to `ws://127.0.0.1:54270`
on the phone, which the designer forwards to itself over the USB cable (`adb reverse`). A Mac Catalyst Debug build
without a URL connects to `ws://127.0.0.1:54270`, the designer on the same Mac. The build's `DRAWNUI_DESIGNER_URL` variable counts
as a URL everywhere (on iOS it replaces the cable). Bring the window forward does nothing on a phone.

**What the agent does in your app.** Only builds that do not optimize (Debug) get it, on every platform;
`DrawnUiDesigner=false` turns it off, and a Release build warns if the agent DLL ends up in its output. At start it
prints one line, `DrawnUI Designer agent: waiting for a designer at <address>` (on the cable: `... on the USB cable
(port 54273)`). Until a designer answers it only tries to connect to that one address, with a pause growing from 1 s
to 30 s, or, on the cable, waits for the designer to connect: no work on the UI thread, no event subscriptions.
Connected, it reads the tree, properties and screenshots when the designer asks (and sends a new tree after a
navigation), and it unhooks everything when the designer leaves; one designer at a time. Nothing in the app listens
on a port, except an iPhone or iPad build without a URL: it listens on the phone's own `127.0.0.1` only, port 54273
(or the next free one up to 54282), which the computer reaches only through the cable, like the .NET debugger.

**iPhones on the USB cable.** The designer looks for iPhones on the cable through Apple's usbmux service (on macOS
always there; on Windows it comes with Apple's device software). While a phone is plugged in, the designer tries the
ports an app's agent waits on there (54273 to 54282) every 2 seconds and connects to each app it finds, the way the
.NET debugger reaches an app. No network and no address in the build are needed.
When iOS takes that connection away (the lock screen, a long time in the background), the agent opens it again by
itself, and on coming back to the front the app shows up again within a few seconds. An app that comes back keeps
its id in the designer and in MCP (same name, process and platform).

**Rust apps (DrawnUI for Rust).** Add the agent crate and wrap your `Ui` where you hand it to `drawnui::run`:

```toml
[dependencies]
drawnui-designer-agent = { git = "https://github.com/DrawnUi/DrawnUi.Designer" }
```

```rust
drawnui::run("My app", || drawnui_designer_agent::attach!(Ui::new(App::default(), build)));
```

`attach!` returns the app with the agent around it in debug builds and the app as it is in release builds. The crate
turns on drawnui's `designer` feature itself (drawnui 0.1.0-preview.36 or newer from crates.io). The
app shows up with engine `rust`, its exe name, and its crate's `Cargo.toml` as the project. The tree lists the
controls with their type, tag, text and where they are drawn, the engine's own parts (a button's caption, a
switch's look, scroll bars) marked as the library's; each open popup window (a menu, a dropdown) follows as a root
of its own, named `popup 1`, `popup 2`, its controls placed where the popup shows, in canvas pixels (past the
canvas's edge when its window opens there). Properties are the engine's own names (`font_size`, `text_color`,
`is_visible`) with their Rust values (`true`, `#FFF26854`, `l,t,r,b`), set live; enum properties offer their cases
as a dropdown (`horizontal_options`: Start, Center, End, Fill). For an optional enum, `None` is the enum's own case
where it has one, and an empty value clears the property. The highlight box (in a popup window too), Pick in the
app (Esc cancels), snapshots and Bring to front work as for .NET apps, also while the app is minimized: the app
draws the frame that answers off screen. The Controls panel lists the engine's controls (layout, scroll, label,
editor, button, checkbox, radio button, switch, slider, progress, image, SVG, shape); they are placed, pasted with
their properties and children (or dropped by a click in the app: Place puts it into the container under the pointer, after the child there; Esc cancels), copied, taken out and moved among their siblings live (into another control not
yet). A tree waits for the app's controls to be built and laid out (fonts still loading), up to about a second.
Write to code changes the crate's `.rs` files (see What Write to code changes, Rust). Screens are not there yet, and a
snapshot shows the app's window, not its popup windows. `DRAWNUI_DESIGNER=0` turns the agent off, `DRAWNUI_DESIGNER_URL` points it
at another hub. While no designer runs it tries the one address every 2 s on its own thread and adds no work to the
UI thread; connected, each request costs one frame. The sample is `agents/rust/sample` (`cargo run -p
designer-agent-sample` in `agents/rust`); its Quick actions switch opens a popup menu. Run app and Restart app start
a desktop app with `cargo run` in its crate's folder (a debug build, so the agent attaches); the line at the bottom
says when the build fails, with its first error (all of them, and where the full output is, in its tooltip). A
failed run is over at once: Run, the pill's run button and Restart app (Ctrl+R, which runs the app when none is
connected) start it again.

**Rust apps in a web page (DrawFiddle).** In the browser build (`wasm32-unknown-emscripten`, debug or release)
`attach!` wraps the app the same way, and the page carries the messages: no socket, no thread, no timer. Until a
page attaches, a frame costs one call that compares two numbers. The app's link flags need `addFunction` among
`EXPORTED_RUNTIME_METHODS` and `-sALLOW_TABLE_GROWTH=1` (`agents/rust/.cargo/config.toml` has the full list).
With `module` and `requestFrame` from `DrawnUi.start`:

```js
const sink = module.addFunction((ptr, len) => toDesigner(module.UTF8ToString(ptr, len)), "vii");
const call = (fn, first, text) => {
  const bytes = new TextEncoder().encode(text);
  const ptr = module._dui_alloc(bytes.length);
  module.HEAPU8.set(bytes, ptr);
  first === null ? fn(ptr, bytes.length) : fn(first, ptr, bytes.length);
  module._dui_free(ptr, bytes.length);
  requestFrame();
};
call(module._dui_designer_attach, sink, "My app");          // attach: the hello and the tree go out
call(module._dui_designer_receive, null, designerJson);     // each message of the designer
module._dui_designer_attach(0, 0, 0);                       // detach
```

- Out, through `sink`, during frames: the hello (`{"role":"app", "engine":"rust", "platform":"browser",
  "pid":0, "project":null, ...}`) once per attach, then `AppMsg` JSON (`tree`, `props`, `picked`, `snapshot`,
  `error`). `sink` runs inside a frame: queue or `postMessage` the text, never call into the module from it.
- In, with `dui_designer_receive`: `HubToApp` JSON (`getTree`, `getProps`, `setProperty`, `highlight`, `pick`,
  `pickMode`, `snapshot`, `focus`, `openScreen`, `getControls`, `insert`, `placeMode`, `remove`, `move`, `copy`) and `{"type":"rebuilt"}` (the app rebuilt its controls in place:
  a new tree goes out). Each is handled at the next frame, so ask for one after each (`requestFrame`).
- Attaching again starts a new session (a new hello). A new module instance (a full reload) is a new app: attach
  its module again, with a sink added to its own table.
- Hot reload (`dui_hot_swap`): the app's maker wraps it with `drawnui::hot_reload!(App, |s|
  drawnui_designer_agent::in_page(make(s)))`, and the engine's main module links the agent
  (`drawnui_designer_agent::page::link_in()` in its `main`). The link's state lives there, so a swapped-in app
  finds the page attached and sends its tree once its controls are laid out; the page does nothing, and no second
  hello comes. The app's state crosses the swap as without the agent (the wrapper hands `hot::save` its `Ui`).

`agents/rust/sample/web/page-check.cjs` checks it all in Node with the sample's browser build (drawn on the CPU),
the script playing the page and the designer.

**React apps (DrawnUi.React).** Add the agent package to the app and attach it to the engine canvas, the value
the `<Canvas>` ref gives:

```
cd <this repository>/agents/react && npm install && npm run build     # once: builds dist/
cd <your app> && npm i -D <this repository>/agents/react
```

```tsx
import { attach } from "drawnui-react-designer-agent";

<Canvas ref={(c) => { if (c) attach(c, { name: "My app" }); }} ...>
```

The package has two entries: development builds (Vite `dev`, any bundler that resolves the `development` export
condition) get the agent, production builds get functions that do nothing, so nothing of the agent is in the
bundle. `drawnui-react-designer-agent/dev` is the agent always. It needs `drawnui-react` as a peer dependency and
uses the app's own copy: when the agent comes from a folder outside the app, add `resolve: { dedupe:
["drawnui-react", "react", "react-dom"] }` to the Vite config. `attach` connects to `ws://127.0.0.1:54270` (option
`url`, or `globalThis.DRAWNUI_DESIGNER_URL`), prints one console line, and while no designer runs tries again after
1 s, doubling to 30 s, with no other work. It returns a function that disconnects; `detach()` does the same.
Options: `name` (default: the page title), `project` (the folder of the app's sources: Write to code reads and
edits the `.tsx` files there; a page cannot know it by itself), `screens` (`{ routes, titles, open }` when the agent cannot
find the app's `SkiaShell` by itself), `controls` (the app's own control classes for the Controls panel; the ones
in the tree are found by themselves) and `mapSource` (below).

The app shows up with engine `react` and platform `browser`. The tree lists the controls in the order they are
drawn, a templated layout with its realized cells, each open popup window (a menu, a dropdown) as a root of its own
named `popup 1`, `popup 2`, when drawnui-react lists them (`Canvas.PopupWindowRoots`). The parts a control makes for
itself (a button's caption and its shape) are marked as the library's. Properties are the engine's own names
(`FontSize`, `Margin`, `TextColor`) with their values as text: numbers, `true`/`false`, `#AARRGGBB` colors,
thicknesses `l,t,r,b`, the string choices of the common enums (`HorizontalOptions`: Start, Center, End, Fill);
"set" means different from a new control of the class. A live value stays while React draws the component again
(an inline `Margin={new Thickness(8)}` is written again on every render): the agent takes that property over on
that one control, drops a write of the value the code had, and lets any other value through, so the next value
your code gives after an edit and hot reload wins. Undo back to the code's value, a closed designer and a control
that is gone end it. Deleting a control the agent added takes it out; deleting one your JSX makes hides it until
the code no longer makes it, and Undo shows the same control again. Moving a control the agent added moves it;
moving one your JSX makes leaves it, hidden, where React has it and shows a copy of it at the new place under the
same id (React then never finds its controls moved behind its back). Moving it back to its place shows the original
again; the copy goes when React unmounts the original (the move written to code and reloaded) and when the designer
closes. The copy keeps the values it had: state your component changes later reaches the hidden original only. The highlight box is drawn at the end of the
app's own frames; Pick and place take the pointer over the canvas while they are on (Esc cancels). Bring to front
does nothing in a browser. Write to code changes the JSX in the `.tsx` file (`project` set); Vite then reloads the
page with the new code.

With drawnui-react's `DesignerSourceOf` (development React builds under Vite), every node carries `source`
(`file:line:column` of the JSX element's `<` in the original `.tsx`, read through the served module's source map,
such as `/src/App.tsx:12:7`), which the designer matches first.
A page that evaluates code itself maps its positions with `mapSource(file, line, column)`; the agent exports
`sourceMapLookup(map, moduleUrl)` for that. React's production build has no such information: then nodes carry
no `source`.

**A page that hosts the editor (DrawFiddle).** `attachPage(canvas, send, options)` returns the function that takes
the designer's messages (JSON text or objects); `send` gets the agent's messages as JSON text, the hello first. Send
`{"type":"rebuilt"}` after the page rebuilt the controls in the same canvas, call `attachPage` again for a new
canvas, and `detach()` when the editor closes. The checks: `npm test` in `agents/react` runs the agent against
drawnui-react on CanvasKit in Node (`DRAWNUI_REACT=<a DrawnUi.React checkout or npm package folder with dist>` for
another build; `MINIFIED=1` loads it as DrawFiddle does, one minified bundle reached through the `DrawnUi` and
`DrawnUiCore` globals). Type names come from the library's export names, so a minified library (DrawFiddle) still
shows `SkiaLabel`; the agent works with drawnui-react 0.1.0-preview.33 and newer.

**Ports.** The hub listens on `127.0.0.1:54270` and its MCP server on `127.0.0.1:54271`. Apps on other devices, when
allowed, connect to port 54272 on the network. To use other ports, set
`DRAWNUI_DESIGNER_PORT` for the designer (the MCP port is that port + 1) and
`DRAWNUI_DESIGNER_URL=ws://127.0.0.1:<port>` for the app. In VS Code the setting is `drawnuiDesigner.port`.
When a designer already holds the port, a second one joins its hub; when that designer closes, the second one
starts its own hub on the port. A hub of another designer version (an older
build still running) is named in the status bar: "Connected to the hub of another designer (version X); close it
to use this one".

## The window

**Menu bar**, at the left of the title bar, before the app:

| Menu | Items |
|---|---|
| File | Run app (the recent apps that are not running), Restart app (Ctrl+R) · Settings (Ctrl+,), About · Exit |
| Edit | Undo, Redo (Ctrl+Z, Ctrl+Y) · Copy (Ctrl+C), Paste (Ctrl+V), Duplicate (Ctrl+D) · Copy style (Ctrl+Shift+C), Paste style (Ctrl+Shift+V) · Delete (Del; on macOS also Backspace and Cmd+Backspace) |

Right of the apps, **Pick** (Ctrl+P): lit while the next click in the app selects a control. At the right end, a
newer release (**Update** and its version, while one is out), the count of changes not in code with **Write all**
(while there are some), Settings, and Help (F1). A click on Update offers **Update and restart** (the new version
replaces this one and opens; changes not in code are written or asked about first, as before closing), **What's new**
(its release page) and **Later** (that version is not offered again).

Click a title, or press Alt with its underlined letter. Alt alone puts the keyboard on the bar: Left and Right
move between the titles, Down, Enter or Space open one. In a menu, Up and Down move through the items that can act
(round at the ends), Right opens a submenu or the next menu, Left closes the submenu or opens the menu before,
Enter runs the item, Esc closes one level; Esc on the bar, or Alt again, gives the keyboard back to where it was.
A click outside closes everything. Each item does what its button
or key does, shows that key, and is dimmed when it cannot act now. The keys work with the menus closed too,
while no text field has the keyboard; on macOS they use Command. In a page (DrawFiddle) there is no title bar: Pick
is in the Layers header, and the count of changes not in code is a strip of its own under it.

**Screens.** The pages your app's shell can open. Tap one to open it in the app. Under them, the app's **actions**
in their groups (see Actions below): tap one to put the app in that state.

**Controls**, under Screens (drag the line between them to give one more room). The controls your app can
create, as its agent lists them: the library's by kind (Layout, Text, Media, Input, Shapes, Other), then your own
classes under **This app**, each group A to Z. Click a group's header to fold it; type in the search field to find a
control by name. See [Placing controls](#placing-controls).

**Layers.** The controls of the current screen as a tree. A control a helper of the code made reads by the name the
code gives it, its type after it dimmed: `Circle(..) => new SkiaShape { .. }` reads "Circle · SkiaShape" (C#
methods and local functions, found in the code by the designer; React components, from a development build of the
app or the code in a page). Each row shows the control's type, the field that
holds it (from `.Assign(out _field)`) and its text when it has one. Click a row to select the control: a box
appears around it in the app. Double-click a row, or click its box on the tree line, to fold or unfold it. Parts
the library builds for itself, such as a button's inner label, are left out, because no code of yours creates
them. The tree follows the app: opening a screen here, moving to another page in the app itself, or
a hot reload brings the new screen's tree.

**Properties.** The selected control's properties with their current values, only those your code can set (not a
computed value with no setter, such as `WillClipBounds`), in one of two orders:

- **Set**: two groups, each A to Z: Set (in your code or edited here) and Unset. The designer always opens on it.
- **Type**: Layout, Text, Look, Transform, Behavior.

Changed properties are written brighter. Click a value to edit it where it is: a field for text and numbers
(with - and + for numbers), a menu for choices (True / False too: typing moves to the first choice it starts, so
"fa" and Enter picks False), a color picker for colors (a saturation and brightness square, hue and opacity bars, a hex
field; the color changes in the app as you drag). Enter sets the value in the running app; Esc puts it back as it
was and leaves the field. The value a property already has is no change: nothing is sent, marked or written.

Each row of a changed property has two buttons at its right:

- **Undo** (the curved arrow) takes the last change back: a value set in the app returns to what it was before;
  after Write, the edit in your C# file is taken back too, byte for byte, as long as that spot of the file was
  not changed since.
- **Write** (the `<>` icon) writes the value into your C# file. It shows on a changed row, and on any row
  under the mouse. The line at the bottom of the window says which file changed, or why nothing was written; a
  long line is cut, and its whole text is in its tooltip and read by a screen reader.

**Help** (the question mark next to Settings) explains all of this in the window.

**Settings** (the gear at the right of the title bar; Esc or the arrow goes back):

- **Appearance**: the theme: Dark, Midnight, Graphite, Light, Paper or High contrast, each shown as a small
  picture of the window. The new theme opens from where you clicked, over the old one. The themes are defined
  in `crates/designer-desktop/assets/themes.json` (every role of the window as `#RRGGBB`): edit it to change a
  theme or add one.
- **Appearance** also has the **Language**: System (follows Windows), English, Deutsch, Español, Français,
  Srpski or Русский. The texts live in `crates/designer-i18n/i18n/<code>.ftl` (Fluent); every language
  has every key of `en.ftl` (a test checks it). The hub's messages shown in the window (why a value was not written
  to code, why an app did not run) follow the language too; AI agents always get them in English.
- **Layers**: also list the parts the library builds for itself.
- **Connection**: where apps and AI agents reach the designer, and the command that adds its MCP server to
  Claude Code, each with a copy button. **Apps on other devices** lets an app on a phone or another computer on the
  same network connect: the designer then listens on port 54272 (hub port + 2) on the network and shows the address
  to build the app with, `ws://<this computer>:54272/?token=<secret>`. Only apps with that token get in, and only apps
  (an editor never connects from another device). The token is made once and kept in the designer's settings folder
  (`device-token`), so apps built with it keep connecting. The system may ask once to allow the designer on the
  network.
- **About**: the version, links, **Check for updates** (on by default: the desktop window asks GitHub for a newer
  release at start and once a day, nothing else is sent), what the designer is made with and what it uses.

Screen readers (Narrator, NVDA, VoiceOver, Orca) read every control with its name and state, and the keyboard
walks them: Tab between groups, the arrow keys inside a list, a menu or a switch, Enter or Space to act.

The designer remembers these, plus the widths of the Screens and Properties panels, the split between Screens
and Controls, the folded property and control groups and the last Settings section, in `%APPDATA%\DrawnUi.Designer\settings.json`.

Values are typed as text: `18`, `true`, `Hello`, `Center`, `#FF0000` for colors (`#RGB`, `#RRGGBB` or
`#AARRGGBB`), and `8`, `8,4` or `8,4,8,4` (left, top, right, bottom) for margins and padding.

## Actions

Some states are hard to reach by hand: level 12 of a game, the game-over dialog, an empty cart. The app declares them
as actions; the designer lists them under its screens, in their groups, and one click puts the running app there.
AI agents run the same actions with `list_actions` and `run_action`.

C# (DrawnUi, any head):

```csharp
using DrawnUi.Draw;

// once, after the app's UI exists
for (var i = 1; i <= 20; i++)
{
    var level = i;
    DesignerActions.Add($"Levels/Level {level}", () => Game.StartLevel(level));
}
DesignerActions.Add("Game/Paused", () => Game.Pause());
DesignerActions.Add("Dialogs/Options", () => Game.ShowOptions());
```

- The path names the action and its groups, slash-separated: `Levels/Level 3` shows as **Level 3** under **Levels**.
  Groups and actions keep the order the app added them in.
- The action runs on the app's UI thread. A failure shows in the status bar.
- `Add` returns an `IDisposable` that removes the action again (one a screen offers only while it is shown);
  `DesignerActions.Remove(path)` removes one by its path. The designer's list follows at once.
- Nothing runs without a designer: the list only keeps names and callbacks, and Release builds carry no agent.
- Hot Preview commands (static `[Preview]` methods without parameters that return nothing) show up too, under
  **Previews**, with no change to the app.

Protocol: the app sends `{"type": "actions", "actions": [{"path": "Levels/Level 3"}]}` on connect and whenever the
list changes; the hub sends `{"type": "runAction", "path": "Levels/Level 3"}`. An agent for another engine declares
its actions in that engine's own way and speaks the same two messages.

## Placing controls

Two ways to add a control from the Controls panel:

- **Click it, then click in the app.** The row stays pressed and Properties says "Click in the app where it goes.
  Esc cancels." In the app the pointer boxes the layout the control would go into, with a line where it goes in a
  stack, row or wrap; the click puts it there. The control under the pointer takes it when it holds children: in a
  stack before the first child whose middle is below the pointer, in a row before the first one whose middle is to
  its right, in a wrap the same in reading order, in a layer or grid last. Over a control that holds none (a label,
  a button, an image) the new one goes next to it in its parent: before it when the pointer is in its first half
  (top half in a stack, left half in a row or wrap), else after it (always after in a layer or grid). In DrawFiddle
  the page's own wrapper is not shown: its empty area is the snippet's top control's, by the same rule (in a stack
  by the pointer, else last). Where nothing can take it (a list's cell made from a template, a snippet whose top
  control holds no children) there is no box and nothing is placed. Esc,
  a second click on the row, or Pick cancels; another row places that one instead. Enter on a row does the same as
  a click.
- **Drag it onto Layers.** The rows move apart to open a gap where it will land: between two rows it goes there,
  among the lower row's siblings; over a layout's row that row lights up and the gap opens after what it holds: it
  goes inside, last. Near the top or bottom of Layers the list moves along. Esc cancels and the gap slides shut. Only layouts take children: the app's Controls list says which types do. A list
  that makes its cells from a template takes none (add an item to its ItemsSource instead).
- **Drag it onto the app** (in a page that hosts the editor, such as DrawFiddle). Carried out of the editor, the
  page shows where it lands; let go on the app and it goes where a click there puts it. Back over the editor it is a
  drag onto Layers again.

The new control is selected and shown in Layers. It is in the running app only: its row has the dot of a live change
and two buttons. **Write** (`<>`) adds `new T { .. }` with its starting values to the code that makes the parent's
children, at the same place among them. **Undo** takes it out of the app, and after a write takes the code back
first, byte for byte. Both keep working after a hot reload rebuilds the screen.

The code must list the parent's children one by one (`Children = { .. }`, `new List<SkiaControl> { .. }`,
`.WithChildren(..)`); otherwise the line at the bottom says why nothing was written.

## Move controls

Drag a control's row in Layers with the mouse. The row lifts off (a raised copy follows the pointer, the row itself
fades) and the rows open a gap where it will land, as in Placing controls: between two rows, or into a layout, last.
Not into itself or what it holds, not into a list whose cells come from a template (also while it has no items),
not into, beside or out of anything inside one of its cells (every cell is the same template code), and not into a
page's own wrapper in DrawFiddle. Near the top or bottom of Layers the list moves along; Esc cancels and everything slides back.
On a touch screen a drag on Layers scrolls the list.

From the keyboard, **Alt+Up** and **Alt+Down** move the selected control one place among its siblings.

A moved control moves in the running app only: its row has the dot, Undo and Write, as an added control has
(Properties' header has them too). **Write** moves its code to the same place, among the children the code makes
(the code must list them one by one, in the same file); **Undo** moves it back, and after a write takes the code back
first. Ctrl+Z takes back a move as one step ("Undo Move SkiaLabel"). A hot reload before the write builds the screen
from the code, where the control is still at its old place; the designer moves it again.

## Copy, paste and styles

Right-click a row in Layers (it selects that control), or press the Menu key or Shift+F10 for the selected row. The
menu, and the same keys without it while no text field has the keyboard:

| Item | Key | What it does |
|---|---|---|
| Copy | Ctrl+C | Remembers the control with everything inside it: its type, its Set properties and those of every control it holds, as the app has them. |
| Paste | Ctrl+V | Adds the copy, whole: inside the selected control when it is a layout (last), else right after it. |
| Duplicate | Ctrl+D | Copy and Paste right after the selected control, in one step. |
| Copy style | Ctrl+Shift+C | Remembers only the Set properties that make the look: colors, fonts, strokes, corners, shadows, gradients, padding, opacity, and the alignment (HorizontalOptions, VerticalOptions, text alignment). Never the content (text, sources, items) nor size and place (sizes, margin, grid cells, z order, translations). |
| Paste style | Ctrl+Shift+V | Sets those properties live on the selected control, the ones its type has and with another value. |
| Delete | Del | Takes the selected control out of the app. On macOS Backspace and Cmd+Backspace do too (not in a text field). |

On macOS the keys use Command. What is copied stays in the designer (not on the system clipboard). An item that
cannot act reads disabled: Paste with nothing copied, Paste style with no style, Delete on the root.

Every change starts in the running app only, as a property edit does:

- A pasted control is like a placed one: its Layers row has the dot, Undo and Write. Write adds it whole, laid out
  like the code around it: a short control without children on one line, else one property per line and its
  children inside `Children = { .. }` (the original's own form, such as `new List<SkiaControl> { .. }`, when its
  code uses that), one per line, a level deeper. It gets only the values the original's code sets (and those
  changed live in the designer): values the library sets for itself, such as `UseCache`, stay out.
- Paste style makes ordinary live edits, one per property, each with its dot, Undo and Write; a value the control
  already has is no edit. While a control has more than one, **Write all** in the Properties header writes them all.
- A copy of a control your own function makes (`swatch(CORAL, "coral")` in Rust) is written as that call again.
- A deleted control keeps its row where it was, its name dimmed and "taken out" beside it. Undo puts it back in the
  app at the same place, with what it held; Write takes it out of the code too, and Undo after that puts the
  code back first.

Live changes wait for each other. Each Write is matched with the code as it is, not with the app: a control added and
not written yet is left out, one taken out and not written is still counted where it was. So writes work in any order
(two pasted controls written last one first, a paste next to a deleted control written before the delete). A write
the code cannot take says why: the parent's children come from a loop or a template, its parent is itself not written
yet, it is already in the code. A hot reload builds the screen again from the code, after every Write: the
changes not written yet are put back onto the rebuilt controls (values, added controls at their place, taken-out
ones taken out again), with their dots, Undo and Write. A change whose control or place is no longer in the code
cannot be kept; the line at the bottom says which.

## When changes are written to code

Settings, Code, **Write changes to code**:

- **Manual**: you write them, with a row's Write or Write all.
- **Auto** (the default): they stay live until the code is needed. Restart app (Ctrl+R, the app's restart button)
  and Run app write them all first, then restart or run. In a page, the page asks before it needs the code
  (`code.flush`, see Hosting the editor in a page).
- **Always**: each finished change is written at once: Enter or leaving a field, a color picker or gradient closed,
  a drop, paste, duplicate, move, delete or paste style. Never while you type or drag.

In every mode the title bar shows how many changes are not in code yet, with **Write all**, which writes them all as
one step of the history (in a page this is a strip under the Layers header). What cannot be written stays live with its dot:
an app with no code the designer can reach (a browser page that sends none), a value the code computes, a write the
code refuses.
With Manual, Restart app and Run app ask first: write the changes, go on without them, or cancel. When Auto writes
first and some changes cannot be written, the restart stops and asks: go on without them (they are lost), or cancel;
the question lists why they were not written. Closing the
designer (its close button, Alt+F4, Cmd+Q, File, Exit) with changes not in code goes the same way: Auto and Always
write them first, then the window closes; Manual asks. An app that closes by itself takes its live changes with it.

## Undo

**Ctrl+Z** takes back your last changing action, whatever it was, written to code or not; again, the one before.
**Ctrl+Y** (or Ctrl+Shift+Z) does it again. On macOS they use Command. A write to code is no action of its own: it
belongs to the action it wrote (a row's Write, Write all, Auto before Restart or Run, Always, a page's `code.flush`),
so Ctrl+Z takes the action back in the app and, when it was written, in the code too, and Ctrl+Y does it and writes
it again. The Edit menu and the top of the Layers menu name the step ("Undo Delete
SkiaShape"), and the status line says what went back or came again ("Undone: Paste style").

| Action | One step | Ctrl+Z | Ctrl+Y |
|---|---|---|---|
| A property edit | from opening the value to Enter or leaving it (a color dragged is one step) | the value before, live and in the file when it was written (a value from before an earlier edit not written stays live, with its dot) | the value again, written again when it was |
| Paste style | all its properties | every value before | every value again |
| Placing, Paste, Duplicate | the control added | it leaves the app, and the file when it was written | added again, the same place and content, written again when it was |
| Delete | the control taken out | back in the app, where it was, and in the file | taken out again, of the file too when it was |
| Move (a drag, Alt+Up / Alt+Down) | the control moved | back where it was, in the file too | moved again, in the file too when it was |

Three edits written by one Write all are three steps: Ctrl+Z takes back the last one, in the app and in the file,
and the two others stay as they are.

A row's own Undo button still takes back that row's change; it then leaves the history, so Ctrl+Z never takes it
back twice. A new action clears what Ctrl+Y could do again. The history is kept per app and goes when the app
disconnects. The keys work while no text field has the keyboard, also when the designer is inside a page.

## What Write to code changes

The designer finds the code that creates the selected control. That can be the page's constructor, an object
initializer, a helper method like `Card(...)` or `Heading(...)`, a cell template, or a field assigned with
`.Assign(out _field)`. It then changes one span of text:

| Your code | After writing `FontSize = 20`, `TextColor = #FF0000`, `Padding = 8` |
|---|---|
| `FontSize = 18` | `FontSize = 20` (an `f` or `d` suffix is kept) |
| `Text = "Hi"` | the string, escaped as C# needs |
| `Type = LayoutType.Row` | `Type = LayoutType.Column` |
| `TextColor = Color.Parse("#fff")` | `Color.Parse("#f00")`: your hex format is kept |
| `TextColor = Colors.White` | `Colors.Red` when the color has a name, else `Color.Parse("#RRGGBB")` |
| `Padding = new Thickness(16, 12)` | `new Thickness(8)`: the shortest form, `new(...)` kept if you used it |
| `.WithMargin(8, 4)` | the call's arguments |
| `Heading("Title")` where `Heading` sets `Text = text` | the argument at that call, not the helper |
| nothing set yet | a new `Name = value` in the initializer, or a new line in the constructor |

Everything outside that span stays byte for byte, including the encoding, BOM and line endings.

The designer refuses instead of guessing, and says why, when:

- the value comes from a field, a constant, a binding or an expression (`TextColor = Muted`,
  `Padding = new Thickness(PagePadding)`),
- the control is made in a loop or a LINQ query the designer cannot follow,
- the property type is not one of: number, text, true/false, enum (or a static member such as
  `LayoutOptions.Fill`), color, thickness,
- the file changed on disk while the edit was being made.

A value changes only the control you edited, never another one made by the same code. For a control a helper
returns (`Tag("Two")`), the value goes on that call: the argument it passes for it (`Tag("Three")`), its own
`.WithFontSize(..)` or `.Adapt(..)` when the call has one, else a new `.Adapt(me => me.FontSize = 32)` after it
(with a cast when the helper returns a base type); never into the helper. A control inside a helper that is called
more than once is refused, with why (a template's cells share their code the same way: a value written there
changes every cell). Inside a helper called once, the value is written where it is.

### Rust (DrawnUI for Rust)

The designer follows the function the app builds its window with (`Ui::new(state, build)`, or a DrawFiddle
snippet's `fn build`) through `.children(..)` and `.content(..)`, tuples, `vec![..]` and calls into your own
functions, to the constructor that makes the control. It changes one span of text:

| Your code | After writing `font_size = 20`, `text_color = #FF0000`, `padding = 8,4` |
|---|---|
| `.font_size(18)` | `.font_size(20)`; a decimal (`18.0`) stays a decimal |
| `SkiaLabel::new("Hi")` | the text in the constructor, escaped as Rust needs |
| `.horizontal_options(LayoutOptions::Center)` | `LayoutOptions::Start`, its path kept |
| `.text_color(Color::from_rgb(0xDC, 0x14, 0x3C))` | `Color::from_rgb(0xFF, 0x00, 0x00)`: hex or decimal kept, `from_argb` when not opaque |
| `.text_color(Color::WHITE)` | `Color::RED` when the color has a name, else `Color::from_rgb(..)` |
| `.padding(24)` | `.padding((8, 4))`: the shortest form |
| `circle(Color::BLUE, 40.0)` where `circle` calls `.background_color(colour)` | the argument at that call, not the helper |
| `tag("Two")` where `tag` sets `.font_size(18.0)` itself | `tag("Two").font_size(32)`: on that call only; its own `.font_size(..)` when it has one |
| nothing set yet | `.text_color(..)` at the end of the chain, on its own line when the chain's calls are; for a control a helper returns, on that call (`swatch(..).margin(8)`) |

Added, pasted and duplicated controls are written with the engine's own constructors (`SkiaLabel::new("Label")`)
and their values as builder calls (text values such as an accessibility role as strings: `.accessibility_role("heading")`); a control without children gets `.children(..)`. Moves and deletes edit the
children tuple or `vec![..]`, commas and lines kept; taking out the last child takes out its `.children(..)`. The
designer refuses, and says why, when the children come from a loop, an iterator or a condition, a value is a
constant or an expression, a `vec![..]` would get a control of another type, or a tuple already holds 12 controls
(the most DrawnUI for Rust takes). Controls of your own types are followed when they are made with `Build::new(..)`.

### React (TSX)

Values are attributes of the JSX element (`FontSize={20}`, `HorizontalOptions="Center"`). A component's own element
(what it returns) takes a value as a prop on the use you edited (`<Tag text="Two" FontSize={32} />`) when the
component passes its props on to it (`{...props}`), and an attribute fed by one of its props (`Text={text}`) is
written on that use too. Otherwise, when the component is used more than once, the write is refused, with why
("Tag does not take FontSize"); never into the component's body.

## For AI agents (MCP)

The hub serves MCP (streamable HTTP) at `http://127.0.0.1:54271/mcp`, on this computer only. Register it for
Claude Code once:

```powershell
claude mcp add --transport http --scope user drawnui-designer http://127.0.0.1:54271/mcp
```

The server runs while a designer window or the VS Code panel is open. Without either, an agent starts a designer
with no window:

```powershell
drawnui-designer --mcp
```

It serves the hub and the same MCP until the process is stopped; apps connect to it as to the window, and a window
opened later joins it. When a designer already runs on the port, `--mcp` does not start: it prints that designer's
MCP address (`http://127.0.0.1:54271/mcp`) on stderr and exits with code **3**. The agent then uses the running one,
whose window shows what the agent does.

| Tool | Arguments | What it does |
|---|---|---|
| `list_apps` | none | The connected apps: `id`, name, engine, pid, project. Every other tool takes the `app` id. |
| `run_app` | `project` (default: the most recent app) | Builds and starts a recent app with the agent, waits until it connects (up to 3 minutes). Returns its id, or the build errors. |
| `restart_app` | `app` | Stops the app and runs it again; returns the new id. |
| `allow_devices` | `on` | Lets apps on other devices (a phone, another computer on the network) connect, or stops new ones. Returns the address with its secret token. |
| `list_screens` | `app` | The shell routes and their titles. |
| `open_screen` | `app`, `route` | Opens a screen. Call `get_tree` about a second later. |
| `list_actions` | `app` | The actions the app declared: each `path`, its groups before its name (`Levels/Level 3`). |
| `run_action` | `app`, `path` | Runs an action in the app, on its UI thread. Call `get_tree` or `snapshot` after it. |
| `get_tree` | `app`, `include_internal` (default false) | The controls drawn now: `id`, `parent`, `type`, `madeBy` (the name the code gives it: the helper or component that made it), `field`, `tag`, `rect` [x, y, width, height] in canvas pixels, `visible`, `host` on a root (the page, or `popup 1` for a popup window's content), `templated: true` on a layout that makes its children from items (nothing goes into it), `item` on a cell made from them (the item it shows), and `source` (file:line of the code that creates it). |
| `get_properties` | `app`, `node` | Each property's name, .NET type, current value, and whether it was set. |
| `select` | `app`, `node` (omit to clear) | Draws a box around the control in the app. |
| `pick` | `app`, `x`, `y` | The topmost control at a canvas point. |
| `set_property` | `app`, `node`, `name`, `value` | Sets the value in the running app only. |
| `write_property` | `app`, `node`, `name`, `value` | Sets it in the app and writes it to code. Returns `code.edited` (file, range and an `undo` id) or `code.refused` (the reason and its `key`). |
| `get_controls` | `app` | The control types the app can create: type, group, its own or the library's, container, starting values. |
| `insert` | `app`, `parent`, `index` (omit: last), `type` and `props`, or `copy` | Adds a control to the running app only: a new one of `type`, or a `copy` from `copy`, whole. Returns its id. |
| `copy` | `app`, `node` | The control with everything inside it: type, set properties with their types, children. Pass it to `insert`. |
| `remove` | `app`, `node` | Takes a control out of the running app only. |
| `write_insert` | `app`, `node` | Writes a control added with `insert` into the code, at its place among the children the code makes. |
| `write_remove` | `app`, `node` | Takes a control out of the code that makes it (one removed live, or one still shown). |
| `move` | `app`, `node`, `parent`, `index` | Moves a control in the running app only: into `parent` at `index`, counted without the control (the index it has after the move). |
| `write_move` | `app`, `node` | Moves a control in the code that makes it to where `move` put it. |
| `undo_write` | `undo` | Takes any of these writes back in the file, if that spot was not changed since. The app follows through hot reload. |
| `snapshot` | `app` | A PNG of the app's canvas as drawn now. |
| `focus` | `app` | Brings the app's window back from the taskbar and to the front. |

When a tool fails (the app is gone, refuses a value or does not answer within 5 seconds), the result is marked
as an error and its text says why.

**Example session**

```text
list_apps                       -> [{"id": 8, "name": "DesignerSample", "engine": "dotnet-wpf", "pid": 42760,
                                     "project": "...\\samples\\wpf\\DesignerSample"}]
list_screens {app: 8}           -> [{"route": "home", "title": "Home"}, {"route": "details", "title": "Details"}]
get_tree {app: 8}               -> {"width": 882, "height": 753, "nodes": [
                                     {"id": 5, "parent": 4, "type": "SkiaLabel", "rect": [139, 30, 603, 48],
                                      "field": "DesignerSample.Pages.HomePage._title",
                                      "source": "...\\Pages\\HomePage.cs:32", "visible": true}, ...]}
get_properties {app: 8, node: 5} -> [{"name": "FontSize", "typeName": "System.Double", "value": "28", "isSet": true}, ...]
write_property {app: 8, node: 5, name: "FontSize", value: "32"}
                                -> {"live": "set", "code": {"edited": {"uri": "...\\Pages\\HomePage.cs",
                                     "range": {"start": {"line": 33, "character": 35}, ...}}}}
snapshot {app: 8}               -> the screen as a PNG image
```

A good loop: `get_tree`, then `get_properties` on a control, then `write_property`, then wait two or three seconds
for hot reload, then `snapshot` to check the result.

## How it fits together

```text
your app + agent  --WebSocket-->  hub (127.0.0.1:54270)  <--WebSocket--  designer window / VS Code
                                    |
                                    +-- MCP (127.0.0.1:54271/mcp)  <--  AI agents
```

- Each connection first sends `hello`, saying whether it is an app or an editor.
- An app sends its control tree, its screens, a control's properties on request, and answers to picks and
  snapshots. Each control in the tree says what the designer may change around it: `internal` (a part the
  library made for itself: its properties only), `pageRoot` (the hosting page's wrapper), `templated` (a layout
  that makes its children from items, even with none yet: nothing goes into it) and `templateIndex` (a cell made
  from those items: the item it shows). Each is optional; an app that leaves `templated` out is still known by its
  cells' `templateIndex`.
- An editor sends requests for one app (`toApp`), `setCode` to write a property to code, `insertCode` to write
  a control added live, `removeCode` to take one out of the code, or `undoCode` to take such a write back. The hub answers them itself: `codeEdited`, `codeUndone` or `codeRefused`.
- The hub keeps each app's last tree, screens and properties, and gives them to every editor that connects.
- Writing to code happens in the hub. It reads the app's project files fresh each time (all `.cs`, `.tsx`,
  `.jsx` and `.rs` files under the folder of its `.csproj` or `Cargo.toml`, without `bin`, `obj`, `node_modules`,
  `dist` and `target`) and changes only the one file and span.

Messages are JSON text frames. Their definitions are in `crates/designer-protocol`.

The hub's logic lives in `crates/designer-hubcore`, which does no IO of its own: it takes what apps and editors
say and returns what to send where. It reaches project files through a small `CodeStore` (list, read, write),
and hands parsing and file writes back as work for its host to run. `crates/designer-core` is the native host:
the WebSocket and MCP servers, running apps, phones on a cable, and the files on disk. The same core can run
inside a web page, with the page's code as its files, so the designer works where there is no hub process.

## Hosting the editor in a page

A web page that holds both the app and its code (DrawFiddle) runs the editor in one iframe with the hub
inside it: no hub process, no WebSocket. The page relays the app's messages and owns the code text.

Build it with `dev/build-fiddle-editor.ps1` (needs emsdk, as the VS Code media). It writes `web/fiddle/`:

| File | What |
|---|---|
| `editor.html` | the iframe's page |
| `drawnui-designer.js`, `drawnui_designer.wasm` | the editor and its hub |
| `drawnui_host.js` | the drawnui page host of the version the wasm was built with |
| `designer.js` | starts the editor (`DrawnUiDesigner.start`) |
| `assets/` | fonts, themes, icons |

Every path is relative: copy the folder as is and load `<folder>/editor.html` in the iframe. `editor.html`
calls `DrawnUiDesigner.start({ canvas, designerHub: "page" })`; without `designerHub` the same build connects
to a hub over a WebSocket (`url`), as in VS Code.

Start options in the iframe's address, read before the first frame (`editor.html?writeMode=auto&...`):

| Option | What |
|---|---|
| `theme` | the look: `dark`, `light` or a designer theme id |
| `animateTheme=0` | theme and language changes apply at once, without the opening circle |
| `writeMode` | `manual`, `auto` or `always` (When changes are written to code); without it, Auto |
| `controlsWidth`, `propertiesWidth` | the side panels' widths at start, in CSS pixels; Layers takes the rest. Without them Controls is 150 and Properties 270. A width the person drags stays theirs. |

Every message both ways is posted with `postMessage` and carries `channel: "drawnui-designer"`. The iframe
takes messages only from its parent window.

The iframe sends:

- `{ channel, type: "code.get" }`: send the current code. It is the iframe's first message, sent before the wasm
  loads; the page can hold its own messages until it arrives. The iframe keeps what arrives before its hub is
  ready.
- `{ channel, type: "toApp", msg }`: a `HubToApp` message for the app's agent, passed on unchanged.
- `{ channel, type: "code.edit", version, edits: [{ start, end, text }] }`: write to the code. The offsets are
  UTF-16 code units of the text at `version` (as a JS string or Monaco counts). Apply all edits as one undo
  step. Write to code and its Undo both send it.
- `{ channel, type: "history", canUndo, canRedo, undo, redo }`: what the page's Undo and Redo would do now, sent
  once when the editor is up (nothing to undo: `false`, `false`, `""`, `""`) and again whenever it changes. `undo`
  and `redo` name the step as the Edit menu does ("Change FontFamily", "Delete SkiaShape"), for a tooltip.
- `{ channel, type: "code.flushed", request, written, left }`: the answer to `code.flush`, once its writes are done
  (their `code.edit` messages sent first): how many changes were written and how many are still not in code (they
  cannot be written, or the mode is Manual). It comes within 2.5 s: a write the page has not taken by then counts in
  `left`, and still lands when the page takes it.
- `{ channel, type: "drag.out", typeName, x, y, localX, localY, phase }`: a control dragged from Controls is outside
  the iframe, the button still held (the iframe keeps the pointer). `x`, `y` are in the page's client coordinates
  (`frame.offset` added); `localX`, `localY` are the same point in the iframe's own CSS pixels, so the page can add
  where the iframe is at that moment (`iframe.getBoundingClientRect()`) and need not keep `frame.offset` current.
  `phase` is `"move"` while it is carried, `"drop"` when it is let go outside the iframe,
  `"cancel"` on Esc or when it comes back into the iframe (then it is a drag onto Layers again). A drop outside the
  app's canvas places nothing: the page treats it as a cancel.

The page sends:

- `{ channel, type: "fromApp", msg }`: the agent's messages; the first is its hello (`{ role: "app", ... }`).
  A hello from the same app again (same name, engine and platform: its agent attached again after a
  rebuild) keeps it, with the changes not in code yet, as a hot reload does; another app's hello replaces it.
- `{ channel, type: "app.closed" }`: the page let its app go (its code view is shown, say). The editing session
  stays: the history (with its writes to code), the changes not in code and the selection, for the same app's next
  hello, whose new controls are found again by their place. A different app's hello ends the session.
- `{ channel, type: "theme", theme }`: the page's look. `"dark"`, `"light"` or a designer theme id (`midnight`,
  `graphite`, `paper`, `high-contrast`). Send it once at start and again whenever the page's theme changes.
- `{ channel, type: "code", uri: "fiddle:///snippet.cs", text, version }`: the code (`snippet.tsx` for React,
  `snippet.rs` for Rust). The answer to `code.get`, and sent again on every change in the page's editor, with a new
  version.
- `{ channel, type: "code.flush", request }`: the page needs the code (before Source, Run, Share or Export). With
  Auto or Always the editor writes every change not in code yet, as one step of its history, then answers
  `code.flushed` with the same `request`; with Manual it answers at once and writes nothing.
- `{ channel, type: "undo" }`, `{ channel, type: "redo" }`: the page's Undo and Redo while the designer is shown:
  exactly Ctrl+Z / Ctrl+Y in the editor, the last changing action back in the app and in the code (see Undo). The
  page's own code undo (its editor's) stays for its source view.
- `{ channel, type: "frame.offset", x, y }`: where the iframe sits in the page (CSS pixels, the left and top of its
  client box). Send it once at start and again whenever the iframe moves: on every resize or scroll, and when the
  page's layout first gives the iframe its place (it may still be 0 by 0 at the iframe's first message).
- `{ channel, type: "place", typeName, x, y }`: a `drag.out` dropped on the app's canvas, at that canvas point in
  the app's pixels. The editor sends the app `placeAt`: the control goes where a place-mode click at that point puts
  it, and it is recorded, undone and written as a placed control.

The hub treats the app's code as one file, `fiddle:///snippet.cs` (the app's hello has no project): the
snippet's last top-level `return` is what the Fiddle's own layout holds (for Rust, `fn build`). After a `code.edit` the iframe uses
the new text at once; the next `code` message with a newer version is the truth either way (one at the
version the edit was made against, or older, is the text from before the edit and waits for the edit). A write that
comes before that message (Write all, `code.flush`) waits for it, then is made against that text: the writes land one
by one, in order, each on the text the page sent after the one before, also when the person typed in between.

`dev/fiddle-probe/index.html` is a stand-in for such a page (a fake agent, its preview as the app's canvas, and a
textarea): serve the repo root, then open `/dev/fiddle-probe/`. Drag a control from Controls onto the preview to
see `drag.out`, `frame.offset` and `place` at work.

## Limits

- Apps: DrawnUi.Net WPF and DrawnUi.Maui (Windows, Android, iPhone, iPad, Mac), DrawnUI for Rust on Windows (live
  edits, Pick, snapshots, Bring to front, Write to code into `.rs`) and in DrawFiddle (Write to code into the snippet),
  DrawnUI for React in the browser (live edits, Pick, add / copy / delete, snapshots, Write to code into `.tsx`).
- Controls can be added, pasted, duplicated, moved and deleted (Placing controls; Move controls; Copy, paste and
  styles).
- Gradients and shadows are edited as text.
- `snapshot` of a hidden or minimized app can show GPU-cached controls as blank.
- Only UTF-8 source files are edited. Files in UTF-16 are never written.
