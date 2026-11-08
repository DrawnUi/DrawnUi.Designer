# DrawnUI Designer

Inspect and edit running DrawnUI apps from VS Code.

- **DrawnUI Designer** in the status bar (or **DrawnUI: Open Designer** in the Command Palette) starts the designer hub
  and opens the designer beside your editor.
- Running DrawnUI apps that load the designer agent appear at the top: pick one to see its screens, its live
  control tree and the selected control's properties.
- Selecting a control highlights it in the app; tapping a screen opens it there; an edited property changes the
  live control at once and can be written back to your code (C#, TSX or Rust).
- The panel has the designer window's menu bar, keys and Settings, and shares its settings. While it is the active
  tab, Ctrl+P (Pick), Ctrl+R (Restart app), Ctrl+, (Settings), F1 (Help) and Alt+F / Alt+E (menus) are the
  designer's; the Command Palette is Ctrl+Shift+P.
- Closing the tab with changes not in code yet keeps them live in the app and says how many, with **Reopen**.

Settings: `drawnuiDesigner.port` (hub port, default 54270), `drawnuiDesigner.sidecarPath` (a hub executable of your
own instead of the bundled one).

Windows (x64, ARM64) and macOS (Apple silicon, Intel). More: https://github.com/DrawnUi/DrawnUi.Designer
