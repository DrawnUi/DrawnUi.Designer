# DrawnUI Designer

Inspect and edit running DrawnUI apps from VS Code.

- **DrawnUI: Open Designer** (Command Palette) starts the designer hub and opens the designer beside your editor.
- Running DrawnUI apps that load the designer agent appear at the top: pick one to see its screens, its live
  control tree and the selected control's properties.
- Selecting a control highlights it in the app; tapping a screen opens it there; an edited property changes the
  live control at once and can be written back to your C# code.

Settings: `drawnuiDesigner.port` (hub port, default 54270), `drawnuiDesigner.sidecarPath` (a hub executable of your
own instead of the bundled one).
