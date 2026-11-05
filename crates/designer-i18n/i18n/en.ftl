# DrawnUI Designer, English: the source of every other language. One message per text the window shows or
# a screen reader says. Names in { $braces } are filled in by the designer. Keep every key in every language.

## Window

app-title = DrawnUI Designer
apps = Apps
help = Help
settings = Settings
back-to-designer = { $page }, back to the designer

## Panels

panel-screens = Screens
panel-layers = Layers
panel-properties = Properties
resize-screens = Resize Screens
resize-properties = Resize Properties
screen-open = Open { $title }
controls-count = { $count ->
    [one] { $count } control
   *[other] { $count } controls
}
empty-screens-title = No screens
empty-screens-hint = The app reports its shell routes here.
empty-app-title = No app connected
empty-app-hint = Start a DrawnUI app with the designer agent, or run the last one.
empty-selection-title = Nothing selected
empty-selection-hint = Select a control in Layers to see and edit its properties.

## Title bar apps

app-building = building…
app-starting = starting…
app-failed = failed
app-restart = Restart { $name }
app-run = Run { $name }
app-starting-name = Starting { $name }…
app-shown = { $name }, { $detail }, shown
app-show = Show { $name }, { $detail }
app-run-detail = Run { $name }, { $detail }

## Layers

pick = Pick
pick-active = Click or tap in the app
pick-label = Pick a control in the app
fold = Fold { $name }
unfold = Unfold { $name }
row-level = level { $level }
row-folded = folded
row-open = open
row-selected = selected

## Properties

order = Properties order
order-set = Set
order-groups = Type
group-changed = Set
group-others = Unset
group-layout = Layout
group-text = Text
group-look = Look
group-transform = Transform
group-behavior = Behavior
group-row = { $group }, { $count ->
    [one] { $count } property
   *[other] { $count } properties
}, { $state }
group-open = open
group-folded = folded
prop-changed = changed in the app
prop-set = set
value = Value
decrease = Decrease
increase = Increase
undo-prop = Undo { $name }
write-prop = Write { $name } to code

## Status line

status-connected = { $count ->
    [one] Hub connected · { $count } app
   *[other] Hub connected · { $count } apps
}
status-connecting = Connecting to the hub…
status-run-failed = { $name } failed: { $message }
status-live = { $prop } = { $value } in the app, not in code yet
status-writing = Writing { $prop } to code…
status-cancelled = { $prop }: edit cancelled
status-written = Written to { $file }, line { $line }
status-not-written = Not written to code: { $reason }
status-not-undone = Not taken back in code: { $reason }
status-undoing = Taking { $prop } back in code…
status-undone-code = { $prop } back to { $value } in { $file }
status-undone-app = { $prop } back to { $value } in the app

## Settings

settings-sections = Settings sections
section-appearance = Appearance
section-layers = Layers
section-connection = Connection
section-about = About
appearance-hint = How the designer looks.
theme = Theme
theme-hint = The colors of the whole window. The new one opens from where you click.
theme-dark = Dark
theme-midnight = Midnight
theme-graphite = Graphite
theme-light = Light
theme-paper = Paper
theme-high-contrast = High contrast
language = Language
language-hint = The language of the whole window.
language-system = System ({ $language })
layers-hint = The control tree of the screen the app shows.
library-parts = Parts the library builds
library-parts-hint = Also list the controls DrawnUI makes for itself, such as a button's inner label. No code of yours creates them, so their values cannot be written to code.
connection-hint = Where apps and AI agents reach the designer. Running apps connect by themselves.
connection-apps = Apps
connection-mcp = MCP server for AI agents
connection-claude = Add it to Claude Code
copy = Copy
copied = Copied

## About

about-logo = DrawnUI logo
about-version = Version { $version }
about-description = See the screens, controls and properties of a running DrawnUI app, change them live and write each change back into its code.
made-with = Made with
uses = Uses
credit-drawnui-rust = The drawn UI engine on Skia: one code for the desktop, phones and the web.
credit-nx = Its language protocol: UI source read as a tree of nodes and edited node by node.
credit-tree-sitter = Reads and edits the app's C# code, leaving every byte it does not change as it was.
credit-mcp = The Model Context Protocol server that lets AI agents see and change a running app.
credit-lucide = The icons.

## Help

help-title = Using the designer
help-intro = See a running DrawnUI app's screens, controls and properties, change them live, and write each change into the app's C# code.
help-connect = Connect an app
help-connect-1 = Run your app with the designer agent: it connects by itself and shows in the title bar.
help-connect-2 = An app that ran before stays there too: click it to run it again. The round arrow of a running app restarts it.
help-connect-3 = With several apps connected, click one to show it here; on a computer its window comes to the front.
help-phones = Phones
help-phones-1 = An iPhone or iPad on the USB cable connects by itself: run a Debug build of your app on it with the designer agent. No Wi-Fi needed.
help-phones-2 = Pick, then tap a control on the phone, to select it here. Press Pick again to stop.
help-phones-3 = Without a cable: Settings, Connection, Apps on other devices gives the address to build the app with.
help-screens = Screens
help-screens-1 = The pages the app's shell can open. Click one to open it in the app.
help-layers = Layers
help-layers-1 = The controls of the screen the app shows, as a tree. Click one to select it: a box shows around it in the app.
help-layers-2 = Double-click a row, or click its box on the tree line, to fold or unfold it.
help-layers-3 = Pick, then click a control in the app, or tap it on a phone, to select it here.
help-properties = Properties
help-properties-1 = Click a value to change it: type it, use - and + for numbers, choose from a list, or pick a color.
help-properties-2 = Enter sets it in the running app. Esc puts it back as it was.
help-properties-3 = Changed properties are written brighter. Set lists them first (Set, then Unset), Type by kind.
help-write = Write to code
help-write-1 = The code button of a row writes the value into the C# file that creates the control: only that value changes, the rest of the file stays as it was.
help-write-2 = Undo takes back the last change of a row: in the app, and in the file after a write.
help-write-3 = The line at the bottom says which file changed, or why nothing was written.
help-write-4 = Ctrl+Z takes back your last action, whatever it was: an edit, a write, a paste, a delete. Ctrl+Y does it again. The Edit menu names both.
help-keyboard = Keyboard
help-keyboard-1 = Tab moves between groups, the arrow keys move inside a list or a menu, Enter or Space acts.
help-keyboard-2 = Esc closes a menu, leaves an edit, and goes back from Settings or Help.
help-keyboard-3 = The menu bar: click a title, or press Alt with its underlined letter. Its items show their keys, which work with the menus closed too.
help-agents = AI agents
help-agents-1 = Agents use the same features through MCP at { $address }.
help-agents-2 = Settings, Connection has the address and the command that adds it to Claude Code.

## Color picker

picker = Color
picker-plane = Saturation and brightness
picker-plane-value = saturation { $saturation }%, brightness { $brightness }%
picker-hue = Hue
picker-opacity = Opacity
picker-hex = Hex color
picker-old-new = Before: { $old }, now on the right
picker-keys = Arrows move the selected bar or square (Shift: ten steps). Esc puts the old color back.

## Hub: why a value was not written to code, why an app did not run (AI agents read the English)

src-not-number = `{ $text }` is not a number
src-not-bool = `{ $text }` is not true or false
src-not-color = `{ $text }` is not a #RGB, #RRGGBB or #AARRGGBB color
src-not-thickness = `{ $text }` is not a thickness ("all", "h,v" or "l,t,r,b")
src-type-unsupported = { $type } values cannot be written to code yet
src-not-in-tree = the control is not in the app's last tree
src-library-part = the library made this control for its own parts: no code declares it
src-no-code = no code found that creates this control
src-arg-no-value = { $prop } comes from argument `{ $param }` of `{ $call }`, which has no value here
src-several-args = { $prop } is fed by several arguments of `{ $call }`
src-key-missing = `{ $key }` is not in the code
src-no-value = the property has no value
src-not-literal = `{ $key }` is not a literal the designer can change (an expression or a call)
src-value-misfit = the value does not fit `{ $key }` (another kind of literal)
src-not-single = the edit is not a single change
src-empty-parens = empty parentheses
src-rewrites-colors = { $prop } is set by `{ $code }`; the designer only rewrites Color.Parse("#..") and Colors.X
src-rewrites-thickness-numbers = { $prop } is set by `{ $code }`; the designer only rewrites Thickness arguments that are numbers
src-rewrites-new-thickness = { $prop } is set by `{ $code }`; the designer only rewrites new Thickness(..) with number arguments
src-no-arguments = no arguments
src-with-not-numbers = With{ $prop }(..) has arguments that are not numbers
src-with-misfit = With{ $prop }(..) does not take this value
src-insert-no-list = the code that makes this control's children is not a list the designer can add to (a `Children` list, a `new List<SkiaControl>` or `.WithChildren(..)`)
src-insert-no-initializer = `{ $key }` is created without an object initializer, so the designer cannot give it children
src-insert-templated = this control makes its children from a template: add an item to its ItemsSource instead
src-insert-unmapped = this control's children do not line up one to one with its code (a loop or a helper makes several), so the designer cannot tell where the new control goes
src-insert-type-unknown = the type of { $prop } is unknown, so its starting value cannot be written
src-remove-not-in-list = `{ $key }` is not an item of a list of children, so the designer cannot remove it from the code
src-move-not-in-list = `{ $key }` is not an item of a list of children, so the designer cannot move it in the code
src-move-into-itself = a control cannot be moved into itself or into one of its own children
src-move-other-file = the new parent is made in another file; the designer moves a control only inside one file
hub-read-failed = could not read { $file }: { $error }
hub-changed-since = { $file } changed there since it was written
hub-write-failed = could not write { $file }: { $error }
hub-nothing-written = nothing written by that request
hub-not-connected = the app is not connected
hub-no-project = the app did not say which project it was built from
hub-no-tree = the app has not sent its tree yet
hub-type-unknown = the type of { $prop } is unknown: the app has not sent it among this control's properties
hub-no-sources = no C# files under { $folder }
hub-changed-during = { $file } changed while the edit was made
hub-failed = the hub failed: { $error }
hub-no-app = no app with id { $app } is connected (see list_apps)
run-no-way = no way to run a { $engine } app yet
run-agent-unknown = the app's agent path is unknown: start it once with the agent so the designer learns it
run-already = { $project } is running already (app { $app }): restart it instead
run-not-recent = { $project } is not a recent app (see list_apps)
run-cannot-start = could not start { $program }: { $error }
run-build-failed = the build failed
run-no-connect = no app connected within { $seconds } s
run-ended-code = the process ended with code { $code } before the app connected
run-ended = the process ended before the app connected
connection-devices = Apps on other devices
connection-devices-hint = Lets an app on a phone or another computer on this network connect, with the address below. The address holds a secret key: anyone who has it can connect.
connection-devices-address = Address for apps on other devices
status-devices-failed = Apps on other devices cannot connect: { $reason }
hub-no-network = this computer is not on a network
hub-port-taken = port { $port } is taken: { $error }
app-not-running = not running
status-wifi-on = Wi-Fi on
status-wifi-off = Wi-Fi off
status-usb = USB: { $phones }
apps-not-running = Recent
status-hot-applied = Code update applied in the app
status-hot-reloaded = Hot reload: the screen was rebuilt
gradient-summary = { $count ->
    [one] { $type } · { $count } color
   *[other] { $type } · { $count } colors
}
gradient-none = none
gradient-editor = Gradient
gradient-type = Type
gradient-bar = Colors of the gradient
gradient-bar-hint = Click the bar to add a color, drag a handle to move it, Delete removes the selected one.
gradient-stop-value = Color { $index } of { $count } at { $position }%
gradient-angle = Angle
gradient-angle-less = Turn left 15°
gradient-angle-more = Turn right 15°
row-item = item { $index }
status-live-cell = { $prop } = { $value } on this cell only (item { $index }). Write to code changes every item
empty-tree-title = No controls yet
empty-tree-hint = Controls show here as soon as the app shows them.
empty-tree-hint-page = Write code in Source and its controls show here.
choice-empty = (empty)
choice-empty-label = Empty, clears the value

## Controls panel

panel-controls = Controls
resize-controls = Resize Screens and Controls
controls-search = Search controls
control-row-hint = Click, then click in the app where it goes. Or drag it onto Layers.
empty-controls-title = No controls
empty-controls-hint = Connect an app: the controls it can create show here.
empty-controls-wait = The app has not listed the controls it can create.
empty-controls-match-title = No match
empty-controls-match-hint = No control has this in its name.
group-media = Media
group-input = Input
group-shapes = Shapes
group-other = Other
group-own = This app
place-title = Placing { $type }
place-hint = Click in the app where it goes. Esc cancels.
status-inserted = { $type } added in the app, not in code yet
status-not-inserted = { $type } was not added
status-app-refused = The app refused: { $reason }
status-removed = { $type } taken out of the app
status-removed-code = Taken out of { $file } and of the app
row-added = added here, not in code yet
row-added-written = added here, in code
help-controls = Add a control
help-controls-1 = Controls, under Screens, lists what your app can create: the library's controls by kind, your own classes under This app. Type in its search field to find one.
help-controls-2 = Click a control, then click in the app where it goes; Esc cancels. Or drag it onto Layers: between two rows, or onto a layout to put it inside.
help-controls-3 = A new control is in the app only, with a dot on its Layers row: Write puts it into the code, Undo takes it out.
selection-color = Selection color
selection-color-hint = The box around the selected control in your app.
animate-changes = Animate theme and language changes
animate-changes-hint = A new theme or language opens over the window in a circle. Off: it applies at once.
color-magenta = Magenta
color-lime = Lime
color-cyan = Cyan
color-orange = Orange
color-white = White
selection-color-other = Other color

## Layers menu

layers-menu = Control actions
menu-copy = Copy
menu-paste = Paste
menu-duplicate = Duplicate
menu-copy-style = Copy style
menu-paste-style = Paste style
menu-delete = Delete
menubar = Menu bar
menubar-file = &File
menubar-edit = &Edit
menubar-view = &View
menubar-help = &Help
menu-run-app = Run app
menu-restart-app = Restart app
menu-settings = Settings
menu-exit = Exit
menu-undo = Undo
menu-redo = Redo
menu-undo-step = Undo { $step }
menu-redo-step = Redo { $step }
menu-props-order = Properties order
menu-library-parts = Show library parts
menu-about = About
step-edit = Change { $prop }
step-paste-style = Paste style
step-write = Write { $name }
step-write-all = Write all
step-add = Add { $type }
step-paste = Paste { $type }
step-duplicate = Duplicate { $type }
step-delete = Delete { $type }
step-write-delete = Delete { $type } from code
status-undone = Undone: { $step }
status-redone = Redone: { $step }
write-all = Write all
write-all-label = Write all { $count } changes to code
status-copied = { $type } copied with { $count ->
    [one] { $count } property
   *[other] { $count } properties
}
status-style-copied = Style of { $type } copied: { $count ->
    [one] { $count } property
   *[other] { $count } properties
}
status-style-pasted = { $count ->
    [one] { $count } style property set in the app, not in code yet
   *[other] { $count } style properties set in the app, not in code yet
}
status-style-none = None of the copied style fits { $type }
status-removed-live = { $type } taken out of the app, not in code yet
status-removed-written = { $type } taken out of the app and of the code
status-restored = { $type } is back in the app
row-removed = taken out of the app, not in code yet
row-removed-written = taken out of the app and of the code
row-removed-tag = taken out
help-copy = Copy, paste and styles
help-copy-1 = Right-click a control in Layers, or press the Menu key or Shift+F10, for Copy (Ctrl+C), Paste (Ctrl+V), Duplicate (Ctrl+D), Copy style (Ctrl+Shift+C), Paste style (Ctrl+Shift+V) and Delete (Del).
help-copy-2 = Paste puts the copy inside the selected layout, or right after the selected control. Paste style sets the copied colors, fonts, strokes, corners, shadows and padding on the selected control; Write all puts them in the code.
help-copy-3 = A pasted or deleted control changes the app only, with a dot on its Layers row: Write changes the code too, Undo takes it back.

## Hub: writes while other live changes wait

hub-not-in-code-yet = { $type } is not in the code yet: write it to code first
hub-parent-not-in-code = its parent { $type } is not in the code yet: write that one first
hub-in-code-already = { $type } is in the code already

## Copy with what is inside

status-copied-inside = { $type } copied with { $count ->
    [one] { $count } control inside
   *[other] { $count } controls inside
}
status-copy-gone = Not copied: the app no longer shows that control

## Live changes kept across a rebuild

hub-lost-prop = { $type } { $prop }: its control is not in the code any more
hub-lost-insert = { $type } added here: its place is not in the code any more
hub-lost-remove = { $type } taken out here: it is not in the code any more
status-not-kept = { $count ->
    [one] 1 change could not be kept: { $what }
   *[other] { $count } changes could not be kept: { $what }
}
status-moved = { $type } moved in the app, not in code yet
status-not-moved = { $type } was not moved
step-move = Move { $type }
row-moved = moved here, not in code yet
row-moved-written = moved here, in code
hub-lost-move = { $type } moved here: its place is not in the code any more
help-move = Move controls
help-move-1 = Drag a control's row in Layers: the rows open a gap where it will land. Between two rows it goes there; onto a layout it goes inside, last. Near the top or bottom the list moves along; Esc cancels.
help-move-2 = Alt+Up and Alt+Down move the selected control one place among its siblings.
help-move-3 = A moved control changes the app only, with a dot on its Layers row: Write moves it in the code too, Undo puts it back.
