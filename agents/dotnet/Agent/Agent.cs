using System.Collections;
using System.Globalization;
using System.IO;
using Expression = System.Linq.Expressions.Expression;
using System.Net;
using System.Net.Sockets;
using System.Net.WebSockets;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

/// <summary>
/// .NET startup hook entry: <c>DOTNET_STARTUP_HOOKS=&lt;path&gt;\DrawnUi.Designer.Agent.dll</c>.
/// The runtime calls <see cref="Initialize"/> before the app's Main.
/// </summary>
internal class StartupHook
{
    /// <summary>Starts the agent on a background thread; never blocks or throws into the app.</summary>
    public static void Initialize() => DrawnUi.Designer.Agent.AgentHost.Start();
}

namespace DrawnUi.Designer.Agent
{
    /// <summary>
    /// Build-time entry, for platforms without startup hooks (iOS, Mac Catalyst, Android): DrawnUi.Designer.targets
    /// references the agent and generates a module initializer in the app that calls <see cref="Start"/>.
    /// </summary>
    public static class Attach
    {
        /// <summary>Starts the agent on a background thread; never blocks or throws into the app.</summary>
        /// <param name="url">The hub, taken unchanged (query included); empty: DRAWNUI_DESIGNER_URL, then the default.</param>
        /// <param name="project">The app's project folder on the build machine; empty: read from the PDB.</param>
        public static void Start(string url, string project) => Start(url, project, false);

        /// <param name="cable">iPhone over the USB cable: the agent listens on the phone's 127.0.0.1 and the designer
        /// connects through usbmux; <paramref name="url"/> is not used.</param>
        /// <summary>
        /// A DrawnUi app in a web page (Blazor WebAssembly, DrawFiddle): no socket, the page carries the messages.
        /// The agent sends its hello and first tree through <paramref name="send"/> at once and returns the function
        /// that takes each message the designer sends (text, one at a time; it may be called before the hello has gone,
        /// it waits for it). Nothing ever blocks the page's thread. Call again for a new canvas: the old one is
        /// released. When the page rebuilds the controls inside the same canvas, it passes {"type":"rebuilt"} to the
        /// returned function: a new tree goes out. Call on the page's thread.
        /// </summary>
        /// <param name="canvas">The page's DrawnUi canvas (DrawnUi.Views.Canvas).</param>
        /// <param name="send">Takes each message of the agent, as JSON text.</param>
        public static Func<string, Task> AttachPage(object canvas, Func<string, Task> send) => AgentHost.AttachPage(canvas, send);

        public static void Start(string url, string project, bool cable)
        {
            AgentHost.BakedUrl = string.IsNullOrWhiteSpace(url) ? null : url;
            AgentHost.BakedProject = string.IsNullOrWhiteSpace(project) ? null : project;
            AgentHost.Cable = cable;
            AgentHost.Start();
        }
    }

    /// <summary>
    /// In-app agent of DrawnUI Designer for DrawnUi.Net WPF and DrawnUi.Maui apps (Windows, iOS, Mac Catalyst, Android). Finds the app's canvas,
    /// connects to the hub over a WebSocket and answers its requests (tree, pick, highlight, set property, open screen).
    /// Binds to the app's own DrawnUi and UI framework by reflection (Hosts.cs). Every control access runs on the UI
    /// thread, the one the canvas draws on.
    /// </summary>
    internal static partial class AgentHost
    {
        private const string Protocol = "0.1.0";
        private const BindingFlags Inst = BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly;

        private static IHost _host;

        // the canvas the agent works on, held weakly: a canvas (and its page) the app has left must be free to go
        private static WeakReference<object> _canvasRef;

        private static object _canvas
        {
            get => _canvasRef != null && _canvasRef.TryGetTarget(out var canvas) ? canvas : null;
            set => _canvasRef = value == null ? null : new WeakReference<object>(value);
        }
        private static WebSocket _ws;

        // where the session's messages go: the designer's WebSocket, or the page's channel; null when none is connected
        private static Func<string, Task> _out;
        private static readonly SemaphoreSlim SendLock = new(1, 1);
        private static long _revision;

        // stable ids: one per control instance while it lives
        private static readonly ConditionalWeakTable<object, StrongBox<long>> Ids = new();
        private static readonly Dictionary<long, WeakReference<object>> ById = new();
        private static long _nextId;

        private static int _started;

        // set by Attach (build-time attach), null for the startup hook
        internal static string BakedUrl, BakedProject;
        internal static bool Cable;

        public static void Start()
        {
            // The hook can be listed twice (DOTNET_STARTUP_HOOKS and the app's runtimeconfig after a build with
            // DrawnUi.Designer.targets): the runtime then calls Initialize twice. One agent per process.
            if (OperatingSystem.IsBrowser())
                return; // a page attaches through AttachPage: no socket from a browser
            if (Interlocked.Exchange(ref _started, 1) == 1)
                return;
            // Until a designer answers the agent only tries to connect, on a pool thread with a growing pause: no UI
            // thread work, no subscriptions, no thread of its own.
            Task.Run(async () =>
            {
                try
                {
                    await ConnectLoop();
                }
                catch
                {
                    // the agent must never take the app down
                }
            });
        }

        #region Finding the canvas

        /// <summary>After connecting: waits for the app's UI framework and canvas (the app may still be starting).
        /// False when the hub left meanwhile.</summary>
        private static async Task<bool> FindCanvas(WebSocket ws)
        {
            while (ws.State == WebSocketState.Open)
            {
                try
                {
                    _host ??= MauiHost.TryCreate() ?? WpfHost.TryCreate();
                    if (_host != null)
                    {
                        var found = Ui(() => _host.FindCanvas());
                        if (await Task.WhenAny(found, Task.Delay(10000)) == found && found.Result is { } canvas)
                        {
                            _canvas = canvas;
                            return true;
                        }
                    }
                }
                catch
                {
                }
                await Task.Delay(500);
            }
            return false;
        }

        /// <summary>Runs on the UI thread and hands back the result (or the exception).</summary>
        private static Task<T> Ui<T>(Func<T> work)
        {
            var done = new TaskCompletionSource<T>(TaskCreationOptions.RunContinuationsAsynchronously);
            _host.Post(() =>
            {
                try
                {
                    done.SetResult(work());
                }
                catch (Exception e)
                {
                    done.SetException(e);
                }
            });
            return done.Task;
        }

        private static Task Ui(Action work) => Ui(() =>
        {
            work();
            return true;
        });

        /// <summary>
        /// The canvas to work on now: MAUI shows a new page (and canvas) after navigation. A change of canvas ends the
        /// pick mode and the highlight, which belong to the old one. UI thread.
        /// </summary>
        private static void RefreshCanvas()
        {
            WatchNavigation();
            var canvas = _host.FindCanvas();
            if (canvas == null || ReferenceEquals(canvas, _canvas))
                return;
            SetPickMode(false);
            SetHighlight(null);
            _canvas = canvas;
        }

        private static bool IsType(Type t, string fullName)
        {
            for (; t != null; t = t.BaseType)
                if (t.FullName == fullName)
                    return true;
            return false;
        }

        #endregion

        #region Project

        private static string _project;

        /// <summary>
        /// The folder holding the app's .csproj, from the source paths in the entry assembly's portable PDB
        /// (embedded or next to the dll): the nearest .csproj above each source file, preferring the one named
        /// like the assembly (shared source files may live in other folders). Null without a PDB or when the
        /// paths are not on this machine (a CI build with mapped paths).
        /// </summary>
        /// <summary>The app's own assembly: the entry assembly, or, where the platform starts the app without a Main
        /// of its own (Android), the assembly of the app's Application class.</summary>
        private static Assembly AppAssembly() =>
            Assembly.GetEntryAssembly()
            ?? (StaticGet(LoadedType("Microsoft.Maui.Controls.Application"), "Current")
                ?? StaticGet(LoadedType("System.Windows.Application"), "Current"))?.GetType().Assembly;

        private static string FindProject()
        {
            try
            {
                var assembly = AppAssembly();
                if (assembly == null || string.IsNullOrEmpty(assembly.Location))
                    return null;
                using var pe = new System.Reflection.PortableExecutable.PEReader(File.OpenRead(assembly.Location));
                if (!pe.TryOpenAssociatedPortablePdb(assembly.Location, path => File.Exists(path) ? File.OpenRead(path) : null,
                        out var provider, out _) || provider == null)
                    return null;
                using (provider)
                {
                    var pdb = provider.GetMetadataReader();
                    var counts = new Dictionary<string, int>(StringComparer.OrdinalIgnoreCase);
                    var dirs = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase); // source dir -> csproj
                    foreach (var handle in pdb.Documents)
                    {
                        var file = pdb.GetString(pdb.GetDocument(handle).Name);
                        if (!file.EndsWith(".cs", StringComparison.OrdinalIgnoreCase) || !Path.IsPathRooted(file) || !File.Exists(file))
                            continue;
                        var dir = Path.GetDirectoryName(file);
                        if (!dirs.TryGetValue(dir, out var csproj))
                        {
                            for (var d = new DirectoryInfo(dir); d != null && csproj == null; d = d.Parent)
                                csproj = d.EnumerateFiles("*.csproj").FirstOrDefault()?.FullName;
                            dirs[dir] = csproj;
                        }
                        if (csproj != null)
                            counts[csproj] = counts.GetValueOrDefault(csproj) + 1;
                    }
                    var name = assembly.GetName().Name + ".csproj";
                    var best = counts.Keys.FirstOrDefault(k => Path.GetFileName(k).Equals(name, StringComparison.OrdinalIgnoreCase))
                               ?? counts.OrderByDescending(p => p.Value).Select(p => p.Key).FirstOrDefault();
                    return best == null ? null : Path.GetDirectoryName(best);
                }
            }
            catch
            {
                return null;
            }
        }

        #endregion

        #region Connection

        private static async Task ConnectLoop()
        {
            Uri uri = null;
            TcpListener cable = null;
            if (Cable)
            {
                cable = ListenOnCable();
                if (cable == null)
                {
                    Console.WriteLine($"DrawnUI Designer agent: ports {CablePorts}-{CablePorts + 9} are all taken, the designer cannot connect");
                    return;
                }
                Console.WriteLine($"DrawnUI Designer agent: waiting for a designer on the USB cable (port {((IPEndPoint)cable.LocalEndpoint).Port})");
                Log($"listening on the USB cable, port {((IPEndPoint)cable.LocalEndpoint).Port}");
            }
            else
            {
                var url = BakedUrl ?? Environment.GetEnvironmentVariable("DRAWNUI_DESIGNER_URL");
                uri = new Uri(string.IsNullOrWhiteSpace(url) ? "ws://127.0.0.1:54270" : url);
                // the address without its query: a device token stays out of the app's log
                Console.WriteLine($"DrawnUI Designer agent: waiting for a designer at {uri.GetLeftPart(UriPartial.Path)}");
            }
            WatchResume();
            if (cable != null)
            {
                await AcceptLoop(cable);
                return;
            }
            var delay = 1000;
            while (true)
            {
                WebSocket ws = null;
                try
                {
                    ws = await ConnectOut(uri);
                }
                catch
                {
                    // hub not running: retry quietly, the pause growing up to 30 s (cut short when the app resumes)
                    await Pause(delay);
                    delay = Math.Min(delay * 2, 30000);
                    continue;
                }
                delay = 1000;
                await RunSession(ws, null);
                await Pause(delay);
            }
        }

        private static CancellationTokenSource _wake = new();

        /// <summary>The pause between connect attempts; the app coming back to the foreground ends it.</summary>
        private static async Task Pause(int ms)
        {
            try
            {
                await Task.Delay(ms, _wake.Token);
            }
            catch (OperationCanceledException)
            {
                Interlocked.Exchange(ref _wake, new CancellationTokenSource());
            }
        }

        private static TcpListener _listener;

        /// <summary>
        /// iOS and Mac Catalyst: UIApplication.DidBecomeActiveNotification, by reflection. While an app is suspended
        /// iOS takes its sockets back: on the way back the cable listener is rebound, a connect-out pause ends, and a
        /// session whose socket died is dropped. One observer; it does nothing until the app resumes.
        /// </summary>
        private static void WatchResume()
        {
            if (!OperatingSystem.IsIOS()) // true on Mac Catalyst too
                return;
            try
            {
                var application = LoadedType("UIKit.UIApplication");
                var center = StaticGet(LoadedType("Foundation.NSNotificationCenter"), "DefaultCenter");
                var add = center?.GetType().GetMethods(BindingFlags.Public | BindingFlags.Instance).FirstOrDefault(m =>
                    m.Name == "AddObserver" && m.GetParameters() is { Length: 2 } ps && ps[0].ParameterType.Name == "NSString"
                    && typeof(Delegate).IsAssignableFrom(ps[1].ParameterType));
                if (application == null || add == null)
                    return;
                // one observer each, run only when iOS posts the notification
                foreach (var (notification, handler) in new[]
                         {
                             ("DidBecomeActiveNotification", nameof(OnResumed)),
                             ("WillResignActiveNotification", nameof(OnWillResignActive)),
                             ("DidEnterBackgroundNotification", nameof(OnDidEnterBackground)),
                             ("WillEnterForegroundNotification", nameof(OnWillEnterForeground)),
                         })
                    if (StaticGet(application, notification) is { } name)
                        add.Invoke(center, new[] { name, Bridge(add.GetParameters()[1].ParameterType, handler, 0) });
            }
            catch (Exception e)
            {
                // no app state signals: the listener still rebinds when its accept fails
                Log($"could not watch the app's state: {e.GetType().Name}: {e.Message}");
            }
        }

        /// <summary>
        /// One line in the app's log, for events only: never per frame or while waiting. On iOS and Mac Catalyst the
        /// device log hides app lines (private), so each line also goes, with its time, to
        /// Library/Caches/DrawnUi.Designer.log in the app's container, written and closed per line (the app may be
        /// suspended or killed right after) and cut back to its last half when it passes 64 KB. Never throws.
        /// </summary>
        internal static void Log(string message)
        {
            var line = "[DrawnUi.Designer] " + message;
            try
            {
                Console.WriteLine(line);
                if (!OperatingSystem.IsIOS()) // true on Mac Catalyst too
                    return;
                lock (LogLock)
                {
                    _logFile ??= Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.InternetCache), "DrawnUi.Designer.log");
                    File.AppendAllText(_logFile, $"{DateTime.Now:yyyy-MM-dd HH:mm:ss.fff} {line}\n");
                    var info = new FileInfo(_logFile);
                    if (info.Length > 64 * 1024)
                    {
                        var text = File.ReadAllText(_logFile);
                        var keep = text[(text.Length / 2)..];
                        File.WriteAllText(_logFile, keep[(keep.IndexOf('\n') + 1)..]); // whole lines only
                    }
                }
            }
            catch
            {
                // a log line is never worth an error in the app
            }
        }

        private static readonly object LogLock = new();
        private static string _logFile;

        private static void OnWillResignActive(object _) => Log("the app is going inactive");

        private static void OnDidEnterBackground(object _) => Log("the app went to the background");

        private static void OnWillEnterForeground(object _) => Log("the app is coming back to the foreground");

        private static void OnResumed(object _)
        {
            try
            {
                if (_listener != null)
                    _ = CheckListener();
                else
                    Log("the app became active");
                _wake.Cancel();
                if (_ws is { } ws && ws.State != WebSocketState.Open)
                    ws.Abort();
            }
            catch (Exception e)
            {
                ReportHandlerError("resume", e);
            }
        }

        private static int _selfCheck; // 1 while the agent connects to its own listener

        /// <summary>
        /// After the app comes back: is the cable listener still there? iOS takes the sockets of a suspended app, and
        /// then a connect to the port is refused while the old accept just waits on. The agent connects to its own port:
        /// accepted (and dropped at once), the listener is kept, so a designer connecting right then is not refused;
        /// refused, the listener is stopped and the accept loop binds a fresh one.
        /// </summary>
        private static async Task CheckListener()
        {
            var listener = _listener;
            try
            {
                var port = ((IPEndPoint)listener.LocalEndpoint).Port;
                using var probe = new TcpClient();
                using var cts = new CancellationTokenSource(1000);
                Interlocked.Exchange(ref _selfCheck, 1);
                await probe.ConnectAsync(IPAddress.Loopback, port, cts.Token);
                Log($"the app became active: the listener is still there (port {port})");
            }
            catch (Exception e)
            {
                Interlocked.Exchange(ref _selfCheck, 0);
                Log($"the app became active: the listener is gone ({Unwrap(e).GetType().Name}), binding a fresh one");
                try
                {
                    listener?.Stop(); // the waiting accept ends, the loop binds again
                }
                catch
                {
                }
            }
        }

        /// <summary>
        /// The USB cable: the listener accepts at all times. A new designer replaces the current session (a designer
        /// that was killed may leave its socket half open, and only the new one can tell). Waiting costs one pending
        /// accept.
        /// </summary>
        private static async Task AcceptLoop(TcpListener cable)
        {
            var current = Task.CompletedTask;
            var lastRebind = 0L;
            _listener = cable;
            while (true)
            {
                TcpClient tcp;
                try
                {
                    tcp = await _listener.AcceptTcpClientAsync();
                }
                catch (Exception e)
                {
                    // the listener is gone (iOS took the socket of the suspended app, or the app resumed): a new one
                    Log($"the listener stopped: {e.GetType().Name}: {e.Message}");
                    try
                    {
                        _listener.Stop();
                    }
                    catch
                    {
                    }
                    if (Environment.TickCount64 - lastRebind < 1000)
                        await Task.Delay(1000); // a listener that keeps failing: never a busy loop
                    lastRebind = Environment.TickCount64;
                    Log("binding a new listener");
                    TcpListener fresh;
                    var told = false;
                    while ((fresh = ListenOnCable()) == null)
                    {
                        if (!told)
                            Log($"no free port from {CablePorts} to {CablePorts + 9}: trying again every second");
                        told = true;
                        await Task.Delay(1000);
                    }
                    _listener = fresh;
                    var port = ((IPEndPoint)fresh.LocalEndpoint).Port;
                    Log($"listening on the USB cable again, port {port}");
                    if (_out != null)
                        _ = Send(new { type = "error", request = (uint?)null, message = $"agent: the cable listener was rebound (port {port})" });
                    continue;
                }
                WebSocket ws;
                try
                {
                    ws = await AcceptDesigner(tcp);
                }
                catch (Exception e)
                {
                    if (Interlocked.Exchange(ref _selfCheck, 0) == 0) // the agent's own check is not news
                        Log($"a connection on the cable was not a designer, dropped ({e.GetType().Name})");
                    tcp.Dispose(); // not a designer
                    continue;
                }
                if (_ws != null)
                {
                    _replaced = true;
                    _ws.Abort(); // the session before ends and unhooks
                }
                await Task.WhenAny(current, Task.Delay(3000));
                current = RunSession(ws, tcp);
            }
        }

        private static bool _replaced; // the session is ending because a new designer took over

        /// <summary>One designer, from the hello to its leaving; then nothing of the agent stays hooked into the app.</summary>
        private static async Task RunSession(WebSocket ws, IDisposable connection)
        {
            Log(connection != null ? "a designer connected on the USB cable" : "connected to the designer");
            var why = "the designer closed the connection";
            try
            {
                _ws = ws;
                _out = text => SendText(ws, text);
                if (await FindCanvas(ws))
                    await Session(ws);
                else
                    why = "the designer left before the app showed a canvas";
            }
            catch (Exception e)
            {
                // gone, aborted or replaced
                why = _replaced ? "a new designer took over" : $"the connection was lost: {Unwrap(e).GetType().Name}: {Unwrap(e).Message}";
            }
            finally
            {
                if (Interlocked.CompareExchange(ref _ws, null, ws) == ws)
                    _out = null;
                ws.Dispose();
                connection?.Dispose();
            }
            _replaced = false;
            Log("the session ended: " + why);
            if (_host == null || _out != null)
                return; // a new designer already took over
            try
            {
                // bounded: a UI thread that does not run (app suspended) must not keep the agent from the next designer
                await Task.WhenAny(Ui(() =>
                {
                    SetPickMode(false);
                    SetHighlight(null);
                    Unwatch();
                }), Task.Delay(3000));
            }
            catch
            {
            }
        }

        /// <summary>
        /// Dead peer detection: on .NET 9 and later runtimes (every phone app) the socket pings every 15 s and aborts
        /// when no pong comes within 15 s, so a designer that died without closing frees the session. Set by reflection:
        /// the agent compiles for net8.0, where only unsolicited pongs exist.
        /// </summary>
        private static void SetKeepAliveTimeout(object options) =>
            options.GetType().GetProperty("KeepAliveTimeout")?.SetValue(options, TimeSpan.FromSeconds(15));

        /// <summary>The platform the app runs on, for the designer to show; an iPad says ipados (UIKit's idiom, read on
        /// the UI thread, where UIKit wants it). UI thread.</summary>
        private static string Platform()
        {
            if (OperatingSystem.IsBrowser())
                return "browser";
            if (OperatingSystem.IsAndroid())
                return "android";
            if (OperatingSystem.IsMacCatalyst()) // before IsIOS, which is true there too
                return "maccatalyst";
            if (OperatingSystem.IsIOS())
                return Get(StaticGet(LoadedType("UIKit.UIDevice"), "CurrentDevice"), "UserInterfaceIdiom")?.ToString() == "Pad" ? "ipados" : "ios";
            if (OperatingSystem.IsWindows())
                return "windows";
            if (OperatingSystem.IsMacOS())
                return "macos";
            return OperatingSystem.IsLinux() ? "linux" : null;
        }

        private static async Task<WebSocket> ConnectOut(Uri uri)
        {
            var ws = new ClientWebSocket();
            ws.Options.KeepAliveInterval = TimeSpan.FromSeconds(15);
            SetKeepAliveTimeout(ws.Options);
            try
            {
                using var cts = new CancellationTokenSource(3000);
                await ws.ConnectAsync(uri, cts.Token);
                return ws;
            }
            catch
            {
                ws.Dispose();
                throw;
            }
        }

        private const int CablePorts = 54273;

        /// <summary>The phone's loopback, first free port of 54273-54282 (the designer tries them in order); null when
        /// all are taken.</summary>
        private static TcpListener ListenOnCable()
        {
            for (var port = CablePorts; port < CablePorts + 10; port++)
            {
                var listener = new TcpListener(IPAddress.Loopback, port);
                try
                {
                    listener.Start(1);
                    return listener;
                }
                catch (SocketException)
                {
                    listener.Stop();
                }
            }
            return null;
        }

        /// <summary>The server side of the WebSocket handshake on a connection the designer opened (RFC 6455).</summary>
        private static async Task<WebSocket> AcceptDesigner(TcpClient tcp)
        {
            var stream = tcp.GetStream();
            using var cts = new CancellationTokenSource(5000);
            var request = new StringBuilder();
            var one = new byte[1];
            while (!(request.Length >= 4 && request[^1] == '\n' && request[^2] == '\r' && request[^3] == '\n' && request[^4] == '\r'))
            {
                if (request.Length > 8192 || await stream.ReadAsync(one, cts.Token) != 1)
                    throw new IOException("not a WebSocket request");
                request.Append((char)one[0]);
            }
            var lines = request.ToString().Split("\r\n");
            var key = lines.FirstOrDefault(l => l.StartsWith("Sec-WebSocket-Key:", StringComparison.OrdinalIgnoreCase))?[18..].Trim();
            if (!lines[0].StartsWith("GET ") || string.IsNullOrEmpty(key))
                throw new IOException("not a WebSocket request");
            var accept = Convert.ToBase64String(System.Security.Cryptography.SHA1.HashData(
                Encoding.ASCII.GetBytes(key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11")));
            await stream.WriteAsync(Encoding.ASCII.GetBytes(
                $"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"), cts.Token);
            var options = new WebSocketCreationOptions { IsServer = true, KeepAliveInterval = TimeSpan.FromSeconds(15) };
            SetKeepAliveTimeout(options);
            return WebSocket.CreateFromStream(stream, options);
        }

        /// <summary>What a session starts with, whatever carries it: the hello, the screens, the tree.</summary>
        private static async Task Intro()
        {
            var page = _host is BlazorHost;
            var platform = page ? "browser" : await Ui(Platform);
            await Send(new
            {
                role = "app",
                protocol = Protocol,
                name = AppAssembly()?.GetName().Name ?? "app",
                engine = _host switch { MauiHost => "dotnet-maui", BlazorHost => "dotnet-blazor", _ => "dotnet-wpf" },
                pid = page ? 0 : Environment.ProcessId,
                platform,
                project = page ? null : _project ??= BakedProject ?? FindProject(),
                // what the hub injects when it runs the app again: the DLL the build took the agent from (the app
                // carries a copy in its output or bundle, which the hub cannot build with); a page is not run again
                agent = page ? null
                    : AppContext.GetData("DrawnUi.Designer.AgentSource") as string is { Length: > 0 } source ? source : typeof(AgentHost).Assembly.Location,
            });

            await Ui(RefreshCanvas);
            await EnsureCurrent();
            var screens = await Ui(ReadScreens);
            if (screens != null)
                await Send(new { type = "screens", screens });
            await SendTree();
        }

        private static async Task Session(WebSocket ws)
        {
            await Intro();

            var buffer = new byte[64 * 1024];
            using var message = new MemoryStream();
            while (ws.State == WebSocketState.Open)
            {
                var result = await ws.ReceiveAsync(buffer, CancellationToken.None);
                if (result.MessageType == WebSocketMessageType.Close)
                {
                    try
                    {
                        await ws.CloseOutputAsync(WebSocketCloseStatus.NormalClosure, null, CancellationToken.None);
                    }
                    catch
                    {
                    }
                    return;
                }
                message.Write(buffer, 0, result.Count);
                if (!result.EndOfMessage)
                    continue;

                var text = Encoding.UTF8.GetString(message.GetBuffer(), 0, (int)message.Length);
                message.SetLength(0);
                await Handle(text);
            }
        }

        private static Task Send(object msg)
        {
            var output = _out;
            return output == null ? Task.CompletedTask : output(JsonSerializer.Serialize(msg));
        }

        private static async Task SendText(WebSocket ws, string text)
        {
            if (ws.State != WebSocketState.Open)
                return;
            var bytes = Encoding.UTF8.GetBytes(text);
            await SendLock.WaitAsync();
            try
            {
                await ws.SendAsync(bytes, WebSocketMessageType.Text, true, CancellationToken.None);
            }
            finally
            {
                SendLock.Release();
            }
        }

        private static Task _pageIntro = Task.CompletedTask;

        internal static Func<string, Task> AttachPage(object canvas, Func<string, Task> send)
        {
            if (canvas == null || send == null)
                throw new ArgumentNullException(canvas == null ? nameof(canvas) : nameof(send));
            Interlocked.Exchange(ref _started, 1); // no socket loop in this app
            if (_host != null && _canvas != null)
            {
                // the page made a new canvas (the snippet ran again): nothing stays hooked into the old one
                SetPickMode(false);
                SetHighlight(null);
                Unwatch();
            }
            _host = new BlazorHost(canvas);
            _canvas = canvas;
            _out = send;
            _pageIntro = PageIntro();
            var gate = new SemaphoreSlim(1, 1);
            return async text =>
            {
                await gate.WaitAsync(); // one message at a time, never blocking the page
                try
                {
                    await _pageIntro;
                    await Handle(text);
                }
                finally
                {
                    gate.Release();
                }
            };
        }

        private static async Task PageIntro()
        {
            try
            {
                await Intro();
            }
            catch (Exception e)
            {
                ReportHandlerError("page attach", e);
            }
        }

        private static Task SendError(uint? request, string message) => Send(new { type = "error", request, message });

        private static async Task Handle(string text)
        {
            uint? request = null;
            try
            {
                using var doc = JsonDocument.Parse(text);
                var root = doc.RootElement;
                if (root.TryGetProperty("request", out var r) && r.ValueKind == JsonValueKind.Number)
                    request = r.GetUInt32();

                await Ui(RefreshCanvas);
                switch (root.GetProperty("type").GetString())
                {
                    case "getTree":
                        await EnsureCurrent();
                        await SendTree();
                        break;

                    case "rebuilt":
                        // the app rebuilt its controls in place (DrawFiddle after each run): a new tree goes out, a
                        // highlight on a control that is gone is dropped, pick mode stays as it was
                        await EnsureCurrent();
                        await Ui(() =>
                        {
                            if (_highlightTarget != null && (!_highlightTarget.TryGetTarget(out var shown)
                                    || !Walk(false).Any(v => ReferenceEquals(v.Control, shown))))
                                SetHighlight(null);
                        });
                        await SendTree();
                        break;

                    case "snapshot":
                        var (png, width, height) = await Snapshot();
                        await Send(new { type = "snapshot", request = request ?? 0, pngBase64 = png, width, height });
                        break;

                    case "getProps":
                        var target = root.GetProperty("node").GetInt64();
                        await EnsureCurrent();
                        var props = await Ui(() => ReadProps(Find(target)));
                        await Send(new { type = "props", node = target, props });
                        break;

                    case "pick":
                        var x = root.GetProperty("x").GetSingle();
                        var y = root.GetProperty("y").GetSingle();
                        await EnsureCurrent();
                        var picked = await Ui(() => Pick(x, y));
                        await Send(new { type = "picked", request = request ?? 0, node = picked });
                        break;

                    case "pickMode":
                        var pickOn = root.GetProperty("on").GetBoolean();
                        await Ui(() =>
                        {
                            if (_placeType != null)
                                SetPickMode(false); // place mode ends; pick starts fresh
                            SetPickMode(pickOn);
                        });
                        break;

                    case "getControls":
                        var controls = await Ui(Controls);
                        await Send(new { type = "controls", controls });
                        break;

                    case "insert":
                        var insertParent = root.GetProperty("parent").GetInt64();
                        int? insertIndex = root.TryGetProperty("index", out var ii) && ii.ValueKind == JsonValueKind.Number ? ii.GetInt32() : null;
                        var insertType = root.GetProperty("typeName").GetString();
                        // pasted controls: their properties, after the type's defaults (["name", "value"] pairs)
                        var insertProps = root.TryGetProperty("props", out var ip) && ip.ValueKind == JsonValueKind.Array
                            ? ip.EnumerateArray().Where(e => e.ValueKind == JsonValueKind.Array && e.GetArrayLength() == 2)
                                .Select(e => new[] { e[0].GetString(), e[1].ValueKind == JsonValueKind.String ? e[1].GetString() : e[1].GetRawText() })
                                .ToList()
                            : null;
                        // a pasted copy's children: {typeName, props: [{name, value, typeName}], children: [...]}
                        var insertChildren = root.TryGetProperty("children", out var ic) && ic.ValueKind == JsonValueKind.Array
                            ? ic.EnumerateArray().ToList()
                            : null;
                        try
                        {
                            var (newNode, at, skipped) = await Ui(() => InsertControl(insertParent, insertIndex, insertType, insertProps, insertChildren));
                            await Send(new { type = "inserted", request = request ?? 0, node = (long?)newNode, parent = (long?)insertParent, index = (int?)at });
                            if (skipped.Count > 0)
                                await Send(new { type = "error", request = (uint?)null, message = $"inserted {insertType.Split('.').Last()} without {string.Join("; ", skipped)}" });
                        }
                        catch (Exception e)
                        {
                            await SendError(request, Unwrap(e).Message);
                            await Send(new { type = "inserted", request = request ?? 0, node = (long?)null, parent = (long?)insertParent, index = insertIndex });
                            break;
                        }
                        await EnsureCurrent();
                        await SendTree();
                        break;

                    case "move":
                        var moveNode = root.GetProperty("node").GetInt64();
                        var moveParent = root.GetProperty("parent").GetInt64();
                        int? moveIndex = root.TryGetProperty("index", out var mi) && mi.ValueKind == JsonValueKind.Number ? mi.GetInt32() : null;
                        try
                        {
                            var movedAt = await Ui(() => MoveControl(moveNode, moveParent, moveIndex));
                            await Send(new { type = "moved", request = request ?? 0, node = (long?)moveNode, parent = (long?)moveParent, index = (int?)movedAt });
                        }
                        catch (Exception e)
                        {
                            await SendError(request, Unwrap(e).Message);
                            await Send(new { type = "moved", request = request ?? 0, node = (long?)null, parent = (long?)moveParent, index = moveIndex });
                            break;
                        }
                        await EnsureCurrent();
                        await SendTree();
                        break;

                    case "copy":
                        var copyNode = root.GetProperty("node").GetInt64();
                        var spec = await Ui(() => ById.TryGetValue(copyNode, out var weak) && weak.TryGetTarget(out var copied) ? SpecOf(copied) : null);
                        await Send(new { type = "copied", request = request ?? 0, spec });
                        break;

                    case "placeMode":
                        var placeType = root.TryGetProperty("typeName", out var pt) && pt.ValueKind == JsonValueKind.String ? pt.GetString() : null;
                        await Ui(() => SetPlaceMode(placeType));
                        break;

                    case "placeAt":
                        // a drop from the editor's Controls onto the canvas: as a place-mode click there
                        var dropType = root.GetProperty("typeName").GetString();
                        var (dropX, dropY) = (root.GetProperty("x").GetSingle(), root.GetProperty("y").GetSingle());
                        await Ui(() =>
                        {
                            if (_picking)
                                SetPickMode(false);
                            PlaceHere(dropType, dropX, dropY);
                        });
                        break;

                    case "remove":
                        var removeNode = root.GetProperty("node").GetInt64();
                        await Ui(() => RemoveControl(removeNode));
                        await EnsureCurrent();
                        await SendTree();
                        break;

                    case "highlight":
                        var h = root.GetProperty("node");
                        long? id = h.ValueKind == JsonValueKind.Number ? h.GetInt64() : null;
                        _boxColor = BoxColor(root.TryGetProperty("color", out var hc) && hc.ValueKind == JsonValueKind.String ? hc.GetString() : null);
                        await Ui(() => SetHighlight(id));
                        break;

                    case "setProperty":
                        var node = root.GetProperty("node").GetInt64();
                        var name = root.GetProperty("name").GetString();
                        var value = root.GetProperty("value").GetString();
                        await Ui(() => SetProperty(node, name, value));
                        await EnsureCurrent();
                        break;

                    case "focus":
                        await Ui(BringToFront);
                        break;

                    case "openScreen":
                        var route = root.GetProperty("route").GetString();
                        _hubNavigating = true; // its own tree goes out below: no second one from the navigation events
                        try
                        {
                            var nav = await Ui(() => OpenScreen(route));
                            _ = nav.ContinueWith(t => SendError(null, $"openScreen {route}: {Unwrap(t.Exception).Message}"),
                                TaskContinuationOptions.OnlyOnFaulted);
                            // transitions run on frames: on screen they come by themselves, hidden the agent draws them
                            var navigating = System.Diagnostics.Stopwatch.StartNew();
                            do
                            {
                                await EnsureCurrent();
                                if (!nav.IsCompleted)
                                    await Task.WhenAny(nav, Task.Delay(50));
                            } while (!nav.IsCompleted && navigating.ElapsedMilliseconds < 3000);
                            await Ui(RefreshCanvas);
                            await EnsureCurrent();
                            await SendTree();
                        }
                        finally
                        {
                            _hubNavigating = false;
                        }
                        break;

                    default:
                        await SendError(request, "unknown message: " + text);
                        break;
                }
            }
            catch (Exception e)
            {
                await SendError(request, Unwrap(e).Message);
            }
        }

        private static Exception Unwrap(Exception e)
        {
            while ((e is TargetInvocationException || e is AggregateException) && e.InnerException != null)
                e = e.InnerException;
            return e;
        }

        #endregion

        #region Trees after navigation

        private static bool _hubNavigating;
        private static long _navigatedAt, _treeSentAt;
        private static int _pushQueued;
        private static bool _watchingApp;
        private static WeakReference<object> _watchedShellRef;
        private static Delegate _shellNavigated;

        private static object _watchedShell
        {
            get => _watchedShellRef != null && _watchedShellRef.TryGetTarget(out var shell) ? shell : null;
            set => _watchedShellRef = value == null ? null : new WeakReference<object>(value);
        }
        private static EventInfo _hotReload;
        private static Delegate _onHotReload;

        /// <summary>
        /// Subscribes, once each while a designer is connected, to what tells that the screen changed without the hub
        /// asking: the framework's page changes (MAUI: Application.PageAppearing, any navigation), DrawnUi's hot reload
        /// (Super.HotReload), and the navigated event of the app's SkiaShell (a new one after a rebuild). UI thread.
        /// </summary>
        private static void WatchNavigation()
        {
            if (!_watchingApp)
            {
                _watchingApp = true;
                _host.WatchNavigation(true);
                var super = _canvas?.GetType().Assembly.GetType("DrawnUi.Draw.Super") ?? LoadedType("DrawnUi.Draw.Super");
                if (super?.GetEvent("HotReload", BindingFlags.Public | BindingFlags.Static) is { } reload)
                    reload.AddEventHandler(null, _onHotReload = Bridge((_hotReload = reload).EventHandlerType, nameof(OnHotReloaded), 0));
            }
            if (_canvas == null || FindShell() is not { } shell || ReferenceEquals(shell, _watchedShell))
                return;
            UnwatchShell();
            _watchedShell = shell;
            _shellNavigated = null;
            if (shell.GetType().GetEvent("Navigated") is { } navigated)
                navigated.AddEventHandler(shell, _shellNavigated = Bridge(navigated.EventHandlerType, nameof(OnNavigated), 1));
        }

        /// <summary>Removes every subscription of <see cref="WatchNavigation"/> (the designer left). UI thread.</summary>
        private static void Unwatch()
        {
            if (!_watchingApp)
                return;
            _watchingApp = false;
            _host.WatchNavigation(false);
            _hotReload?.RemoveEventHandler(null, _onHotReload);
            (_hotReload, _onHotReload) = (null, null);
            UnwatchShell();
            (_watchedShell, _shellNavigated) = (null, null);
            WatchScrolls(null);
        }

        private static void UnwatchShell()
        {
            try
            {
                if (_watchedShell is { } old && _shellNavigated != null)
                    old.GetType().GetEvent("Navigated")?.RemoveEventHandler(old, _shellNavigated);
            }
            catch
            {
                // a disposed shell: nothing left to unhook
            }
        }

        private static readonly List<WeakReference<object>> WatchedScrolls = new(); // weakly: a scroll may go with its page
        private static Delegate _onScrollEnded;

        /// <summary>
        /// SkiaScroll.ScrollingEnded of every scroll in the tree just sent (recycled cells show other items once a
        /// scroll stops): the tree follows it like a navigation, once, after the scroll comes to rest. Nothing runs while
        /// the list moves: the event fires once per stop. Scrolls gone from the tree are released, new ones (a new
        /// screen) picked up with the next tree; null releases all (the designer left). UI thread.
        /// </summary>
        private static void WatchScrolls(List<Visit> visits)
        {
            var now = visits?.Select(v => v.Control).Where(c => IsType(c.GetType(), "DrawnUi.Draw.SkiaScroll"))
                          .ToHashSet(ReferenceEqualityComparer.Instance) ?? new HashSet<object>();
            var kept = new HashSet<object>(ReferenceEqualityComparer.Instance);
            foreach (var weak in WatchedScrolls.ToList())
            {
                if (!weak.TryGetTarget(out var scroll))
                {
                    WatchedScrolls.Remove(weak); // collected with its page: nothing to unsubscribe
                    continue;
                }
                if (now.Contains(scroll))
                {
                    kept.Add(scroll);
                    continue;
                }
                try
                {
                    scroll.GetType().GetEvent("ScrollingEnded")?.RemoveEventHandler(scroll, _onScrollEnded);
                }
                catch
                {
                    // a disposed scroll: its event is already cleared
                }
                WatchedScrolls.Remove(weak);
            }
            foreach (var scroll in now)
            {
                if (kept.Contains(scroll) || scroll.GetType().GetEvent("ScrollingEnded") is not { } ended)
                    continue;
                _onScrollEnded ??= Bridge(ended.EventHandlerType, nameof(OnNavigated), 1); // one handler for all scrolls
                ended.AddEventHandler(scroll, _onScrollEnded);
                WatchedScrolls.Add(new WeakReference<object>(scroll));
            }
        }

        /// <summary>DrawnUi raised Super.HotReload (code-built pages rebuild now): the designer hears it, then the tree
        /// follows like after a navigation.</summary>
        private static void OnHotReloaded(object args)
        {
            // DrawnUi raises it on a pool thread as one multicast call: a throw here would stop the app's own handlers
            // (the page rebuild) behind ours. Nothing UI-related runs here; the tree follow goes through the UI dispatch.
            try
            {
                SendHotReload("reloaded");
                OnNavigated(args);
            }
            catch (Exception e)
            {
                ReportHandlerError("Super.HotReload", e);
            }
        }

        /// <summary>An exception an event handler of the agent caught (it never reaches the app's event), for the
        /// designer, only while connected.</summary>
        internal static void ReportHandlerError(string where, Exception e)
        {
            try
            {
                if (_out != null)
                    _ = Send(new { type = "error", request = (uint?)null, message = $"agent {where} handler: {Unwrap(e).GetType().Name}: {Unwrap(e).Message}" });
            }
            catch
            {
            }
        }

        /// <summary>A hot reload step for the designer, only while one is connected; a small message, nothing else.</summary>
        internal static void SendHotReload(string stage)
        {
            if (_out != null)
                _ = Send(new { type = "hotReload", stage });
        }

        /// <summary>Any of the events above: one tree goes out once they stop for 300 ms (a navigation raises several).</summary>
        private static void OnNavigated(object _)
        {
            try
            {
                Interlocked.Exchange(ref _navigatedAt, System.Diagnostics.Stopwatch.GetTimestamp());
                if (Interlocked.Exchange(ref _pushQueued, 1) == 0)
                    _ = PushTreeAfterNavigation();
            }
            catch (Exception e)
            {
                ReportHandlerError("navigation", e);
            }
        }

        private static async Task PushTreeAfterNavigation()
        {
            try
            {
                long last;
                do
                {
                    last = Interlocked.Read(ref _navigatedAt);
                    await Task.Delay(300);
                } while (Interlocked.Read(ref _navigatedAt) != last);
                Interlocked.Exchange(ref _pushQueued, 0);

                // the hub's own openScreen sends its tree; one sent after these events already shows them
                if (_hubNavigating || Interlocked.Read(ref _treeSentAt) > last || _out == null)
                    return;
                await Ui(RefreshCanvas);
                await EnsureCurrent();
                await SendTree();
            }
            catch
            {
                Interlocked.Exchange(ref _pushQueued, 0);
            }
        }

        #endregion

        #region Tree

        private sealed class Visit
        {
            public object Control;
            public long Id;
            public long? Parent;
            public bool Visible;
            public bool Internal;
            public long? TemplateIndex;
            public float[] Rect;
            public float[] ScreenRect; // drawn this frame only, empty otherwise
        }

        private static async Task SendTree()
        {
            Interlocked.Exchange(ref _treeSentAt, System.Diagnostics.Stopwatch.GetTimestamp());
            var tree = await Ui(() =>
            {
                var visits = Walk(false);
                WatchScrolls(visits);
                var fields = FieldNames(visits);
                var host = HostTypeName();
                var wrapper = PageWrapper;
                var pageRoot = wrapper == null ? null : visits.FirstOrDefault(v => ReferenceEquals(v.Control, wrapper))?.Id;
                return (pageRoot, (object)new
                {
                    type = "tree",
                    revision = (ulong)Interlocked.Increment(ref _revision),
                    width = _canvasWidth,
                    height = _canvasHeight,
                    nodes = visits.Select(v => new
                    {
                        id = v.Id,
                        parent = v.Parent,
                        typeName = v.Control.GetType().FullName,
                        assembly = v.Control.GetType().Assembly.GetName().Name,
                        tag = Get(v.Control, "Tag") is string { Length: > 0 } tag ? tag : null,
                        text = TextPreview(v.Control),
                        @internal = v.Internal,
                        templateIndex = v.TemplateIndex,
                        // a layout that makes its children from ItemsSource / ItemTemplate, even with no items yet
                        templated = Get(v.Control, "IsTemplated") is true,
                        field = fields.TryGetValue(v.Control, out var f) ? f : null,
                        host = v.Parent == null ? host : null,
                        rect = v.Rect,
                        visible = v.Visible,
                    }).ToList(),
                });
            });
            if (tree.pageRoot is not { } wrapperId)
            {
                await Send(tree.Item2);
                return;
            }
            // page mode: the page's own wrapper is marked (pageRoot is left out on every other node)
            var json = JsonSerializer.SerializeToNode(tree.Item2);
            if (json["nodes"].AsArray().FirstOrDefault(n => (long)n["id"] == wrapperId) is JsonObject node)
                node["pageRoot"] = true;
            await Send(json);
        }

        /// <summary>
        /// The app class that holds the canvas (sent on root nodes): the nearest page or window of the app's own types
        /// above the canvas, else the nearest app type; null when there is none. Anchors code edits on pages that keep
        /// no field for their controls. UI thread.
        /// </summary>
        private static string HostTypeName()
        {
            string first = null;
            foreach (var element in _host.Ancestors(_canvas))
            {
                var type = element.GetType();
                if (!IsAppType(type))
                    continue;
                if (_host.IsPage(element))
                    return type.FullName;
                first ??= type.FullName;
            }
            return first;
        }

        /// <summary>Parents before children. Paint order (ZIndex) when picking, the parent's own order otherwise.</summary>
        private static List<Visit> Walk(bool paintOrder)
        {
            (_canvasWidth, _canvasHeight) = _host.CanvasPixelSize(_canvas);
            var list = new List<Visit>();
            var seen = new HashSet<object>(ReferenceEqualityComparer.Instance);
            if (Get(_canvas, "Views") is IEnumerable roots)
                foreach (var root in roots.Cast<object>().ToArray())
                    Walk(root, null, true, false, null, paintOrder, list, seen);

            foreach (var dead in ById.Where(p => !p.Value.TryGetTarget(out _)).Select(p => p.Key).ToList())
                ById.Remove(dead);
            return list;
        }

        /// <param name="libraryMade">the parent's children are the library's own parts</param>
        /// <param name="templateIndex">this control is a recycled cell bound to that item</param>
        private static void Walk(object control, long? parent, bool parentVisible, bool libraryMade, long? templateIndex,
            bool paintOrder, List<Visit> list, HashSet<object> seen)
        {
            if (control == null || !seen.Add(control))
                return;

            var type = control.GetType();
            var id = IdOf(control);
            var visible = parentVisible && Get(control, "IsVisible") is true;
            var screen = Rect(Call(control, "GetAccessibilityPixelRect"));
            var drawn = screen[2] > 0 && screen[3] > 0;
            var rect = drawn ? screen : Rect(Get(control, "DrawingRect"));
            // shown on the canvas now: visible, drawn last frame, and meeting the canvas
            var onCanvas = visible && drawn && screen[0] < _canvasWidth && screen[1] < _canvasHeight
                           && screen[0] + screen[2] > 0 && screen[1] + screen[3] > 0;

            // Internal = the library made it for its own parts:
            // - a type nested in a library type (SkiaButton+ButtonLabel);
            // - or a library-type control inside a subtree the library made: the children a control built in
            //   CreateDefaultContent (SkiaControl._ownsDefaultContent: a button's label and shape, a slider's track
            //   and thumb), and everything a SkiaShell adds (page hosts, nav bar, tab bar, overlay layers) except
            //   the app views it holds (root content, pages, popup / modal / toast content).
            // An instance of an app class is never internal, nor is a recycled cell (made from the app's template).
            var isInternal = templateIndex == null && !IsAppType(type)
                             && ((type.DeclaringType != null && IsLibrary(type.DeclaringType.Assembly)) || libraryMade);
            list.Add(new Visit
            {
                Control = control, Id = id, Parent = parent, Visible = onCanvas, Internal = isInternal,
                TemplateIndex = templateIndex, Rect = rect, ScreenRect = screen,
            });

            var childrenLibraryMade = isInternal || OwnsDefaultContent(control);
            HashSet<object> appViews = null;
            if (IsNamed(type, "SkiaShell"))
            {
                childrenLibraryMade = true;
                appViews = ShellAppViews(control);
            }

            var cells = TemplatedCells(control);
            IEnumerable<object> children = ChildrenOf(control, cells);
            if (paintOrder)
                children = children.OrderBy(c => Convert.ToInt32(Get(c, "ZIndex") ?? 0)); // stable sort
            foreach (var child in children)
            {
                long? index = null;
                if (cells != null && cells.Contains(child) && Convert.ToInt64(Get(child, "ContextIndex") ?? -1) is var i and >= 0)
                    index = i;
                var made = childrenLibraryMade && !(appViews?.Contains(child) ?? false);
                Walk(child, id, visible, made, index, paintOrder, list, seen);
            }
        }

        private static List<object> TemplatedCells(object control) =>
            Get(control, "IsTemplated") is true && Get(control, "ChildrenFactory") is { } factory
            && Call(factory, "GetCellsInUse") is IEnumerable cells
                ? cells.Cast<object>().OrderBy(c => Convert.ToInt32(Get(c, "ContextIndex") ?? 0)).ToList()
                : null;

        private static List<object> ChildrenOf(object control, List<object> cells = null)
        {
            var result = new List<object>();
            if (Get(control, "Views") is IEnumerable views)
                result.AddRange(views.Cast<object>().ToArray());

            // recycled cells of a templated layout are not in Views
            cells ??= TemplatedCells(control);
            if (cells != null)
                result.AddRange(cells.Where(c => !result.Contains(c)));

            if (Get(control, "Content") is { } content && IsType(content.GetType(), "DrawnUi.Draw.SkiaControl") && !result.Contains(content))
                result.Add(content);
            return result;
        }

        #region Ownership

        private static FieldInfo _ownsDefaultContent;
        private static bool _ownsDefaultContentLooked;

        /// <summary>The control built its current children itself (CreateDefaultContent on an empty control).</summary>
        private static bool OwnsDefaultContent(object control)
        {
            if (!_ownsDefaultContentLooked)
            {
                _ownsDefaultContentLooked = true;
                for (var t = control.GetType(); t != null && _ownsDefaultContent == null; t = t.BaseType)
                    if (t.FullName == "DrawnUi.Draw.SkiaControl")
                        _ownsDefaultContent = t.GetField("_ownsDefaultContent", BindingFlags.NonPublic | BindingFlags.Instance);
            }
            return _ownsDefaultContent?.GetValue(control) is true;
        }

        /// <summary>The app views a SkiaShell holds: root content, tab roots, every page of every stack,
        /// popup / modal / toast content. Everything else under the shell is the shell's own.</summary>
        private static HashSet<object> ShellAppViews(object shell)
        {
            var views = new HashSet<object>(ReferenceEqualityComparer.Instance);
            void Add(object o)
            {
                if (o == null || o is string)
                    return;
                if (IsType(o.GetType(), "DrawnUi.Draw.SkiaControl"))
                    views.Add(o);
                else if (o is IEnumerable items)
                    foreach (var item in items)
                        Add(item);
                else // PageEntry.View, Overlay.Content
                    foreach (var name in new[] { "View", "Content" })
                        Add(o.GetType().GetField(name)?.GetValue(o));
            }

            foreach (var f in ShellFields(shell.GetType()))
                Add(f.GetValue(shell));
            return views;
        }

        private static readonly Dictionary<Type, FieldInfo[]> ShellFieldsByType = new();

        private static FieldInfo[] ShellFields(Type type)
        {
            if (!ShellFieldsByType.TryGetValue(type, out var fields))
            {
                var list = new List<FieldInfo>();
                for (var t = type; t != null; t = t.BaseType)
                    list.AddRange(t.GetFields(BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly)
                        .Where(f => f.Name is "_rootContent" or "_tabRoots" or "_stacks" or "_popups" or "_modals" or "_toasts"));
                ShellFieldsByType[type] = fields = list.ToArray();
            }
            return fields;
        }

        private static bool IsNamed(Type type, string name)
        {
            for (var t = type; t != null; t = t.BaseType)
                if (t.Name == name)
                    return true;
            return false;
        }

        private static readonly Dictionary<Assembly, bool> Libraries = new();

        private static bool IsAppType(Type type) => !IsLibrary(type.Assembly);

        /// <summary>DrawnUi and the platform; everything else is app code (the app and its own libraries).</summary>
        private static bool IsLibrary(Assembly assembly)
        {
            if (Libraries.TryGetValue(assembly, out var library))
                return library;
            var name = assembly.GetName().Name ?? "";
            library = assembly == _canvas.GetType().Assembly
                      || name.StartsWith("DrawnUi", StringComparison.Ordinal) || name.StartsWith("SkiaSharp", StringComparison.Ordinal)
                      || name.StartsWith("HarfBuzzSharp", StringComparison.Ordinal) || name.StartsWith("AppoMobi", StringComparison.Ordinal)
                      || name.StartsWith("System", StringComparison.Ordinal) || name.StartsWith("Microsoft", StringComparison.Ordinal)
                      || name.StartsWith("Presentation", StringComparison.Ordinal) || name is "mscorlib" or "netstandard" or "WindowsBase";
            return Libraries[assembly] = library;
        }

        /// <summary>
        /// "Namespace.Class._field" for controls held in an instance field of an app class: the app's windows and
        /// Application first, then every app-class control in tree order, so the nearest owner wins.
        /// </summary>
        private static readonly Dictionary<Type, FieldInfo[]> OwnerFieldsByType = new();

        /// <summary>A type's own reference fields that may hold a control, looked up once per type. Not:
        /// compiler-generated ones (auto-property backing fields).</summary>
        private static FieldInfo[] OwnerFields(Type type)
        {
            if (!OwnerFieldsByType.TryGetValue(type, out var fields))
                OwnerFieldsByType[type] = fields = type
                    .GetFields(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly)
                    .Where(f => !f.FieldType.IsValueType && !f.Name.Contains('<')).ToArray();
            return fields;
        }

        private static Dictionary<object, string> FieldNames(List<Visit> visits)
        {
            var names = new Dictionary<object, string>(ReferenceEqualityComparer.Instance);
            var inTree = new HashSet<object>(visits.Select(v => v.Control), ReferenceEqualityComparer.Instance);
            var owners = new List<object>(_host.FieldOwners());
            owners.AddRange(visits.Select(v => v.Control).Where(c => IsAppType(c.GetType())));

            foreach (var owner in owners)
                for (var t = owner.GetType(); t != null && IsAppType(t); t = t.BaseType)
                    foreach (var f in OwnerFields(t))
                    {
                        object value;
                        try
                        {
                            value = f.GetValue(owner);
                        }
                        catch
                        {
                            continue;
                        }
                        if (value != null && inTree.Contains(value))
                            names[value] = t.FullName + "." + f.Name;
                    }
            return names;
        }

        #endregion

        private static float _canvasWidth, _canvasHeight;

        private static long IdOf(object control)
        {
            var box = Ids.GetValue(control, _ => new StrongBox<long>(Interlocked.Increment(ref _nextId)));
            ById[box.Value] = new WeakReference<object>(control);
            return box.Value;
        }

        private static object Find(long id)
        {
            if (ById.TryGetValue(id, out var weak) && weak.TryGetTarget(out var control))
                return control;
            throw new InvalidOperationException($"node {id} not found (request a new tree)");
        }

        /// <summary>
        /// The topmost visible control at a canvas point. A part the library made (a button's label) answers as the
        /// control it belongs to, the nearest ancestor that is not internal: that is the one the app's code declares.
        /// </summary>
        private static long? Pick(float x, float y)
        {
            Visit hit = null;
            var visits = Walk(true);
            foreach (var v in visits)
            {
                var r = v.ScreenRect;
                if (v.Visible && r[2] > 0 && r[3] > 0 && x >= r[0] && y >= r[1] && x < r[0] + r[2] && y < r[1] + r[3])
                    hit = v; // later in paint order = on top
            }
            var byId = visits.ToDictionary(v => v.Id);
            while (hit is { Internal: true, Parent: { } parent } && byId.TryGetValue(parent, out var owner))
                hit = owner;
            return hit?.Id;
        }

        private static float[] Rect(object skRect)
        {
            if (skRect == null)
                return new float[4];
            float F(string name)
            {
                var f = Convert.ToSingle(Get(skRect, name) ?? 0f);
                return float.IsFinite(f) ? f : 0f;
            }
            return new[] { F("Left"), F("Top"), F("Width"), F("Height") };
        }

        #endregion

        #region Properties

        private sealed class PropDef
        {
            public string Name;
            public object Property; // the app's BindableProperty
            public Type ReturnType;
            /// <summary>A person can type or pick its value (see <see cref="IsEditable"/>); getProps lists only these.</summary>
            public bool Editable;
            public string Group;
            /// <summary>The values an editor offers (enum cases, LayoutOptions); null for free values.</summary>
            public string[] Choices;
            /// <summary>The choices combine (a [Flags] enum).</summary>
            public bool Flags;
        }

        private static readonly Dictionary<Type, List<PropDef>> PropDefs = new();
        private static MethodInfo _getValue, _setValue, _isSet;

        /// <summary>Every static <c>*Property</c> field of type BindableProperty on the type chain, derived first.</summary>
        private static List<PropDef> DefsOf(Type type)
        {
            if (PropDefs.TryGetValue(type, out var defs))
                return defs;

            defs = new List<PropDef>();
            var names = new HashSet<string>();
            for (var t = type; t != null; t = t.BaseType)
            {
                foreach (var f in t.GetFields(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static | BindingFlags.DeclaredOnly))
                {
                    // DrawnUi's own BindableProperty (DrawnUi.Net heads) or MAUI's (DrawnUi.Maui)
                    if (f.FieldType.FullName is not ("DrawnUi.Draw.BindableProperty" or "Microsoft.Maui.Controls.BindableProperty") ||
                        !f.Name.EndsWith("Property"))
                        continue;
                    var bp = f.GetValue(null);
                    if (bp == null)
                        continue;
                    var name = Get(bp, "PropertyName") as string ?? f.Name[..^"Property".Length];
                    if (names.Add(name))
                    {
                        var returnType = Get(bp, "ReturnType") as Type ?? typeof(object);
                        defs.Add(new PropDef
                        {
                            Name = name, Property = bp, ReturnType = returnType,
                            // A read-only bindable property (MAUI's CreateReadOnly) is the control's to set.
                            Editable = IsEditable(type, name, returnType) && !EngineManaged(type, f, bp, name) && Get(bp, "IsReadOnly") is not true,
                            Group = GroupOf(name), Choices = ChoicesOf(returnType),
                            Flags = (Nullable.GetUnderlyingType(returnType) ?? returnType) is { IsEnum: true } e && e.IsDefined(typeof(FlagsAttribute), false),
                        });
                    }
                }
            }

            if (defs.Count > 0 && _getValue == null)
            {
                var bpType = defs[0].Property.GetType();
                for (var t = type; t != null && _getValue == null; t = t.BaseType)
                {
                    if (t.FullName is not ("DrawnUi.Draw.BindableObject" or "Microsoft.Maui.Controls.BindableObject"))
                        continue;
                    MethodInfo M(string n, int argc) => t.GetMethods(Inst).FirstOrDefault(m =>
                        m.Name == n && m.GetParameters().Length == argc && m.GetParameters()[0].ParameterType == bpType);
                    _getValue = M("GetValue", 1);
                    _setValue = M("SetValue", 2);
                    _isSet = M("IsSet", 1);
                }
            }

            PropDefs[type] = defs;
            return defs;
        }

        /// <summary>
        /// The engine's own runtime state, which DrawnUi does not mark: the canvas writes it and puts its value back
        /// after a set. Found by setting every editable property of the sample apps and reading it after a frame
        /// (TestHub revert): RenderingScale (the screen scale), OrderedScroll (a scroll command: the scroll resets it
        /// to -1 once it has scrolled); IsHovered is the pointer state the canvas decides after each pointer pass.
        /// </summary>
        private static readonly HashSet<string> EngineStateNames = new() { "RenderingScale", "OrderedScroll", "IsHovered" };

        /// <summary>
        /// Not something an app sets, by rule where the framework says so: a read-only bindable property (MAUI
        /// CreateReadOnly), one whose own property is hidden by a "new" property of a subclass that does not use it
        /// (MAUI VisualElement.Width / Height under SkiaControl's), a property marked EditorBrowsable(Never); then the
        /// engine state above.
        /// </summary>
        private static bool EngineManaged(Type owner, FieldInfo field, object property, string name)
        {
            if (Get(property, "IsReadOnly") is true || EngineStateNames.Contains(name))
                return true;
            var clr = FindProp(owner, name);
            if (clr == null)
                return false;
            if (clr.DeclaringType != field.DeclaringType && clr.DeclaringType?.IsSubclassOf(field.DeclaringType!) == true
                && field.DeclaringType!.GetProperty(name, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly) != null)
                return true;
            return clr.GetCustomAttributes(typeof(System.ComponentModel.EditorBrowsableAttribute), true)
                .Any(a => ((System.ComponentModel.EditorBrowsableAttribute)a).State == System.ComponentModel.EditorBrowsableState.Never);
        }

        private static readonly HashSet<string> NotEditableNames = new()
        {
            "Views", "Children", "Parent", "Superview", "BindingContext", "ItemsSource", "ItemTemplate", "ItemTemplateType",
        };

        /// <summary>
        /// Values a person can type or pick: numbers, bool, string, enums and other value types (Thickness,
        /// LayoutOptions, CornerRadius, Size, Point...), and Color. Not: collections, templates, commands, delegates,
        /// control references, gradients / shadows / effects objects, raw object, and properties whose CLR property
        /// has no public setter.
        /// </summary>
        private static bool IsEditable(Type owner, string name, Type type)
        {
            if (NotEditableNames.Contains(name))
                return false;
            var plain = Nullable.GetUnderlyingType(type) ?? type;
            var typeOk = plain == typeof(string) || plain.IsValueType || plain.Name == "Color" || IsGradient(plain);
            if (!typeOk)
                return false;
            var clr = FindProp(owner, name);
            return clr == null || clr.SetMethod is { IsPublic: true };
        }

        /// <summary>
        /// An enum's cases in declaration order (its public static fields come in metadata order; Enum.GetNames sorts
        /// by value); for a [Flags] enum without its zero case (no toggle on means None). The four LayoutOptions an
        /// editor can set (the names ConvertTo reads back). Else null.
        /// </summary>
        private static string[] ChoicesOf(Type type)
        {
            var plain = Nullable.GetUnderlyingType(type) ?? type;
            if (plain.IsEnum)
            {
                var flags = plain.IsDefined(typeof(FlagsAttribute), false);
                return plain.GetFields(BindingFlags.Public | BindingFlags.Static)
                    .Where(f => !flags || Convert.ToInt64(f.GetRawConstantValue()) != 0)
                    .Select(f => f.Name).ToArray();
            }
            return plain.Name == "LayoutOptions" ? new[] { "Start", "Center", "End", "Fill" } : null;
        }

        /// <summary>Where an editor lists a property, from its name; unknown names are Behavior.</summary>
        private static string GroupOf(string name)
        {
            bool Starts(params string[] prefixes) => prefixes.Any(p => name.StartsWith(p, StringComparison.Ordinal));
            bool Has(params string[] parts) => parts.Any(p => name.Contains(p, StringComparison.Ordinal));

            if (Starts("Accessibility", "UpdateWhen"))
                return "Behavior";
            if (Starts("Translation", "Rotation", "Scale", "Skew", "Anchor", "Perspective", "AddTranslation"))
                return "Transform";
            if (Has("Text", "Font") || Starts("Line", "MaxLines", "Character", "Paragraph", "Placeholder", "Strikeout",
                    "Underline", "AutoSize", "FallbackCharacter"))
                return "Text";
            if (Has("Color", "Stroke", "Corner", "Opacity", "Shadow", "Blur", "Gradient", "Tint", "Aspect", "BlendMode") ||
                Starts("Background", "Brightness", "Contrast", "Saturation", "Gamma", "Lighten", "Darken", "Shape"))
                return "Look";
            if (Has("Width", "Height", "Margin", "Padding", "Options", "Spacing", "FillRatio", "Row", "Column", "PositionOffset",
                    "AlignContent") ||
                Starts("Split", "LockRatio", "ZIndex", "Left", "Top", "Right", "Bottom"))
                return "Layout";
            return "Behavior";
        }

        private static List<object> ReadProps(object control) =>
            PropRows(control).Select(r => (object)new
            {
                name = r.Def.Name, typeName = r.Def.ReturnType.FullName, value = r.Value, isSet = r.IsSet, isDefault = r.IsDefault,
                group = r.Def.Group, choices = r.Def.Choices, flags = r.Def.Flags,
            }).ToList();

        private static List<(PropDef Def, string Value, bool IsSet, bool? IsDefault)> PropRows(object control)
        {
            var props = new List<(PropDef, string, bool, bool?)>();
            foreach (var d in DefsOf(control.GetType()))
            {
                if (!d.Editable)
                    continue;
                string value;
                bool? isDefault = null;
                try
                {
                    var current = _getValue?.Invoke(control, new[] { d.Property });
                    value = Format(current);
                    if (DefaultOf(control, d.Property, out var fallback))
                        isDefault = Equals(current, fallback) || value == Format(fallback);
                }
                catch (Exception e)
                {
                    value = "<" + Unwrap(e).GetType().Name + ">";
                }

                var isSet = false;
                try
                {
                    isSet = _isSet?.Invoke(control, new[] { d.Property }) is true;
                }
                catch
                {
                }

                props.Add((d, value, isSet, isDefault));
            }
            return props;
        }

        private static readonly Dictionary<Type, (MethodInfo Creator, PropertyInfo Plain)> DefaultReaders = new();

        /// <summary>
        /// The bindable property's default for this control, as the framework computes it: its internal
        /// GetDefaultValue(bindable) (the default value creator, else the default value) in MAUI and the DrawnUi shim,
        /// else the public DefaultValue. False when neither is there.
        /// </summary>
        private static bool DefaultOf(object control, object property, out object value)
        {
            var type = property.GetType();
            if (!DefaultReaders.TryGetValue(type, out var reader))
                DefaultReaders[type] = reader = (
                    type.GetMethods(BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic)
                        .FirstOrDefault(m => m.Name == "GetDefaultValue" && m.GetParameters().Length == 1),
                    type.GetProperty("DefaultValue", BindingFlags.Instance | BindingFlags.Public));
            value = null;
            if (reader.Creator != null && reader.Creator.GetParameters()[0].ParameterType.IsInstanceOfType(control))
                value = reader.Creator.Invoke(property, new[] { control });
            else if (reader.Plain != null)
                value = reader.Plain.GetValue(property);
            else
                return false;
            return true;
        }

        private static string Format(object value)
        {
            var s = value switch
            {
                null => "",
                _ when IsColor(value.GetType()) => ColorHex(value),
                _ when IsGradient(value.GetType()) => GradientJson(value),
                // the framework's own structs (Point's ToString gives "{X=.. Y=..}", CornerRadius none): their numbers
                _ when value.GetType() is { IsValueType: true, IsEnum: false, IsPrimitive: false } t
                       && !(t.Namespace ?? "").StartsWith("System", StringComparison.Ordinal) && FormatStruct(value) is { } numbers => numbers,
                bool b => b ? "true" : "false",
                IFormattable f => f.ToString(null, CultureInfo.InvariantCulture),
                _ => value.ToString(),
            };
            if (s == value?.GetType().FullName)
                s = FormatStruct(value) ?? value.GetType().Name;
            return s.Length > 256 && !(value != null && IsGradient(value.GetType())) ? s[..256] + "…" : s; // a gradient goes whole: its text is read back
        }

        /// <summary>A color of any framework: MAUI Color (float fields), DrawnUi Color (float properties), SKColor,
        /// System.Drawing.Color.</summary>
        private static bool IsColor(Type type) =>
            type.Name is "Color" or "SKColor" && ChannelMember(type, "Red", "R") != null && ChannelMember(type, "Alpha", "A") != null;

        private static MemberInfo ChannelMember(Type type, string name, string shortName) =>
            (MemberInfo)type.GetProperty(name) ?? type.GetField(name) ?? (MemberInfo)type.GetProperty(shortName) ?? type.GetField(shortName);

        /// <summary>#AARRGGBB, uppercase, whatever the color type: what the designer's picker and code writer read.</summary>
        private static string ColorHex(object color)
        {
            int Channel(string name, string shortName)
            {
                var v = ChannelMember(color.GetType(), name, shortName) switch
                {
                    PropertyInfo p => p.GetValue(color),
                    FieldInfo f => f.GetValue(color),
                    _ => null,
                };
                return v switch
                {
                    float f => (int)Math.Round(Math.Clamp(f, 0f, 1f) * 255),
                    double d => (int)Math.Round(Math.Clamp(d, 0d, 1d) * 255),
                    _ => Convert.ToInt32(v ?? 0, CultureInfo.InvariantCulture),
                };
            }
            return $"#{Channel("Alpha", "A"):X2}{Channel("Red", "R"):X2}{Channel("Green", "G"):X2}{Channel("Blue", "B"):X2}";
        }

        /// <summary>A color of <paramref name="type"/> from #RGB, #ARGB, #RRGGBB or #AARRGGBB (alpha first, as the
        /// agent sends it); null when the text is not such a hex.</summary>
        private static object ColorFromHex(Type type, string text)
        {
            var hex = text.StartsWith('#') ? text[1..] : text;
            if (hex.Length is 3 or 4)
                hex = string.Concat(hex.Select(c => new string(c, 2)));
            if (hex.Length == 6)
                hex = "FF" + hex;
            if (hex.Length != 8 || !uint.TryParse(hex, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out var argb))
                return null;
            var channels = new Dictionary<string, int>
            {
                ["alpha"] = (int)(argb >> 24), ["red"] = (int)(argb >> 16) & 0xFF, ["green"] = (int)(argb >> 8) & 0xFF, ["blue"] = (int)argb & 0xFF,
                ["a"] = (int)(argb >> 24), ["r"] = (int)(argb >> 16) & 0xFF, ["g"] = (int)(argb >> 8) & 0xFF, ["b"] = (int)argb & 0xFF,
            };
            // constructors and factories whose 4 parameters are named after the channels: the order never matters
            var candidates = type.GetConstructors().Cast<MethodBase>()
                .Concat(type.GetMethods(BindingFlags.Public | BindingFlags.Static).Where(m => m.Name is "FromArgb" or "FromRgba" && type.IsAssignableFrom(m.ReturnType)));
            foreach (var method in candidates)
            {
                var ps = method.GetParameters();
                if (ps.Length != 4 || !ps.All(q => channels.ContainsKey(q.Name.ToLowerInvariant())))
                    continue;
                var t = ps[0].ParameterType;
                if (ps.Any(q => q.ParameterType != t))
                    continue;
                var args = ps.Select(q => channels[q.Name.ToLowerInvariant()]).Select(v =>
                    t == typeof(float) ? (object)(v / 255f) : t == typeof(double) ? v / 255d : Convert.ChangeType(v, t, CultureInfo.InvariantCulture)).ToArray();
                return method is ConstructorInfo ctor ? ctor.Invoke(args) : method.Invoke(null, args);
            }
            return null;
        }

        /// <summary>Types without their own ToString, in the text <see cref="ConvertTo"/> reads back:
        /// a color as #AARRGGBB (ToHex), a Thickness-like value as "l,t,r,b".</summary>
        private static string FormatStruct(object value)
        {
            var type = value.GetType();
            // MAUI's Color: #AARRGGBB, which Color.Parse reads back (its ToHex drops the alpha)
            if (type.GetMethod("ToArgbHex", new[] { typeof(bool) }) is { } argb && argb.ReturnType == typeof(string))
                return (string)argb.Invoke(value, new object[] { true });
            if (type.GetMethod("ToHex", Type.EmptyTypes) is { } hex && hex.ReturnType == typeof(string))
                return (string)hex.Invoke(value, null);
            var sides = new[] { "Left", "Top", "Right", "Bottom" }.Select(n => Get(value, n)).ToArray();
            if (sides.All(v => v is double or float))
                return string.Join(",", sides.Select(v => Convert.ToDouble(v).ToString(CultureInfo.InvariantCulture)));
            if (Get(value, "Alignment") is Enum alignment)
                return alignment.ToString(); // LayoutOptions.Center
            return NumbersInConstructorOrder(value);
        }

        /// <summary>
        /// A struct of numbers (CornerRadius, Point, Size, Rect...) as its values in the order of the constructor whose
        /// parameters are named after them, the text <see cref="ConvertTo"/> hands back to that constructor and XAML
        /// reads too (CornerRadius: topLeft,topRight,bottomLeft,bottomRight). One number when all are equal and the
        /// type has a one-number constructor. Null for anything else.
        /// </summary>
        private static string NumbersInConstructorOrder(object value)
        {
            var type = value.GetType();
            if (!type.IsValueType)
                return null;
            if (!StructShapes.TryGetValue(type, out var shape))
                StructShapes[type] = shape = StructShape(type); // looked up once per type
            if (shape is not { } s)
                return null;
            var values = s.Props.Select(p => Convert.ToDouble(p.GetValue(value), CultureInfo.InvariantCulture)).ToArray();
            if (s.OneNumber && values.Distinct().Count() == 1)
                return values[0].ToString(CultureInfo.InvariantCulture);
            return string.Join(",", values.Select(v => v.ToString(CultureInfo.InvariantCulture)));
        }

        private static readonly Dictionary<Type, (PropertyInfo[] Props, bool OneNumber)?> StructShapes = new();

        /// <summary>The properties named after the parameters of the widest all-number constructor, in its order,
        /// and whether a one-number constructor exists; null when there is no such constructor.</summary>
        private static (PropertyInfo[] Props, bool OneNumber)? StructShape(Type type)
        {
            static bool IsNumber(Type t) => t == typeof(double) || t == typeof(float) || t == typeof(int);
            foreach (var ctor in type.GetConstructors().OrderByDescending(c => c.GetParameters().Length))
            {
                var ps = ctor.GetParameters();
                if (ps.Length < 2 || !ps.All(p => IsNumber(p.ParameterType)))
                    continue;
                var props = ps.Select(p => type.GetProperty(p.Name ?? "", BindingFlags.Public | BindingFlags.Instance | BindingFlags.IgnoreCase)).ToArray();
                if (props.Any(p => p == null || !IsNumber(p.PropertyType)))
                    continue;
                return (props, type.GetConstructors().Any(c => c.GetParameters() is { Length: 1 } one && IsNumber(one[0].ParameterType)));
            }
            return null;
        }

        private static void SetProperty(long node, string name, string text)
        {
            var control = Find(node);
            SetValueText(control, name, text);
            Call(control, "Invalidate");
            Call(_canvas, "Update");
        }

        /// <summary>A bindable property from the text the designer sends (as in setProperty). UI thread.</summary>
        private static void SetValueText(object control, string name, string text)
        {
            var def = DefsOf(control.GetType()).FirstOrDefault(d => d.Name == name || d.Name + "Property" == name)
                      ?? throw new InvalidOperationException($"{control.GetType().Name} has no bindable property {name}");
            if (IsGradient(def.ReturnType))
                SetGradient(control, def, text);
            else
                _setValue.Invoke(control, new[] { def.Property, ConvertTo(text, def.ReturnType) });
        }

        /// <summary>Text to a property value: primitives, enums, Parse(string) types (Color), static members by name
        /// (LayoutOptions.Fill), numeric constructors from "a,b,c,d" (Thickness), then the type's TypeConverter.</summary>
        private static object ConvertTo(string text, Type type)
        {
            var nullable = Nullable.GetUnderlyingType(type);
            if (nullable != null)
            {
                if (string.IsNullOrWhiteSpace(text) || text == "null")
                    return null;
                type = nullable;
            }

            var inv = CultureInfo.InvariantCulture;
            if (type == typeof(string) || type == typeof(object))
                return text;
            if (type.IsEnum)
                return text.Trim().Length == 0 && type.IsDefined(typeof(FlagsAttribute), false)
                    ? Enum.ToObject(type, 0) // "" = every flag off
                    : Enum.Parse(type, text.Trim(), true);
            if (type.IsPrimitive || type == typeof(decimal))
                return Convert.ChangeType(text.Trim(), type, inv);
            if (!type.IsValueType && (text == "null" || text.Length == 0))
                return null;

            var trimmed = text.Trim();
            if (IsColor(type) && ColorFromHex(type, trimmed) is { } color)
                return color;
            var member = (MemberInfo)type.GetProperty(trimmed, BindingFlags.Public | BindingFlags.Static | BindingFlags.IgnoreCase)
                         ?? type.GetField(trimmed, BindingFlags.Public | BindingFlags.Static | BindingFlags.IgnoreCase);
            if (member is PropertyInfo sp && type.IsAssignableFrom(sp.PropertyType))
                return sp.GetValue(null);
            if (member is FieldInfo sf && type.IsAssignableFrom(sf.FieldType))
                return sf.GetValue(null);

            var parse = type.GetMethod("Parse", BindingFlags.Public | BindingFlags.Static, new[] { typeof(string) });
            if (parse != null && type.IsAssignableFrom(parse.ReturnType))
            {
                try
                {
                    return parse.Invoke(null, new object[] { trimmed });
                }
                catch when (trimmed.Contains(','))
                {
                    // "1,2,3,4" for a type whose Parse wants something else: try the constructors below
                }
            }

            var parts = trimmed.Split(',', StringSplitOptions.TrimEntries);
            foreach (var ctor in type.GetConstructors())
            {
                var ps = ctor.GetParameters();
                if (ps.Length != parts.Length || ps.Any(p => p.ParameterType != typeof(double) && p.ParameterType != typeof(float)))
                    continue;
                return ctor.Invoke(ps.Select((p, i) => Convert.ChangeType(double.Parse(parts[i], inv), p.ParameterType, inv)).ToArray());
            }

            var converter = System.ComponentModel.TypeDescriptor.GetConverter(type);
            if (converter.CanConvertFrom(typeof(string)))
                return converter.ConvertFromInvariantString(trimmed);

            throw new InvalidOperationException($"cannot convert \"{text}\" to {type.FullName}");
        }

        #endregion

        #region Screens

        private static object FindShell()
        {
            if (Get(_canvas, "Views") is not IEnumerable roots)
                return null;
            var queue = new Queue<object>(roots.Cast<object>());
            var seen = new HashSet<object>(ReferenceEqualityComparer.Instance);
            while (queue.Count > 0)
            {
                var c = queue.Dequeue();
                if (!seen.Add(c))
                    continue;
                for (var t = c.GetType(); t != null; t = t.BaseType)
                    if (t.Name == "SkiaShell")
                        return c;
                foreach (var child in ChildrenOf(c))
                    queue.Enqueue(child);
            }
            return null;
        }

        private static List<object> ReadScreens()
        {
            if (FindShell() is not { } shell || Get(shell, "Routes") is not IDictionary routes)
                return _host.Screens();
            var titles = Get(shell, "Titles") as IDictionary;
            return routes.Keys.Cast<object>().Select(k => (object)new
            {
                route = k.ToString(),
                title = titles != null && titles.Contains(k) ? titles[k]?.ToString() : null,
            }).ToList();
        }

        /// <summary>
        /// Opens a screen the way a user would reach it: nothing when it is already on top; a tab's route selects that
        /// tab at its root; the shell's root route goes back to the root instead of pushing a second copy of it;
        /// any other route is pushed (GoToAsync). Which route is the root is learned after the fact: when a pushed
        /// page has the root content's type, the agent pops back to the root (disposing that duplicate) and
        /// remembers the route. Pages are never built just to learn their type.
        /// </summary>
        private static Task OpenScreen(string route)
        {
            var shell = FindShell();
            if (shell == null)
                return _host.OpenScreen(route) ?? throw new InvalidOperationException("the app has no SkiaShell or Shell");
            var name = route.Split('?')[0];
            if (Get(shell, "Route") as string == route)
                return Task.CompletedTask;

            if (Get(shell, "Tabs") is IList { Count: > 0 } tabs)
            {
                for (var index = 0; index < tabs.Count; index++)
                    if (Get(tabs[index], "Route") as string == name)
                        return SelectTabAtRoot(shell, index);
            }
            else if (RootRoutes.Contains(name))
                return Invoke(shell, "PopToRootAsync");

            return PushScreen(shell, route, name);
        }

        /// <summary>Routes found to build the shell's root page.</summary>
        private static readonly HashSet<string> RootRoutes = new();

        private static async Task PushScreen(object shell, string route, string name)
        {
            var go = shell.GetType().GetMethods(BindingFlags.Public | BindingFlags.Instance)
                .Where(m => m.Name == "GoToAsync" && m.GetParameters().FirstOrDefault()?.ParameterType == typeof(string))
                .OrderBy(m => m.GetParameters().Length).First();
            var args = go.GetParameters().Select((p, i) => i == 0 ? route : p.HasDefaultValue ? p.DefaultValue : null).ToArray();
            if (go.Invoke(shell, args) is Task pushing)
                await pushing;

            if (Get(shell, "Tabs") is IList { Count: > 0 } || Get(shell, "RootContent") is not { } root)
                return;
            if (Call(shell, "GetTopmostView") is { } top && !ReferenceEquals(top, root) && top.GetType() == root.GetType())
            {
                RootRoutes.Add(name);
                await Invoke(shell, "PopToRootAsync");
            }
        }

        private static async Task SelectTabAtRoot(object shell, int index)
        {
            var select = shell.GetType().GetMethod("SelectTabAsync", new[] { typeof(int) });
            if (select?.Invoke(shell, new object[] { index }) is Task selecting)
                await selecting;
            await Invoke(shell, "PopTabToRootAsync");
        }

        private static Task Invoke(object target, string method) =>
            target.GetType().GetMethod(method, BindingFlags.Public | BindingFlags.Instance, Type.EmptyTypes)?.Invoke(target, null) as Task
            ?? Task.CompletedTask;


        #endregion

        #region Snapshot and text preview

        /// <summary>
        /// What the control shows, for a tree row: a label's or button's text, an editor's text or else its
        /// placeholder; one line (whitespace runs become one space), at most 40 characters. Null for others.
        /// </summary>
        private static string TextPreview(object control)
        {
            var type = control.GetType();
            if (!IsNamed(type, "SkiaLabel") && !IsNamed(type, "SkiaButton") && !IsNamed(type, "SkiaEditor"))
                return null;
            var text = Get(control, "Text") as string;
            if (string.IsNullOrWhiteSpace(text) && IsNamed(type, "SkiaEditor"))
                text = Get(control, "PlaceholderText") as string;
            if (string.IsNullOrWhiteSpace(text))
                return null;
            text = string.Join(' ', text.Split((char[])null, StringSplitOptions.RemoveEmptyEntries));
            return text.Length > 40 ? text[..39] + "…" : text;
        }

        private static readonly List<TaskCompletionSource<(string Png, uint Width, uint Height)>> SnapshotWaiters = new();

        /// <summary>
        /// The canvas as base64 PNG. While the window presents frames: DrawnView.TakeScreenShot, the next frame's
        /// own surface image, encoded on the render thread (works on the GPU). When it is hidden (minimized,
        /// display off: no frame within 300 ms), the canvas draws a frame into a raster surface instead, on Windows only
        /// (see <see cref="OffscreenAllowed"/>); elsewhere the snapshot waits up to 3 s for the canvas's own frame.
        /// </summary>
        private static async Task<(string Png, uint Width, uint Height)> Snapshot()
        {
            if (!await Ui(WindowHidden))
            {
                var waiter = new TaskCompletionSource<(string, uint, uint)>(TaskCreationOptions.RunContinuationsAsynchronously);
                await Ui(() =>
                {
                    SnapshotWaiters.Add(waiter);
                    if (SnapshotWaiters.Count > 1)
                        return; // a frame is already asked for
                    var take = _canvas.GetType().GetMethods(BindingFlags.Public | BindingFlags.Instance)
                        .FirstOrDefault(m => m.Name == "TakeScreenShot" && m.GetParameters().Length == 1)
                        ?? throw new InvalidOperationException("this DrawnUi canvas has no TakeScreenShot");
                    var callbackType = take.GetParameters()[0].ParameterType; // Action<SKImage>
                    var image = Expression.Parameter(callbackType.GetGenericArguments()[0]);
                    var callback = Expression.Lambda(callbackType,
                        Expression.Call(typeof(AgentHost).GetMethod(nameof(OnScreenshot), BindingFlags.NonPublic | BindingFlags.Static),
                            Expression.Convert(image, typeof(object))), image).Compile();
                    take.Invoke(_canvas, new object[] { callback });
                    Call(_canvas, "Update");
                });
                // Off Windows the canvas's own render thread draws it: a slow frame (a first Vulkan frame) is
                // waited for, never drawn by the agent.
                if (await Task.WhenAny(waiter.Task, Task.Delay(OffscreenAllowed ? 300 : 3000)) == waiter.Task)
                    return await waiter.Task;
                await Ui(() => SnapshotWaiters.Remove(waiter)); // no frame: the window does not present
            }

            if (!OffscreenAllowed)
                throw new InvalidOperationException("the app draws no frame (is it in the background?): bring it to the front for a snapshot");
            await SettleOffscreen();
            var frame = await Ui(() => RenderOffscreen(keepImage: true));
            return Encode(frame); // PNG encoding off the UI thread
        }

        /// <summary>The frame's image, on the thread that drew it: only the GPU read-back happens there (a raster
        /// copy); the PNG is encoded on the pool.</summary>
        private static void OnScreenshot(object image)
        {
            var waiters = SnapshotWaiters.ToArray();
            SnapshotWaiters.Clear();
            object raster = null;
            try
            {
                raster = Call(image, "ToRasterImage") ?? throw new InvalidOperationException("the frame could not be read");
            }
            catch (Exception e)
            {
                foreach (var waiter in waiters)
                    waiter.TrySetException(Unwrap(e));
                return;
            }
            finally
            {
                if (!ReferenceEquals(raster, image))
                    (image as IDisposable)?.Dispose();
            }
            Task.Run(() =>
            {
                try
                {
                    var result = Encode(raster);
                    foreach (var waiter in waiters)
                        waiter.TrySetResult(result);
                }
                catch (Exception e)
                {
                    foreach (var waiter in waiters)
                        waiter.TrySetException(Unwrap(e));
                }
            });
        }

        /// <summary>PNG of an SKImage, base64; disposes the image.</summary>
        private static (string Png, uint Width, uint Height) Encode(object image)
        {
            try
            {
                var data = Call(image, "Encode") ?? throw new InvalidOperationException("the frame could not be encoded");
                var bytes = (byte[])Call(data, "ToArray");
                (data as IDisposable)?.Dispose();
                return (Convert.ToBase64String(bytes), Convert.ToUInt32(Get(image, "Width")), Convert.ToUInt32(Get(image, "Height")));
            }
            finally
            {
                (image as IDisposable)?.Dispose();
            }
        }

        #endregion

        #region Frames while the window does not present

        /// <summary>Drawing a frame into the agent's own surface: the highlight stays out of it.</summary>
        private static bool _offscreen;

        private static MethodInfo _renderExternal, _surfaceCreate;
        private static Type _imageInfoType;
        private static Func<long> _nanos;

        /// <summary>
        /// The agent may draw the canvas itself (a frame into its own raster surface, on the UI thread) only on
        /// Windows, where the hosts draw on the UI thread or stop drawing while the window does not present. On
        /// Android, iOS and Mac Catalyst the canvas has its own render thread (Vulkan, OpenGL, Metal): a frame drawn
        /// from the UI thread would race it, and GPU caches made on its context from another thread crash the
        /// driver. There the agent waits for the canvas's own frames.
        /// </summary>
        private static bool OffscreenAllowed => OperatingSystem.IsWindows();

        private static bool WindowHidden()
        {
            if (_host.CanvasHidden(_canvas))
                return true;
            if (!OperatingSystem.IsWindows())
                return false;
            var hwnd = _host.WindowHandle(_canvas);
            return hwnd != IntPtr.Zero && (IsIconic(hwnd) || !IsWindowVisible(hwnd));
        }

        private static bool CanvasDirty() => Get(_canvas, "IsDirty") is true;

        private static long FrameNumber() => Convert.ToInt64(Get(_canvas, "FrameNumber") ?? 0L);

        /// <summary>
        /// DrawnUI measures and arranges inside the draw pass: after a change (setProperty, openScreen, hot reload)
        /// rects and visibility are stale until a frame is drawn. When the canvas waits for a frame and the window
        /// does not draw one within 150 ms (hidden, display off), the agent draws frames itself on Windows; elsewhere
        /// it waits up to 1.5 s for the canvas's own frame (see <see cref="OffscreenAllowed"/>) and answers with what
        /// the last frame laid out.
        /// </summary>
        private static async Task EnsureCurrent()
        {
            var (dirty, hidden, frame) = await Ui(() => (CanvasDirty(), WindowHidden(), FrameNumber()));
            if (!dirty || (hidden && !OffscreenAllowed))
                return;
            if (!hidden)
            {
                var wait = OffscreenAllowed ? 150 : 1500;
                for (var waited = 0; waited < wait; waited += 15)
                {
                    await Task.Delay(15);
                    if (await Ui(FrameNumber) != frame)
                        return; // the window presents
                }
            }
            if (OffscreenAllowed)
                await SettleOffscreen();
        }

        /// <summary>
        /// Draws offscreen frames, 16 ms apart in real time, while the canvas still asks for more (a page transition,
        /// an animation), at most 1.5 s, so layout and running transitions reach their state as on screen.
        /// </summary>
        private static async Task SettleOffscreen()
        {
            var clock = System.Diagnostics.Stopwatch.StartNew();
            while (await Ui(() =>
                   {
                       RenderOffscreen(keepImage: false);
                       return CanvasDirty();
                   }) && clock.ElapsedMilliseconds < 1500)
                await Task.Delay(16);
        }

        /// <summary>
        /// One canvas frame into a raster surface of the canvas size (DrawnView.RenderExternalSurface, the path the
        /// WPF host draws through): layout, animators and the render tree advance as for a frame on screen.
        /// Returns the frame's image when asked, else null. UI thread only.
        /// </summary>
        private static object RenderOffscreen(bool keepImage)
        {
            if (_renderExternal == null)
            {
                const BindingFlags all = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance;
                _renderExternal = _canvas.GetType().GetMethods(all).FirstOrDefault(m => m.Name == "RenderExternalSurface" && m.GetParameters().Length == 3)
                                  ?? _canvas.GetType().GetMethods(all).FirstOrDefault(m => m.Name == "OnDrawSurface" && m.GetParameters().Length == 2)
                                  ?? throw new InvalidOperationException("this DrawnUi canvas cannot draw into a surface of the agent");
                var surfaceType = _renderExternal.GetParameters()[0].ParameterType;
                _imageInfoType = surfaceType.Assembly.GetType("SkiaSharp.SKImageInfo");
                _surfaceCreate = surfaceType.GetMethod("Create", new[] { _imageInfoType });
                var super = _canvas.GetType().Assembly.GetType("DrawnUi.Draw.Super");
                var nanos = super?.GetMethod("GetCurrentTimeNanos", BindingFlags.Public | BindingFlags.Static);
                _nanos = nanos != null ? () => (long)nanos.Invoke(null, null) : () => System.Diagnostics.Stopwatch.GetTimestamp() * 100;
            }

            var (pixelWidth, pixelHeight) = _host.CanvasPixelSize(_canvas);
            var width = Math.Max(1, (int)Math.Round(pixelWidth));
            var height = Math.Max(1, (int)Math.Round(pixelHeight));
            var surface = _surfaceCreate.Invoke(null, new[] { Activator.CreateInstance(_imageInfoType, width, height) })
                          ?? throw new InvalidOperationException("no raster surface");
            var rect = Activator.CreateInstance(_renderExternal.GetParameters()[1].ParameterType, 0f, 0f, (float)width, (float)height);
            try
            {
                _offscreen = true;
                _renderExternal.Invoke(_canvas, _renderExternal.GetParameters().Length == 3 ? new[] { surface, rect, _nanos() } : new[] { surface, rect });
                _offscreen = false;
                if (!keepImage)
                    return null;
                Call(Get(surface, "Canvas"), "Flush");
                return Call(surface, "Snapshot");
            }
            finally
            {
                _offscreen = false;
                (surface as IDisposable)?.Dispose();
            }
        }

        #endregion

        #region Focus

        [System.Runtime.InteropServices.DllImport("user32.dll")]
        private static extern bool SetForegroundWindow(IntPtr hwnd);

        [System.Runtime.InteropServices.DllImport("user32.dll")]
        private static extern IntPtr GetForegroundWindow();

        [System.Runtime.InteropServices.DllImport("user32.dll")]
        private static extern bool ShowWindow(IntPtr hwnd, int command);

        [System.Runtime.InteropServices.DllImport("user32.dll")]
        private static extern bool IsIconic(IntPtr hwnd);

        [System.Runtime.InteropServices.DllImport("user32.dll")]
        private static extern bool IsWindowVisible(IntPtr hwnd);

        [System.Runtime.InteropServices.DllImport("user32.dll")]
        private static extern int GetWindowLong(IntPtr hwnd, int index);

        [System.Runtime.InteropServices.DllImport("user32.dll")]
        private static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int cx, int cy, uint flags);

        private const int SwMinimize = 6, SwShow = 5, SwRestore = 9;

        /// <summary>
        /// Brings the window hosting the canvas to the front (the designer's app pill). Windows only (WPF, MAUI Windows).
        /// A minimized window is restored to its previous state and shown. SetForegroundWindow works when
        /// the designer granted the foreground (AllowSetForegroundWindow with the user's click); otherwise a Topmost
        /// toggle raises it, and when the foreground still belongs to another app (an editor inside VS Code, which
        /// cannot grant it), minimize + restore, which Windows lets a window do to take the foreground.
        /// </summary>
        private static void BringToFront()
        {
            if (!OperatingSystem.IsWindows())
                return; // a phone shows one app: nothing to bring forward
            var hwnd = _host.WindowHandle(_canvas);
            if (hwnd == IntPtr.Zero)
                throw new InvalidOperationException("no window hosts the canvas");

            if (IsIconic(hwnd))
                ShowWindow(hwnd, SwRestore); // back to normal or maximized, as it was
            else if (!IsWindowVisible(hwnd))
                ShowWindow(hwnd, SwShow);
            if (SetForegroundWindow(hwnd) && GetForegroundWindow() == hwnd)
                return;

            // raise it above the others: topmost on, then back off unless the app made it topmost itself
            const uint noMoveSizeActivate = 0x0001 | 0x0002 | 0x0010;
            var wasTopmost = (GetWindowLong(hwnd, -20) & 0x8) != 0; // GWL_EXSTYLE, WS_EX_TOPMOST
            SetWindowPos(hwnd, new IntPtr(-1), 0, 0, 0, 0, noMoveSizeActivate);
            if (!wasTopmost)
                SetWindowPos(hwnd, new IntPtr(-2), 0, 0, 0, 0, noMoveSizeActivate);
            if (GetForegroundWindow() == hwnd)
                return;

            ShowWindow(hwnd, SwMinimize);
            ShowWindow(hwnd, SwRestore);
            SetForegroundWindow(hwnd);
        }

        #endregion

        #region Pick mode

        private static bool _picking;
        private static WeakReference<object> _pickRootRef;
        private static FieldInfo _onGestures;
        // the app's own OnGestures, put back after picking: held only as long as its root lives
        private static readonly ConditionalWeakTable<object, StrongBox<object>> AppOnGestures = new();

        private static object _pickRoot
        {
            get => _pickRootRef != null && _pickRootRef.TryGetTarget(out var root) ? root : null;
            set => _pickRootRef = value == null ? null : new WeakReference<object>(value);
        }
        private static IDisposable _pickEscape;
        private static long? _pickHover;
        private static float _pointerX, _pointerY;
        private static bool _hoverQueued, _pickQueued;

        /// <summary>
        /// The designer's element picker. While on, the root control's OnGestures delegate (the first thing its
        /// ProcessGestures asks, before any child) is the agent's: every pointer event stops there, so the app sees
        /// none of them; the wheel passes so content can still be scrolled to what is wanted. A mouse move highlights
        /// the control under the pointer, a release picks it (Picked, request 0), keeps it highlighted and ends the
        /// mode. Esc cancels (Picked with no node). The app's own OnGestures is put back as it was.
        /// With no frames (window hidden) no input arrives: the mode just waits.
        /// </summary>
        private static void SetPickMode(bool on)
        {
            if (on == _picking)
                return;
            if (on)
            {
                var root = (Get(_canvas, "Views") as IList)?.Cast<object>().FirstOrDefault()
                           ?? throw new InvalidOperationException("the canvas has no content");
                _onGestures = FindField(root.GetType(), "OnGestures") ?? throw new InvalidOperationException("this DrawnUi has no OnGestures");
                var invoke = _onGestures.FieldType.GetMethod("Invoke");
                var parameters = invoke.GetParameters().Select(p => Expression.Parameter(p.ParameterType)).ToArray();
                var body = Expression.Convert(
                    Expression.Call(typeof(AgentHost).GetMethod(nameof(OnPickGesture), BindingFlags.NonPublic | BindingFlags.Static),
                        Expression.Convert(parameters[0], typeof(object))),
                    invoke.ReturnType);
                AppOnGestures.AddOrUpdate(root, new StrongBox<object>(_onGestures.GetValue(root)));
                _onGestures.SetValue(root, Expression.Lambda(_onGestures.FieldType, body, parameters).Compile());
                _pickRoot = root;
                _pickHover = null;
                _picking = true;
                _pickEscape = _host.HookEscape(_canvas, OnPickEscape);
                return;
            }

            _picking = false;
            _placeType = null;
            _placeLine = null;
            if (_pickRoot is { } pickRoot && _onGestures != null && AppOnGestures.TryGetValue(pickRoot, out var saved))
            {
                _onGestures.SetValue(pickRoot, saved.Value);
                AppOnGestures.Remove(pickRoot);
            }
            _pickRoot = null;
            _pickEscape?.Dispose();
            _pickEscape = null;
            if (_pickHover != null)
                SetHighlight(null);
            _pickHover = null;
        }

        /// <summary>The root's OnGestures while picking: returns the root (consumed) or null (let it pass).</summary>
        private static object OnPickGesture(object args)
        {
            try
            {
                return PickGesture(args);
            }
            catch (Exception e)
            {
                ReportHandlerError("pick gesture", e);
                return null;
            }
        }

        private static object PickGesture(object args)
        {
            if (!_picking)
                return null;
            var kind = Get(args, "Type")?.ToString();
            if (kind == "Wheel")
                return null;
            if (kind is "Pointer" or "Up" && Get(Get(args, "Event"), "Location") is { } location)
            {
                _pointerX = Convert.ToSingle(Get(location, "X"));
                _pointerY = Convert.ToSingle(Get(location, "Y"));
                // after the frame's input pass: the tree walk stays out of the gesture dispatch
                if (kind == "Up" && !_pickQueued)
                {
                    _pickQueued = true;
                    _host.Post(PickAtPointer, low: true);
                }
                else if (kind == "Pointer" && !_hoverQueued)
                {
                    _hoverQueued = true;
                    _host.Post(HoverAtPointer, low: true);
                }
            }
            return _pickRoot; // the press, its moves, Up, Tapped, long press, context menu: none reach the app
        }

        private static void HoverAtPointer()
        {
            _hoverQueued = false;
            if (!_picking || _pickQueued)
                return;
            // place mode: the layout a click would put the control into, and the line where; pick mode: the control
            var place = _placeType != null ? PlaceTarget(_pointerX, _pointerY) : default;
            var node = _placeType != null ? place.Container : Pick(_pointerX, _pointerY);
            var lineMoved = !(place.Line ?? Array.Empty<float>()).SequenceEqual(_placeLine ?? Array.Empty<float>());
            _placeLine = place.Line;
            if (node != _pickHover)
            {
                _pickHover = node;
                SetHighlight(node);
            }
            else if (lineMoved)
                Call(_canvas, "Update");
        }

        private static void PickAtPointer()
        {
            _pickQueued = false;
            if (!_picking)
                return;
            if (_placeType != null)
            {
                PlaceAtPointer();
                return;
            }
            var node = Pick(_pointerX, _pointerY);
            _pickHover = null; // the picked control keeps its highlight
            SetPickMode(false);
            SetHighlight(node);
            _ = Send(new { type = "picked", request = 0, node });
        }

        /// <summary>The click in place mode: the control goes where <see cref="PlaceTarget"/> says. UI thread.</summary>
        private static void PlaceAtPointer()
        {
            var type = _placeType;
            if (PlaceTarget(_pointerX, _pointerY).Container == null && PageWrapper != null)
            {
                _ = SendError(0, OutsideSnippet); // stays in place mode for the next click
                return;
            }
            _pickHover = null;
            SetPickMode(false);
            PlaceHere(type, _pointerX, _pointerY);
        }

        /// <summary>A control of <paramref name="type"/> put where a place-mode click at the point puts it: inserted
        /// (request 0), then the tree. UI thread.</summary>
        private static void PlaceHere(string type, float x, float y)
        {
            var (container, index, _) = PlaceTarget(x, y);
            if (container == null)
            {
                _ = SendError(0, PageWrapper != null ? OutsideSnippet : "no control can go here");
                _ = Send(new { type = "inserted", request = 0, node = (long?)null, parent = (long?)null, index = (int?)null });
                return;
            }
            try
            {
                var (node, at, _) = InsertControl(container.Value, index, type);
                SetHighlight(node);
                _ = Send(new { type = "inserted", request = 0, node = (long?)node, parent = container, index = (int?)at });
                _ = TreeAfterChange();
            }
            catch (Exception e)
            {
                _ = SendError(0, Unwrap(e).Message);
                _ = Send(new { type = "inserted", request = 0, node = (long?)null, parent = container, index });
            }
        }

        private static async Task TreeAfterChange()
        {
            try
            {
                await EnsureCurrent();
                await SendTree();
            }
            catch (Exception e)
            {
                ReportHandlerError("tree after a change", e);
            }
        }

        private static bool OnPickEscape()
        {
            try
            {
                if (_placeType != null)
                {
                    SetPickMode(false);
                    _ = Send(new { type = "inserted", request = 0, node = (long?)null, parent = (long?)null, index = (int?)null });
                    return true;
                }
                SetPickMode(false);
                _ = Send(new { type = "picked", request = 0, node = (long?)null });
                return true;
            }
            catch (Exception e)
            {
                ReportHandlerError("Esc", e);
                return false;
            }
        }

        #endregion

        #region Highlight

        private static WeakReference<object> _highlightTarget;
        /// <summary>Place mode: the line (x, y, width, height) where the control would go in the boxed stack, row or wrap.</summary>
        private static float[] _placeLine;
        private static Delegate _willDraw, _wasDrawn;
        private static WeakReference<object> _hookedCanvas; // the canvas the frame hooks went on
        private static bool _frameStarted;
        private static FieldInfo _frameContext;
        private static object _paint;
        private static MethodInfo _drawRect;

        /// <summary>
        /// Draws a 2 px box around the control on top of every frame the app draws anyway: after the canvas
        /// drew a frame (DrawnView.WasDrawn, still inside the frame, before the host presents it) onto that
        /// frame's canvas. Nothing is added to the app's tree and no frame is requested beyond one Update when the
        /// highlight is set, moved or cleared; the box follows scrolls and animations on the frames they draw.
        /// </summary>
        private static void SetHighlight(long? node)
        {
            var target = node == null ? null : Find(node.Value);
            if (target == null && _highlightTarget == null)
                return;

            _highlightTarget = target == null ? null : new WeakReference<object>(target);
            if (target != null && _wasDrawn == null && _canvas is { } canvas)
            {
                _frameContext = FindField(canvas.GetType(), "Context") ?? throw new InvalidOperationException("this DrawnUi canvas keeps no frame context");
                _willDraw = Subscribe("WillDraw", nameof(OnWillDraw));
                _wasDrawn = Subscribe("WasDrawn", nameof(OnWasDrawn));
                _hookedCanvas = new WeakReference<object>(canvas);
            }
            else if (target == null)
            {
                try
                {
                    if (_hookedCanvas != null && _hookedCanvas.TryGetTarget(out var hooked))
                    {
                        hooked.GetType().GetEvent("WillDraw")?.RemoveEventHandler(hooked, _willDraw);
                        hooked.GetType().GetEvent("WasDrawn")?.RemoveEventHandler(hooked, _wasDrawn);
                    }
                }
                catch
                {
                    // a disposed canvas: its events are gone with it
                }
                _willDraw = _wasDrawn = null;
                _hookedCanvas = null;
            }

            if (_canvas is not { } shown)
                return;
            // a retained surface keeps last frame's pixels: repaint the content so the old box goes
            if (Get(shown, "RenderingMode")?.ToString() == "AcceleratedRetained" && Get(shown, "Views") is IList { Count: > 0 } roots)
                Call(roots[0], "Update");
            Call(shown, "Update");
        }

        private static Delegate Subscribe(string eventName, string handler)
        {
            var info = _canvas.GetType().GetEvent(eventName) ?? throw new InvalidOperationException("this DrawnUi canvas has no " + eventName);
            var ps = info.EventHandlerType.GetMethod("Invoke").GetParameters().Select(p => Expression.Parameter(p.ParameterType)).ToArray();
            var call = Expression.Call(typeof(AgentHost).GetMethod(handler, BindingFlags.NonPublic | BindingFlags.Static));
            var d = Expression.Lambda(info.EventHandlerType, call, ps).Compile();
            info.AddEventHandler(_canvas, d);
            return d;
        }

        private static void OnWillDraw() => _frameStarted = true;

        private static void OnWasDrawn()
        {
            if (!_frameStarted) // WasDrawn without a drawn frame: the context is not this frame's
                return;
            _frameStarted = false;
            if (_offscreen)
                return; // the agent's own frame (snapshot, layout): no highlight in it
            try
            {
                DrawHighlight(_frameContext.GetValue(_canvas));
            }
            catch
            {
                // never into the app's frame
            }
        }

        private static FieldInfo FindField(Type type, string name)
        {
            for (var t = type; t != null; t = t.BaseType)
                if (t.GetField(name, BindingFlags.NonPublic | BindingFlags.Public | BindingFlags.Instance | BindingFlags.DeclaredOnly) is { } f)
                    return f;
            return null;
        }

        private const uint Magenta = 0xFFFF00FF;
        private static uint _boxColor = Magenta, _paintColor; // ARGB: what the designer asked, what the paint has
        private static Type _skColor;

        /// <summary>The box color the designer sent (#AARRGGBB or #RRGGBB, its Selection color); magenta when there is
        /// none or it cannot be read. Read once per highlight message.</summary>
        private static uint BoxColor(string text)
        {
            var hex = text?.TrimStart('#');
            if (hex == null || hex.Length is not (6 or 8)
                || !uint.TryParse(hex, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out var value))
                return Magenta;
            return hex.Length == 6 ? 0xFF000000 | value : value;
        }

        private static bool DrawHighlight(object context)
        {
            try
            {
                if (_highlightTarget == null || !_highlightTarget.TryGetTarget(out var target))
                    return false;
                var r = Rect(Call(target, "GetAccessibilityPixelRect"));
                if (r[2] <= 0 || r[3] <= 0)
                    return false;
                var canvas = Get(Get(context, "Context"), "Canvas");
                if (_paint == null)
                {
                    var asm = canvas.GetType().Assembly;
                    var paintType = asm.GetType("SkiaSharp.SKPaint");
                    _skColor = asm.GetType("SkiaSharp.SKColor");
                    _paint = Activator.CreateInstance(paintType);
                    Set(_paint, "Style", Enum.Parse(asm.GetType("SkiaSharp.SKPaintStyle"), "Stroke"));
                    Set(_paint, "StrokeWidth", 2f);
                    Set(_paint, "IsAntialias", false);
                    _drawRect = canvas.GetType().GetMethod("DrawRect", new[] { typeof(float), typeof(float), typeof(float), typeof(float), paintType });
                }
                if (_paintColor != _boxColor) // only when the designer sent another color, never per frame
                {
                    var c = _boxColor;
                    Set(_paint, "Color", Activator.CreateInstance(_skColor, (byte)(c >> 16), (byte)(c >> 8), (byte)c, (byte)(c >> 24)));
                    _paintColor = c;
                }
                // stroke centered on a line 1 px outside the box: covers the 2 px just around it; kept inside the canvas,
                // so a control that fills it (a snippet's root) gets its box along the edges
                var (left, top) = (Math.Max(r[0] - 1, 1), Math.Max(r[1] - 1, 1));
                var (w, h) = _canvasWidth > 0 && _canvasHeight > 0 ? ((float)_canvasWidth, (float)_canvasHeight) : (float.MaxValue, float.MaxValue);
                var (right, bottom) = (Math.Min(r[0] + r[2] + 1, w - 1), Math.Min(r[1] + r[3] + 1, h - 1));
                _drawRect.Invoke(canvas, new object[] { left, top, right - left, bottom - top, _paint });
                // place mode: where the control goes in the boxed stack, row or wrap
                if (_placeType != null && _placeLine is { } line)
                    _drawRect.Invoke(canvas, new object[] { line[0], line[1], line[2], line[3], _paint });
                return true;
            }
            catch
            {
                return false;
            }
        }

        #endregion

        #region Reflection

        private static readonly Dictionary<(Type, string), PropertyInfo> Props = new();

        /// <summary>Most derived public instance property of that name (skips <c>new</c>-hidden ones, no ambiguity).</summary>
        private static PropertyInfo FindProp(Type type, string name)
        {
            lock (Props)
            {
                if (Props.TryGetValue((type, name), out var p))
                    return p;
                for (var t = type; t != null && p == null; t = t.BaseType)
                    p = t.GetProperty(name, Inst);
                return Props[(type, name)] = p;
            }
        }

        private static object Get(object o, string name)
        {
            if (o == null)
                return null;
            var p = FindProp(o.GetType(), name);
            return p != null && p.GetIndexParameters().Length == 0 ? p.GetValue(o) : null;
        }

        private static void Set(object o, string name, object value) => FindProp(o.GetType(), name)?.SetValue(o, value);

        private static readonly Dictionary<(Type, string), MethodInfo> Methods = new();

        /// <summary>Calls the most derived public parameterless instance method of that name; null when there is none.
        /// The method is looked up once per type: GetMethods copies every method of every type it walks, about 27 KB
        /// for a DrawnUi control, which the highlight paid on every frame.</summary>
        private static object Call(object o, string name)
        {
            if (o == null)
                return null;
            MethodInfo m;
            lock (Methods)
            {
                var key = (o.GetType(), name);
                if (!Methods.TryGetValue(key, out m))
                {
                    for (var t = key.Item1; t != null && m == null; t = t.BaseType)
                        m = t.GetMethods(Inst).FirstOrDefault(x => x.Name == name && x.GetParameters().Length == 0);
                    Methods[key] = m;
                }
            }
            return m?.Invoke(o, null);
        }

        #endregion
    }
}
