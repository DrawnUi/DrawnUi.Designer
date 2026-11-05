# DrawnUi.Designer.Agent

Lets the DrawnUI Designer attach to a running DrawnUi app: it shows the app's screens, control tree and properties,
edits them live and writes edits back into the app's C# code.

- **Debug builds only.** Release builds get nothing: no reference, no startup hook, no DLL in the output.
- **Nothing to set up.** DrawnUi packages that depend on it bring it along; referencing this package directly works too.
- **Idle until a designer connects.** It only tries to reach the designer at one address, with a growing pause, and
  does no work in the app until then. One console line at start says where it waits.
- **Turn it off** with the MSBuild property `DrawnUiDesigner=false`.

How it attaches:

| App | How |
|---|---|
| Windows (WPF, MAUI) | startup hook in the app's `runtimeconfig.json`; connects to `ws://127.0.0.1:54270` |
| Android | built into the app; connects to `ws://127.0.0.1:54270`, which the designer forwards over the USB cable |
| iPhone, iPad | built into the app; waits on the USB cable (the phone's `127.0.0.1`, port 54273 to 54282) |
| Mac Catalyst | built into the app; connects to `ws://127.0.0.1:54270`, the designer on the same Mac |

`DrawnUiDesignerUrl` (MSBuild property) points any of them at another designer address, passed unchanged.
