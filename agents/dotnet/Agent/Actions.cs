using System.Reflection;

namespace DrawnUi.Designer.Agent
{
    internal static partial class AgentHost
    {
        #region Actions the app declares for the designer

        // The app's actions: DrawnUi's DesignerActions (registered in code), then Hot Preview's [Preview]
        // static methods without parameters that return nothing (its commands), so apps that have them light up
        // without a change. Read by reflection: neither is a compile-time reference of the agent.

        private static EventInfo _actionsChanged;
        private static Delegate _onActionsChanged;
        private static List<(string Path, MethodInfo Method)> _previews;

        /// <summary>DrawnUi's DesignerActions type, when the app's DrawnUi has it.</summary>
        private static Type DesignerActionsType() =>
            _canvas?.GetType().Assembly.GetType("DrawnUi.Draw.DesignerActions") ?? LoadedType("DrawnUi.Draw.DesignerActions");

        /// <summary>Sends the app's actions, and from then on again whenever DesignerActions changes.</summary>
        private static async Task SendActions()
        {
            var actions = await Ui(() =>
            {
                WatchActions();
                return ReadActions().Select(path => new { path }).ToList();
            });
            await Send(new { type = "actions", actions });
        }

        private static void WatchActions()
        {
            if (_onActionsChanged != null || DesignerActionsType()?.GetEvent("Changed", BindingFlags.Public | BindingFlags.Static) is not { } changed)
                return;
            changed.AddEventHandler(null, _onActionsChanged = Bridge((_actionsChanged = changed).EventHandlerType, nameof(OnActionsChanged), 1));
        }

        /// <summary>Stops following DesignerActions (the designer left). UI thread.</summary>
        private static void UnwatchActions()
        {
            _actionsChanged?.RemoveEventHandler(null, _onActionsChanged);
            (_actionsChanged, _onActionsChanged) = (null, null);
        }

        private static int _actionsQueued;

        /// <summary>DesignerActions changed (any thread): one list goes out once the changes stop for 100 ms.</summary>
        private static void OnActionsChanged(object _)
        {
            if (_out == null || Interlocked.Exchange(ref _actionsQueued, 1) == 1)
                return;
            _ = Task.Run(async () =>
            {
                await Task.Delay(100);
                Interlocked.Exchange(ref _actionsQueued, 0);
                try
                {
                    if (_out != null)
                        await SendActions();
                }
                catch (Exception e)
                {
                    ReportHandlerError("actions", e);
                }
            });
        }

        /// <summary>The paths of every action, DesignerActions first. UI thread.</summary>
        private static List<string> ReadActions()
        {
            var paths = new List<string>();
            if (DesignerActionsType()?.GetProperty("Paths", BindingFlags.Public | BindingFlags.Static)?.GetValue(null) is IEnumerable<string> declared)
                paths.AddRange(declared);
            foreach (var (path, _) in Previews())
                if (!paths.Contains(path))
                    paths.Add(path);
            return paths;
        }

        /// <summary>
        /// Hot Preview's [Preview] commands in the app's assemblies: static, no parameters, no result. Their path is
        /// "Previews/" and the attribute's display name, else the method name with its underscores as spaces.
        /// </summary>
        private static List<(string Path, MethodInfo Method)> Previews()
        {
            if (_previews != null)
                return _previews;
            var found = new List<(string, MethodInfo)>();
            foreach (var assembly in AppDomain.CurrentDomain.GetAssemblies())
            {
                if (assembly.IsDynamic || IsFrameworkAssembly(assembly))
                    continue;
                Type[] types;
                try
                {
                    types = assembly.GetTypes();
                }
                catch (ReflectionTypeLoadException e)
                {
                    types = e.Types.Where(t => t != null).ToArray();
                }
                foreach (var type in types)
                foreach (var method in type.GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static | BindingFlags.DeclaredOnly))
                {
                    if (method.ReturnType != typeof(void) || method.GetParameters().Length != 0 || method.ContainsGenericParameters)
                        continue;
                    var attribute = method.GetCustomAttributesData().FirstOrDefault(a => a.AttributeType.FullName == "HotPreview.PreviewAttribute");
                    if (attribute == null)
                        continue;
                    var name = attribute.ConstructorArguments.Select(a => a.Value).OfType<string>().FirstOrDefault(s => !string.IsNullOrWhiteSpace(s))
                               ?? method.Name.Replace('_', ' ');
                    found.Add(("Previews/" + name, method));
                }
            }
            return _previews = found;
        }

        private static bool IsFrameworkAssembly(Assembly assembly)
        {
            var name = assembly.GetName().Name ?? "";
            return name.StartsWith("System", StringComparison.Ordinal) || name.StartsWith("Microsoft", StringComparison.Ordinal)
                   || name.StartsWith("SkiaSharp", StringComparison.Ordinal) || name.StartsWith("DrawnUi", StringComparison.Ordinal)
                   || name.StartsWith("Mono", StringComparison.Ordinal) || name.StartsWith("Xamarin", StringComparison.Ordinal)
                   || name == "mscorlib" || name == "netstandard" || name == "HotPreview";
        }

        /// <summary>Runs an action on the UI thread: DesignerActions first, then a [Preview] command.</summary>
        private static async Task RunAction(string path)
        {
            await Ui(() =>
            {
                if (DesignerActionsType()?.GetMethod("Run", BindingFlags.Public | BindingFlags.Static, new[] { typeof(string) }) is { } run
                    && run.Invoke(null, new object[] { path }) is true)
                    return;
                var preview = Previews().FirstOrDefault(p => p.Path == path).Method
                              ?? throw new InvalidOperationException($"the app has no action \"{path}\"");
                preview.Invoke(null, null);
            });
        }

        #endregion
    }
}
