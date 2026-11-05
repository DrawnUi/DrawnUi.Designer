# MAUI Tutorials on Android

The `firstappc` screen of the DrawnUI MAUI Tutorials sample, recorded from an Android phone over adb:
`firstappc.json` is its control tree (the hub's get_tree output in protocol `Tree` form, type names prefixed with `DrawnUi.Draw.`), and `FirstAppPageCode.cs` is the page that
builds it. The page is a MAUI `ContentPage`: the tree holds only the canvas content, which the page
builds in `Build()` through `new Canvas { Content = CreateMainLayout() }`.

- Recorded: 2026-11-01, DrawnUi commit `a2cf1901`
- Used by the matcher tests in `crates/designer-sources`.
