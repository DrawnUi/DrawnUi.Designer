using System.Collections;
using System.Diagnostics;
using System.Reflection;
using System.Runtime.InteropServices;
using Expression = System.Linq.Expressions.Expression;

namespace DrawnUi.Designer.Agent
{
    internal static partial class AgentHost
    {
        /// <summary>
        /// What the agent needs from the UI framework that hosts the DrawnUi canvas. The agent references neither WPF
        /// nor MAUI: each host reaches its framework by reflection, so one agent DLL loads in both and binds to whatever
        /// framework version the app runs.
        /// </summary>
        private interface IHost
        {
            /// <summary>Runs on the UI thread, soon (normal priority) or after pending input and rendering (low).</summary>
            void Post(Action action, bool low = false);

            /// <summary>The DrawnView to inspect now (MAUI: the one on the current page), or null. UI thread.</summary>
            object FindCanvas();

            /// <summary>The canvas size in pixels. UI thread.</summary>
            (float Width, float Height) CanvasPixelSize(object canvas);

            /// <summary>The host element of the canvas is not shown (collapsed, not on screen). UI thread.</summary>
            bool CanvasHidden(object canvas);

            /// <summary>The native window that shows the canvas. UI thread.</summary>
            IntPtr WindowHandle(object canvas);

            /// <summary>Calls <paramref name="onEscape"/> on Esc in the canvas's window; true marks the key handled.
            /// Dispose to stop. UI thread.</summary>
            IDisposable HookEscape(object canvas, Func<bool> onEscape);

            /// <summary>App objects whose fields may hold controls: the application, windows, pages. UI thread.</summary>
            IEnumerable<object> FieldOwners();

            /// <summary>The app's screens when the framework navigates (MAUI Shell); null when it does not. UI thread.</summary>
            List<object> Screens();

            /// <summary>Opens a screen of <see cref="Screens"/>; null when the framework does not navigate. UI thread.</summary>
            Task OpenScreen(string route);

            /// <summary>Starts (on) or stops calling back when the framework shows another page (no polling). UI thread.</summary>
            void WatchNavigation(bool on);

            /// <summary>The framework elements holding the canvas, nearest first. UI thread.</summary>
            IEnumerable<object> Ancestors(object canvas);

            /// <summary>The element is a page or window of the framework (what an app class holding a canvas usually is).</summary>
            bool IsPage(object element);
        }

        private static readonly Dictionary<string, Type> LoadedTypes = new();

        /// <summary>The first loaded type with that full name; never loads an assembly. Found types are remembered
        /// (a lookup walks every loaded assembly); a missing one is looked for again, its assembly may load later.</summary>
        private static Type LoadedType(string fullName)
        {
            lock (LoadedTypes)
                if (LoadedTypes.TryGetValue(fullName, out var known))
                    return known;
            foreach (var assembly in AppDomain.CurrentDomain.GetAssemblies())
                if (assembly.GetType(fullName, false) is { } type)
                {
                    lock (LoadedTypes)
                        LoadedTypes[fullName] = type;
                    return type;
                }
            return null;
        }

        private static object StaticGet(Type type, string name) =>
            type?.GetProperty(name, BindingFlags.Public | BindingFlags.Static)?.GetValue(null);

        /// <summary>A delegate of <paramref name="delegateType"/> whose first argument goes to a static method of this
        /// class taking one object (event args or a value).</summary>
        private static Delegate Bridge(Type delegateType, string method, int argument)
        {
            var invoke = delegateType.GetMethod("Invoke");
            var parameters = invoke.GetParameters().Select(p => Expression.Parameter(p.ParameterType)).ToArray();
            var body = Expression.Call(typeof(AgentHost).GetMethod(method, BindingFlags.NonPublic | BindingFlags.Static),
                Expression.Convert(parameters[argument], typeof(object)));
            return Expression.Lambda(delegateType, body, parameters).Compile();
        }

        private static Func<bool> _escape;

        /// <summary>A key event of either framework: Esc goes to the current handler, which may mark it handled.</summary>
        private static void OnHostKey(object args)
        {
            try
            {
                if (_escape != null && Get(args, "Key")?.ToString() == "Escape" && _escape())
                    Set(args, "Handled", true);
            }
            catch (Exception e)
            {
                ReportHandlerError("key", e);
            }
        }

        private sealed class Unhook(Action undo) : IDisposable
        {
            public void Dispose() => undo();
        }

        /// <summary>Adds an Esc handler to an object's PreviewKeyDown event (WPF Window, WinUI UIElement).</summary>
        private static IDisposable HookPreviewKeyDown(object target, Func<bool> onEscape)
        {
            var info = target?.GetType().GetEvent("PreviewKeyDown");
            if (info == null)
                return new Unhook(() => { });
            var handler = Bridge(info.EventHandlerType, nameof(OnHostKey), 1);
            _escape = onEscape;
            info.AddEventHandler(target, handler);
            var weak = new WeakReference<object>(target); // the window may close while picking: never keep it
            return new Unhook(() =>
            {
                try
                {
                    if (weak.TryGetTarget(out var alive))
                        info.RemoveEventHandler(alive, handler);
                }
                catch
                {
                }
                _escape = null;
            });
        }

        #region WPF

        /// <summary>DrawnUi.Wpf: a DrawnUiElement in one of the application's windows; the WPF dispatcher.</summary>
        private sealed class WpfHost : IHost
        {
            private readonly object _app;
            private readonly object _dispatcher;
            private readonly MethodInfo _beginInvoke;
            private readonly object _normal, _background;
            private readonly MethodInfo _childrenCount, _child;
            private WeakReference<object> _elementRef; // weakly: a closed window must be free to go

            private object _element
            {
                get => _elementRef != null && _elementRef.TryGetTarget(out var element) ? element : null;
                set => _elementRef = value == null ? null : new WeakReference<object>(value);
            }

            private WpfHost(object app)
            {
                _app = app;
                _dispatcher = Get(app, "Dispatcher");
                var priority = LoadedType("System.Windows.Threading.DispatcherPriority");
                _normal = Enum.Parse(priority, "Normal");
                _background = Enum.Parse(priority, "Background");
                _beginInvoke = _dispatcher.GetType().GetMethod("BeginInvoke", new[] { priority, typeof(Delegate) });
                var tree = LoadedType("System.Windows.Media.VisualTreeHelper");
                var dependencyObject = LoadedType("System.Windows.DependencyObject");
                _childrenCount = tree.GetMethod("GetChildrenCount", new[] { dependencyObject });
                _child = tree.GetMethod("GetChild", new[] { dependencyObject, typeof(int) });
            }

            public static IHost TryCreate() =>
                StaticGet(LoadedType("System.Windows.Application"), "Current") is { } app ? new WpfHost(app) : null;

            public void Post(Action action, bool low) => _beginInvoke.Invoke(_dispatcher, new[] { low ? _background : _normal, action });

            public object FindCanvas()
            {
                if (_element == null && Get(_app, "Windows") is IEnumerable windows)
                    foreach (var window in windows)
                        if ((_element = FindElement(window)) != null)
                            break;
                return _element == null ? null : Get(_element, "Canvas");
            }

            private object FindElement(object node)
            {
                if (Get(node, "IsLoaded") is true && IsType(node.GetType(), "DrawnUi.Wpf.DrawnUiElement"))
                    return node;
                var count = (int)_childrenCount.Invoke(null, new[] { node });
                for (var i = 0; i < count; i++)
                    if (FindElement(_child.Invoke(null, new[] { node, i })) is { } found)
                        return found;
                return null;
            }

            public (float Width, float Height) CanvasPixelSize(object canvas)
            {
                var scale = Convert.ToSingle(Get(canvas, "RenderingScale") ?? 1f);
                return ((float)(Convert.ToDouble(Get(_element, "ActualWidth")) * scale),
                    (float)(Convert.ToDouble(Get(_element, "ActualHeight")) * scale));
            }

            public bool CanvasHidden(object canvas) => Get(_element, "IsVisible") is false;

            private object Window() =>
                LoadedType("System.Windows.Window")?.GetMethod("GetWindow", BindingFlags.Public | BindingFlags.Static)
                    ?.Invoke(null, new[] { _element });

            public IntPtr WindowHandle(object canvas)
            {
                var window = Window();
                var helper = LoadedType("System.Windows.Interop.WindowInteropHelper");
                return window == null || helper == null ? IntPtr.Zero : (IntPtr)Get(Activator.CreateInstance(helper, window), "Handle");
            }

            public IDisposable HookEscape(object canvas, Func<bool> onEscape) => HookPreviewKeyDown(Window(), onEscape);

            public IEnumerable<object> FieldOwners()
            {
                yield return _app;
                if (Get(_app, "Windows") is IEnumerable windows)
                    foreach (var window in windows)
                        yield return window;
            }

            public List<object> Screens() => null;

            public Task OpenScreen(string route) => null;

            // DrawnUi.Wpf navigates inside the canvas only: its SkiaShell's events are watched by the agent itself
            public void WatchNavigation(bool on)
            {
            }

            public IEnumerable<object> Ancestors(object canvas)
            {
                var parent = LoadedType("System.Windows.Media.VisualTreeHelper")?.GetMethod("GetParent", new[] { LoadedType("System.Windows.DependencyObject") });
                for (var node = _element; node != null && parent != null; node = parent.Invoke(null, new[] { node }))
                    yield return node;
            }

            public bool IsPage(object element) =>
                IsType(element.GetType(), "System.Windows.Window") || IsType(element.GetType(), "System.Windows.Controls.UserControl")
                || IsType(element.GetType(), "System.Windows.Controls.Page");
        }

        #endregion

        #region Page (Blazor WebAssembly)

        /// <summary>
        /// A DrawnUi canvas in a web page (DrawnUi.Views.Canvas on Blazor WebAssembly, DrawFiddle), handed over by the
        /// page: no window, no screens, no Esc. The page's thread is the only one: work is queued behind the current
        /// one, never run inline (pick mode posts precisely to get out of the gesture dispatch).
        /// </summary>
        private sealed class BlazorHost : IHost
        {
            private readonly WeakReference<object> _canvas; // the page owns it, the agent only looks at it
            private readonly SynchronizationContext _context = SynchronizationContext.Current;

            public BlazorHost(object canvas) => _canvas = new WeakReference<object>(canvas);

            public void Post(Action action, bool low)
            {
                if (_context != null)
                    _context.Post(_ => action(), null);
                else
                    ThreadPool.UnsafeQueueUserWorkItem(_ => action(), null); // a single-threaded page runs it next
            }

            public object FindCanvas() => _canvas.TryGetTarget(out var canvas) ? canvas : null;

            public (float Width, float Height) CanvasPixelSize(object canvas)
            {
                var scale = Convert.ToSingle(Get(canvas, "RenderingScale") ?? 1f);
                return ((float)(Convert.ToDouble(Get(canvas, "Width") ?? 0d) * scale), (float)(Convert.ToDouble(Get(canvas, "Height") ?? 0d) * scale));
            }

            public bool CanvasHidden(object canvas) => Get(canvas, "IsVisible") is false;

            public IntPtr WindowHandle(object canvas) => IntPtr.Zero;

            public IDisposable HookEscape(object canvas, Func<bool> onEscape) => new Unhook(() => { });

            public IEnumerable<object> FieldOwners() => Array.Empty<object>();

            public List<object> Screens() => null;

            public Task OpenScreen(string route) => null;

            public void WatchNavigation(bool on)
            {
            }

            public IEnumerable<object> Ancestors(object canvas)
            {
                yield return canvas;
            }

            public bool IsPage(object element) => false;
        }

        #endregion

        #region MAUI

        /// <summary>
        /// DrawnUi.Maui (Windows, iOS, Mac Catalyst, Android): the DrawnView canvases of the current page (Shell.CurrentPage and the like), MAUI's
        /// dispatcher, Shell routes as screens.
        /// </summary>
        private sealed class MauiHost : IHost
        {
            private readonly object _app;
            private readonly object _dispatcher;
            private readonly MethodInfo _dispatch;

            private MauiHost(object app)
            {
                _app = app;
                _dispatcher = Get(app, "Dispatcher");
                _dispatch = LoadedType("Microsoft.Maui.Dispatching.IDispatcher")?.GetMethod("Dispatch", new[] { typeof(Action) });
            }

            public static IHost TryCreate() =>
                StaticGet(LoadedType("Microsoft.Maui.Controls.Application"), "Current") is { } app && Get(app, "Dispatcher") != null
                    ? new MauiHost(app)
                    : null;

            // MAUI's dispatcher has no priorities; a dispatched action runs after the work already queued
            public void Post(Action action, bool low) => _dispatch.Invoke(_dispatcher, new object[] { action });

            private IEnumerable<object> Windows() => (Get(_app, "Windows") as IEnumerable)?.Cast<object>() ?? Array.Empty<object>();

            /// <summary>The page the user sees: Shell, navigation and flyout pages resolved to their current page.</summary>
            private object CurrentPage()
            {
                var page = Windows().Cast<object>().Select(w => Get(w, "Page")).FirstOrDefault(p => p != null);
                for (var depth = 0; page != null && depth < 8; depth++)
                {
                    var inner = IsNamed(page.GetType(), "Shell") || IsNamed(page.GetType(), "NavigationPage")
                        ? Get(page, "CurrentPage")
                        : IsNamed(page.GetType(), "FlyoutPage") ? Get(page, "Detail") : null;
                    if (inner == null || ReferenceEquals(inner, page))
                        break;
                    page = inner;
                }
                return page;
            }

            private static MethodInfo _visualChildren;

            private static IEnumerable<object> VisualChildren(object element)
            {
                var tree = LoadedType("Microsoft.Maui.IVisualTreeElement");
                if (tree == null || !tree.IsInstanceOfType(element))
                    return Array.Empty<object>();
                _visualChildren ??= tree.GetMethod("GetVisualChildren");
                return _visualChildren.Invoke(element, null) as IEnumerable<object> ?? Array.Empty<object>();
            }

            public object FindCanvas()
            {
                // the largest visible DrawnView of the current page
                object best = null;
                double bestArea = -1;
                var queue = new Queue<object>();
                if (CurrentPage() is { } page)
                    queue.Enqueue(page);
                while (queue.Count > 0)
                {
                    var element = queue.Dequeue();
                    if (IsType(element.GetType(), "DrawnUi.Views.DrawnView"))
                    {
                        var area = Convert.ToDouble(Get(element, "Width") ?? 0d) * Convert.ToDouble(Get(element, "Height") ?? 0d);
                        if (Get(element, "IsVisible") is not false && area > bestArea)
                            (best, bestArea) = (element, area);
                        continue; // a canvas's drawn content is not a MAUI tree
                    }
                    foreach (var child in VisualChildren(element))
                        queue.Enqueue(child);
                }
                return best;
            }

            public (float Width, float Height) CanvasPixelSize(object canvas)
            {
                if (Get(Get(canvas, "CanvasView"), "CanvasSize") is { } size && Convert.ToSingle(Get(size, "Width")) > 0)
                    return (Convert.ToSingle(Get(size, "Width")), Convert.ToSingle(Get(size, "Height")));
                var scale = Convert.ToSingle(Get(canvas, "RenderingScale") ?? 1f);
                return ((float)(Convert.ToDouble(Get(canvas, "Width") ?? 0d) * scale), (float)(Convert.ToDouble(Get(canvas, "Height") ?? 0d) * scale));
            }

            public bool CanvasHidden(object canvas) =>
                Get(canvas, "IsVisible") is false || Convert.ToDouble(Get(canvas, "Width") ?? 0d) <= 0;

            private object NativeWindow(object canvas)
            {
                var window = Get(canvas, "Window") ?? Windows().Cast<object>().FirstOrDefault();
                return Get(Get(window, "Handler"), "PlatformView");
            }

            public IntPtr WindowHandle(object canvas)
            {
                if (!OperatingSystem.IsWindows())
                    return IntPtr.Zero;
                var native = NativeWindow(canvas);
                var extensions = LoadedType("Microsoft.Maui.Platform.WindowExtensions");
                var getHandle = extensions?.GetMethods(BindingFlags.Public | BindingFlags.Static)
                    .FirstOrDefault(m => m.Name == "GetWindowHandle" && m.GetParameters().Length == 1);
                if (native != null && getHandle != null && getHandle.GetParameters()[0].ParameterType.IsInstanceOfType(native))
                    return (IntPtr)getHandle.Invoke(null, new[] { native });
                return Process.GetCurrentProcess().MainWindowHandle;
            }

            public IDisposable HookEscape(object canvas, Func<bool> onEscape) =>
                HookPreviewKeyDown(Get(NativeWindow(canvas), "Content"), onEscape);

            public IEnumerable<object> FieldOwners()
            {
                yield return _app;
                foreach (var window in Windows())
                {
                    yield return window;
                    if (Get(window, "Page") is { } root)
                        yield return root;
                }
                if (CurrentPage() is { } page)
                    yield return page;
            }

            private object Shell() => StaticGet(LoadedType("Microsoft.Maui.Controls.Shell"), "Current");

            public IEnumerable<object> Ancestors(object canvas)
            {
                for (var node = canvas; node != null; node = Get(node, "Parent"))
                    yield return node;
            }

            public bool IsPage(object element) => IsType(element.GetType(), "Microsoft.Maui.Controls.Page");

            /// <summary>Application.PageAppearing: raised for every page shown, through Shell, NavigationPage or a modal.</summary>
            private Delegate _appearing;

            public void WatchNavigation(bool on)
            {
                if (_app.GetType().GetEvent("PageAppearing") is not { } appearing)
                    return;
                if (on)
                    appearing.AddEventHandler(_app, _appearing = Bridge(appearing.EventHandlerType, nameof(OnNavigated), 1));
                else if (_appearing != null)
                    appearing.RemoveEventHandler(_app, _appearing);
                if (!on)
                    _appearing = null;
            }

            /// <summary>The Shell's own pages (ShellContent routes) first, then the routes registered with Routing.</summary>
            public List<object> Screens()
            {
                if (Shell() is not { } shell)
                    return null;
                var screens = new List<object>();
                foreach (var content in ShellContents(shell))
                    if (Get(content, "Route") is string route && route.Length > 0 && !IsImplicit(route))
                        screens.Add(new { route, title = Get(content, "Title") as string });

                var routing = LoadedType("Microsoft.Maui.Controls.Routing");
                if (routing?.GetField("s_routes", BindingFlags.NonPublic | BindingFlags.Static)?.GetValue(null) is IDictionary routes)
                    foreach (DictionaryEntry entry in routes)
                    {
                        var route = entry.Key.ToString();
                        if (IsImplicit(route) || screens.Any(s => (string)Get(s, "route") == route))
                            continue;
                        var type = entry.Value?.GetType().GetField("_type", BindingFlags.NonPublic | BindingFlags.Instance)?.GetValue(entry.Value) as Type;
                        screens.Add(new { route, title = type?.Name });
                    }
                return screens;
            }

            private static bool IsImplicit(string route) => route.StartsWith("IMPL_") || route.StartsWith("D_FAULT_");

            private static IEnumerable<object> ShellContents(object shell)
            {
                foreach (var item in Get(shell, "Items") as IEnumerable ?? Array.Empty<object>())
                foreach (var section in Get(item, "Items") as IEnumerable ?? Array.Empty<object>())
                foreach (var content in Get(section, "Items") as IEnumerable ?? Array.Empty<object>())
                    yield return content;
            }

            /// <summary>
            /// A Shell page route goes there as the root (//route); a registered route is pushed over the current root
            /// (//root/route), so opening screens one after another never stacks them; the route on top already: nothing.
            /// </summary>
            public Task OpenScreen(string route)
            {
                if (Shell() is not { } shell)
                    return null;
                var location = Get(Get(shell, "CurrentState"), "Location")?.ToString() ?? "";
                var segments = location.Split('/', StringSplitOptions.RemoveEmptyEntries);
                if (segments.LastOrDefault() == route)
                    return Task.CompletedTask;
                var roots = ShellContents(shell).Select(c => Get(c, "Route") as string).Where(r => r != null).ToList();
                var target = roots.Contains(route) ? "//" + route : "//" + (segments.FirstOrDefault() ?? roots.FirstOrDefault()) + "/" + route;
                var stateType = LoadedType("Microsoft.Maui.Controls.ShellNavigationState");
                var go = shell.GetType().GetMethod("GoToAsync", new[] { stateType });
                return go.Invoke(shell, new[] { Activator.CreateInstance(stateType, target) }) as Task ?? Task.CompletedTask;
            }
        }

        #endregion
    }
}
