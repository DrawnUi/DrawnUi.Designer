using System.Collections;
using System.Collections.Specialized;
using System.ComponentModel;
using System.Reflection;
using System.Reflection.Emit;
using System.Text.Json;

namespace DrawnUi.Designer.Agent
{
    internal static partial class AgentHost
    {
        #region The controls the app can create

        private sealed class ControlKind
        {
            public Type Type;
            public string Group;
            public bool Own;
            public bool Container;
            public List<string[]> Defaults;
        }

        private static List<ControlKind> _controls;

        /// <summary>
        /// Every DrawnUi control the app can create with a public parameterless constructor: the library's public
        /// top-level controls and the app's own. Read once, on the first request of a session. UI thread.
        /// </summary>
        private static List<object> Controls()
        {
            _controls ??= FindControlKinds();
            // Specialized library controls are offered only when the app's tree already uses them.
            var used = _controls.Any(k => !k.Own && Specialized.Contains(k.Type.Name))
                ? Walk(false).Where(v => !v.Internal).Select(v => v.Control.GetType()).ToHashSet()
                : new HashSet<Type>();
            return _controls.Where(k => k.Own || !Specialized.Contains(k.Type.Name) || used.Contains(k.Type)).Select(k => (object)new
            {
                typeName = k.Type.FullName,
                group = k.Group,
                own = k.Own,
                defaults = k.Defaults,
                container = k.Container,
            }).ToList();
        }

        private static List<ControlKind> FindControlKinds()
        {
            var kinds = new List<ControlKind>();
            var screens = ScreenTypes();
            foreach (var assembly in AppDomain.CurrentDomain.GetAssemblies())
            {
                if (assembly.IsDynamic)
                    continue;
                var name = assembly.GetName().Name ?? "";
                var own = !IsLibrary(assembly);
                if (!own && !name.StartsWith("DrawnUi", StringComparison.Ordinal))
                    continue; // the platform: no controls there
                Type[] types;
                try
                {
                    types = assembly.GetTypes();
                }
                catch (ReflectionTypeLoadException e)
                {
                    types = e.Types.Where(t => t != null).ToArray();
                }
                catch
                {
                    continue; // an assembly whose types need one the app does not carry (MessagePack on a phone)
                }
                foreach (var type in types)
                {
                    try
                    {
                        AddKind(type);
                    }
                    catch
                    {
                        // a type that needs an assembly the app does not carry is not a control to place
                    }
                }

                void AddKind(Type type)
                {
                    if (!type.IsClass || type.IsAbstract || type.ContainsGenericParameters || !type.IsPublic // nested types are not IsPublic
                        || type.GetConstructor(Type.EmptyTypes) == null || !IsType(type, "DrawnUi.Draw.SkiaControl")
                        || type.IsDefined(typeof(ObsoleteAttribute), false)
                        || type.GetCustomAttribute<EditorBrowsableAttribute>()?.State == EditorBrowsableState.Never)
                        return;
                    if (!own && LibraryParts.Contains(type.Name) || own && screens.Contains(type))
                        return;
                    var group = GroupOf(type);
                    var container = group is "Layout" or "Shapes" && !IsType(type, "DrawnUi.Draw.SkiaScroll");
                    kinds.Add(new ControlKind { Type = type, Group = group, Own = own, Container = container, Defaults = DefaultsOf(type, group) });
                }
            }
            // the panel's order: library first, then the app's; by name within
            return kinds.OrderBy(k => k.Own).ThenBy(k => k.Type.Name, StringComparer.Ordinal).ToList();
        }

        /// <summary>
        /// The app's screens, which are not controls to place: the types its SkiaShell routes make, the shell's root
        /// content and the canvas's root. A route is a factory: the types it creates are read from its code (the
        /// constructors it calls), never by running it.
        /// </summary>
        private static HashSet<Type> ScreenTypes()
        {
            var types = new HashSet<Type>();
            if ((Get(_canvas, "Views") as IList)?.Cast<object>().FirstOrDefault() is { } root)
                types.Add(root.GetType());
            if (Get(_canvas, "Content") is { } content)
                types.Add(content.GetType());
            if (FindShell() is { } shell)
            {
                if (Get(shell, "RootContent") is { } rootContent)
                    types.Add(rootContent.GetType());
                if (Get(shell, "Routes") is IDictionary routes)
                    foreach (var factory in routes.Values.OfType<Delegate>())
                        types.UnionWith(ConstructedTypes(factory, 0));
            }
            return types;
        }

        /// <summary>The types a factory creates: its own code, then the delegates it captured (a route that wraps
        /// another route's factory), a few levels deep.</summary>
        private static IEnumerable<Type> ConstructedTypes(Delegate factory, int depth)
        {
            var types = ConstructedTypes(factory.Method).ToList();
            if (depth < 3 && factory.Target is { } closure)
                foreach (var field in closure.GetType().GetFields(BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic))
                    if (field.GetValue(closure) is Delegate captured)
                        types.AddRange(ConstructedTypes(captured, depth + 1));
            return types;
        }

        private static Dictionary<short, OpCode> _opCodes;

        /// <summary>The types a method creates with <c>new</c> (its IL's newobj), read without running it.</summary>
        private static IEnumerable<Type> ConstructedTypes(MethodInfo method)
        {
            var il = method.GetMethodBody()?.GetILAsByteArray();
            if (il == null)
                yield break;
            _opCodes ??= typeof(OpCodes).GetFields(BindingFlags.Public | BindingFlags.Static)
                .Select(f => (OpCode)f.GetValue(null)).ToDictionary(o => o.Value);
            for (var i = 0; i < il.Length;)
            {
                var code = il[i] == 0xFE && i + 1 < il.Length ? (short)(0xFE00 | il[i + 1]) : il[i];
                if (!_opCodes.TryGetValue(code, out var op))
                    yield break; // not IL this reader knows: stop rather than guess
                i += op.Size;
                if (op == OpCodes.Newobj && i + 4 <= il.Length)
                {
                    Type made = null;
                    try
                    {
                        made = method.Module.ResolveMethod(BitConverter.ToInt32(il, i),
                            method.DeclaringType?.GetGenericArguments(), method.GetGenericArguments())?.DeclaringType;
                    }
                    catch
                    {
                    }
                    if (made != null)
                        yield return made;
                }
                i += op.OperandType switch
                {
                    OperandType.InlineNone => 0,
                    OperandType.ShortInlineBrTarget or OperandType.ShortInlineI or OperandType.ShortInlineVar => 1,
                    OperandType.InlineVar => 2,
                    OperandType.InlineI8 or OperandType.InlineR => 8,
                    OperandType.InlineSwitch => 4 + 4 * BitConverter.ToInt32(il, i),
                    _ => 4,
                };
            }
        }

        /// <summary>The Controls panel's group, from the type's ancestry (a button is a layout too: inputs first).</summary>
        private static string GroupOf(Type type)
        {
            bool Is(params string[] names) => names.Any(n => IsNamed(type, n)); // DrawnUi.Draw or DrawnUi.Controls
            if (Is("SkiaButton", "SkiaToggle", "SkiaSwitch", "SkiaCheckbox", "SkiaRadioButton", "SkiaSlider", "SkiaRangeBase", "SkiaProgress"))
                return "Input";
            if (Is("SkiaLabel", "SkiaEditor", "SkiaMauiEntry", "SkiaMauiEditor"))
                return "Text";
            if (Is("SkiaImage", "SkiaSvg", "SkiaLottie", "SkiaGif", "AnimatedFramesRenderer", "SkiaSprite", "SkiaMediaImage", "SkiaImageTiles"))
                return "Media";
            if (Is("SkiaShape"))
                return "Shapes";
            if (Is("SkiaLayout", "SkiaScroll"))
                return "Layout";
            return "Other";
        }

        /// <summary>Specialized library controls: listed in the Controls panel only when the app's tree uses them.</summary>
        private static readonly HashSet<string> Specialized = new()
        {
            "ContentWithBackdrop", "InfiniteLayout", "GridLayout", "AnimatedFramesRenderer",
        };

        /// <summary>The library's public controls that are parts of others (a slider's thumb, a picker's wheel, a
        /// scroll's bar), base classes or app roots: not offered for placing. DrawnUi marks none of them.</summary>
        private static readonly HashSet<string> LibraryParts = new()
        {
            "SkiaControl", "ContentLayout", "SkiaShell", "SkiaDrawnCell", "SkiaDynamicDrawnCell", "SkiaEditorSelection",
            "SliderThumb", "SliderTrail", "SliderValueDesc", "ProgressTrail", "SkiaCursor", "SkiaHoverMask", "SkiaScrollBar",
            "RefreshIndicator", "LottieRefreshIndicator", "ScrollPickerLabelContainer", "ScrollPickerLabelLayout",
            "ScrollPickerWheel", "SkiaWheelShape", "SkiaWheelStack", "SkiaWheelScroll", "DebugSurfaceImage", "SkiaMauiElement",
        };

        /// <summary>
        /// What a new one needs to be seen and told apart, in the text setProperty takes; only properties the type has.
        /// </summary>
        private static List<string[]> DefaultsOf(Type type, string group)
        {
            var wanted = new List<(string, string)>();
            var empty = ("BackgroundColor", "#22FFFFFF");
            switch (group)
            {
                case "Text":
                    wanted.Add(IsNamed(type, "SkiaEditor") || IsNamed(type, "SkiaMauiEntry") ? ("WidthRequest", "200") : ("Text", "Label"));
                    break;
                case "Input":
                    if (IsNamed(type, "SkiaButton"))
                        wanted.Add(("Text", "Button"));
                    else if (IsNamed(type, "SkiaSlider") || IsNamed(type, "SkiaRangeBase"))
                        wanted.Add(("WidthRequest", "200"));
                    break;
                case "Media":
                    wanted.AddRange(new[] { ("WidthRequest", "120"), ("HeightRequest", "80"), empty });
                    break;
                case "Shapes":
                    wanted.AddRange(new[] { ("WidthRequest", "80"), ("HeightRequest", "80"), ("BackgroundColor", "#33FFFFFF") });
                    break;
                case "Layout":
                    wanted.AddRange(new[] { ("HeightRequest", "80"), ("MinimumWidthRequest", "120"), empty });
                    break;
                default:
                    wanted.AddRange(new[] { ("WidthRequest", "80"), ("HeightRequest", "40"), empty });
                    break;
            }
            var has = DefsOf(type).Where(d => d.Editable).Select(d => d.Name).ToHashSet();
            return wanted.Where(w => has.Contains(w.Item1)).Select(w => new[] { w.Item1, w.Item2 }).ToList();
        }

        #endregion

        #region Insert and remove

        /// <summary>A control that holds other controls in its Children: layouts and shapes, not a templated one (its
        /// children come from data) and not a scroll (it holds one Content).</summary>
        private static string NotAContainer(object control)
        {
            var type = control.GetType();
            if (!(IsType(type, "DrawnUi.Draw.SkiaLayout") || IsType(type, "DrawnUi.Draw.SkiaShape")) || IsType(type, "DrawnUi.Draw.SkiaScroll")
                || GroupOf(type) is "Input" or "Text" or "Media")
                return $"{type.Name} does not hold children";
            if (MadeFromData(control))
                return $"{type.Name} makes its children from data (ItemsSource / ItemTemplate)";
            if (ItemsAbove(control) is { } items)
                return $"{type.Name} is made from {items.GetType().Name}'s template: nothing goes into it";
            return null;
        }

        /// <summary>The layout whose template made the cell that is <paramref name="control"/> or holds it, else null: every
        /// cell is the same template code, so nothing inside one is changed one by one.</summary>
        private static object ItemsAbove(object control)
        {
            for (var p = Get(control, "Parent"); p != null && IsType(p.GetType(), "DrawnUi.Draw.SkiaControl"); p = Get(p, "Parent"))
                if (MadeFromData(p))
                    return p;
            return null;
        }

        private static Type ControlType(string typeName)
        {
            var type = LoadedType(typeName) ?? throw new InvalidOperationException($"no control type {typeName}");
            if (type.IsAbstract || type.GetConstructor(Type.EmptyTypes) == null || !IsType(type, "DrawnUi.Draw.SkiaControl"))
                throw new InvalidOperationException($"{typeName} cannot be created here");
            return type;
        }

        /// <summary>Creates a control of that type with its defaults and adds it to the parent's children at
        /// <paramref name="index"/> (null: last). Its id and the index it got. UI thread.</summary>
        /// <param name="props">Properties set after the defaults (a pasted control), in setProperty text; one the type
        /// lacks or a value that does not convert is skipped and named in the returned notes.</param>
        /// <param name="children">A pasted copy's children ({typeName, props: [{name, value, typeName}], children}),
        /// built inside it in order after it is added.</param>
        private static (long Node, int Index, List<string> Skipped) InsertControl(long parentId, int? index, string typeName, List<string[]> props = null,
            List<JsonElement> children = null)
        {
            var parent = Find(parentId) ?? throw new InvalidOperationException($"no control {parentId}");
            if (parent == PageWrapper)
                throw new InvalidOperationException(OutsideSnippet);
            if (NotAContainer(parent) is { } why)
                throw new InvalidOperationException(why);
            var type = ControlType(typeName);
            var control = Activator.CreateInstance(type);
            // a paste carries the copy's whole set state: a new control's defaults would change it (a shape's 80 x 80)
            if ((props?.Count ?? 0) == 0 && (children?.Count ?? 0) == 0)
            {
                var defaults = (_controls?.FirstOrDefault(k => k.Type == type)?.Defaults) ?? DefaultsOf(type, GroupOf(type));
                foreach (var d in defaults)
                    SetValueText(control, d[0], d[1]);
            }
            var skipped = new List<string>();
            SetProps(control, "", props, skipped);
            var at = AddChild(parent, control, index);
            foreach (var child in children ?? new List<JsonElement>())
                BuildChild(control, child, type.Name, skipped);
            Call(parent, "Invalidate");
            Call(_canvas, "Update");
            return (IdOf(control), at, skipped);
        }

        private static void SetProps(object control, string where, List<string[]> props, List<string> skipped)
        {
            foreach (var p in props ?? new List<string[]>())
            {
                try
                {
                    SetValueText(control, p[0], p[1] ?? "");
                }
                catch (Exception e)
                {
                    skipped.Add($"{where}{p[0]} ({Unwrap(e).Message})");
                }
            }
        }

        /// <summary>Builds one child of a pasted copy (its type, its props in order, then its own children) and adds it
        /// last to <paramref name="parent"/>; what does not convert or cannot be made is skipped and named.</summary>
        private static void BuildChild(object parent, JsonElement spec, string path, List<string> skipped)
        {
            var typeName = spec.ValueKind == JsonValueKind.Object && spec.TryGetProperty("typeName", out var t) && t.ValueKind == JsonValueKind.String ? t.GetString() : null;
            var where = $"{path} > {typeName?.Split('.').Last() ?? "?"}";
            object control;
            try
            {
                control = Activator.CreateInstance(ControlType(typeName ?? throw new InvalidOperationException("no typeName")));
                var props = spec.TryGetProperty("props", out var ps) && ps.ValueKind == JsonValueKind.Array
                    ? ps.EnumerateArray()
                        .Where(p => p.ValueKind == JsonValueKind.Object && p.TryGetProperty("name", out var n) && n.ValueKind == JsonValueKind.String)
                        .Select(p => new[] { p.GetProperty("name").GetString(), p.TryGetProperty("value", out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : "" })
                        .ToList()
                    : null;
                SetProps(control, where + ".", props, skipped);
                AddChild(parent, control, null);
            }
            catch (Exception e)
            {
                skipped.Add($"{where} ({Unwrap(e).Message})");
                return;
            }
            if (spec.TryGetProperty("children", out var cs) && cs.ValueKind == JsonValueKind.Array)
                foreach (var child in cs.EnumerateArray())
                    BuildChild(control, child, where, skipped);
        }

        /// <summary>A control as a copy: its set properties (getProps' Set group, in its order and text) and its own
        /// children, recursively; a templated control's children come from data and are left out. UI thread.</summary>
        private static object SpecOf(object control)
        {
            var props = PropRows(control)
                .Where(r => r.IsSet && r.IsDefault != true && !r.Value.StartsWith("<"))
                .Select(r => new { name = r.Def.Name, value = r.Value, typeName = r.Def.ReturnType.FullName })
                .ToList();
            var children = MadeFromData(control) ?new List<object>() : AppChildren(control).Select(SpecOf).ToList();
            return new { typeName = control.GetType().FullName, props, children };
        }

        /// <summary>
        /// Adds through the app's own Children when the control observes it (the default collection, or an observable
        /// one the app set): the app's list stays the truth. Otherwise AddSubView, the runtime path for a plain list,
        /// then the children after the index are moved behind it.
        /// </summary>
        /// <param name="index">Among the parent's non-internal children, in tree order (what the app's Children holds
        /// and what the code writer counts); null or past the end: last.</param>
        private static int AddChild(object parent, object child, int? index)
        {
            var own = AppChildren(parent);
            var at = Math.Clamp(index ?? own.Count, 0, own.Count);
            var before = at < own.Count ? own[at] : null; // the child that will come after the new one
            var children = Get(parent, "Children") as IList;
            if (children is INotifyCollectionChanged && !children.IsReadOnly)
            {
                var i = before == null ? -1 : children.IndexOf(before);
                if (i >= 0)
                    children.Insert(i, child);
                else if (before == null)
                    children.Add(child);
                else
                    children.Insert(Math.Min(at, children.Count), child);
                return at;
            }
            var add = SubViewMethod(parent, "AddSubView");
            var remove = SubViewMethod(parent, "RemoveSubView");
            var views = (Get(parent, "Views") as IList)?.Cast<object>().ToList() ?? new List<object>();
            var after = before == null ? new List<object>() : views.Skip(Math.Max(0, views.IndexOf(before))).ToList();
            add.Invoke(parent, new[] { child });
            foreach (var moved in after)
            {
                remove.Invoke(parent, new[] { moved });
                add.Invoke(parent, new[] { moved });
            }
            return at;
        }

        /// <summary>A parent's children that are not the library's own parts, in tree order: the list insert indexes
        /// count in (the same rule as the tree's internal flag).</summary>
        private static List<object> AppChildren(object parent)
        {
            var views = (Get(parent, "Views") as IList)?.Cast<object>().ToList() ?? new List<object>();
            if (OwnsDefaultContent(parent))
                return new List<object>();
            return views.Where(c => !(!IsAppType(c.GetType()) && c.GetType().DeclaringType is { } outer && IsLibrary(outer.Assembly))).ToList();
        }

        private static MethodInfo SubViewMethod(object parent, string name) =>
            parent.GetType().GetMethods(BindingFlags.Public | BindingFlags.Instance)
                .FirstOrDefault(m => m.Name == name && m.GetParameters().Length == 1)
            ?? throw new InvalidOperationException($"{parent.GetType().Name} has no {name}");

        /// <summary>Takes a control out of its parent, live (the app's Children list when observed). UI thread.</summary>
        private static void RemoveControl(long nodeId)
        {
            var control = Find(nodeId) ?? throw new InvalidOperationException($"no control {nodeId}");
            var parent = Get(control, "Parent") ?? throw new InvalidOperationException("the control has no parent");
            if (!IsType(parent.GetType(), "DrawnUi.Draw.SkiaControl"))
                throw new InvalidOperationException("the control is the canvas's root"); // its parent is the canvas
            if (ItemsAbove(control) is { } items)
                throw new InvalidOperationException($"{control.GetType().Name} is made from {items.GetType().Name}'s template");
            TakeOut(parent, control);
            if (_highlightTarget != null && _highlightTarget.TryGetTarget(out var shown) && ReferenceEquals(shown, control))
                SetHighlight(null);
            Call(parent, "Invalidate");
            Call(_canvas, "Update");
        }

        private static void TakeOut(object parent, object control)
        {
            if (Get(parent, "Children") is IList children and INotifyCollectionChanged && !children.IsReadOnly && children.Contains(control))
                children.Remove(control);
            else
                SubViewMethod(parent, "RemoveSubView").Invoke(parent, new[] { control });
        }

        /// <summary>Its children come from data (ItemsSource / ItemTemplate).</summary>
        private static bool MadeFromData(object control) =>
            Get(control, "IsTemplated") is true || Get(control, "ItemsSource") != null || Get(control, "ItemTemplate") != null;

        /// <summary>Moves the control itself (same object, same id) into <paramref name="parentId"/> at
        /// <paramref name="index"/>, counted among the parent's non-internal children without the moved one. The
        /// index it got. UI thread.</summary>
        private static int MoveControl(long nodeId, long parentId, int? index)
        {
            var control = Find(nodeId);
            var parent = Find(parentId);
            var from = Get(control, "Parent");
            var name = control.GetType().Name;
            if (from == null || !IsType(from.GetType(), "DrawnUi.Draw.SkiaControl"))
                throw new InvalidOperationException($"{name} is the canvas's root");
            if (ItemsAbove(control) is { } items)
                throw new InvalidOperationException($"{name} is made from {items.GetType().Name}'s template");
            if (!AppChildren(from).Contains(control))
                throw new InvalidOperationException($"{name} is a part of {from.GetType().Name}, not in the app's code");
            if (parent == PageWrapper)
                throw new InvalidOperationException(OutsideSnippet);
            for (var p = parent; p != null && IsType(p.GetType(), "DrawnUi.Draw.SkiaControl"); p = Get(p, "Parent"))
                if (ReferenceEquals(p, control))
                    throw new InvalidOperationException($"{name} cannot go inside itself or its own children");
            if (NotAContainer(parent) is { } why)
                throw new InvalidOperationException(why);
            TakeOut(from, control);
            var at = AddChild(parent, control, index);
            Call(from, "Invalidate");
            Call(parent, "Invalidate");
            Call(_canvas, "Update");
            return at;
        }

        #endregion

        #region Place mode

        private const string OutsideSnippet = "Click inside the snippet's layout";

        /// <summary>AttachPage: the canvas Content is the page's own wrapper around the snippet, never a place or
        /// insert target (it is not in the snippet's code). Desktop apps: null, their root is their own code.</summary>
        private static object PageWrapper => _host is BlazorHost && _canvas is { } canvas ? Get(canvas, "Content") : null;

        private static string _placeType;

        /// <summary>
        /// Place mode: pick mode's pointer, but the hover shows the container a control of that type would go into,
        /// and the click inserts it there (Inserted, request 0) and ends the mode. null or Esc cancels. UI thread.
        /// </summary>
        private static void SetPlaceMode(string typeName)
        {
            if (typeName == null)
            {
                if (_placeType != null)
                {
                    _placeType = null;
                    SetPickMode(false);
                }
                return;
            }
            ControlType(typeName); // a bad type is refused at once
            SetPickMode(false);
            SetPickMode(true);
            _placeType = typeName;
        }

        /// <summary>How a layout lines up its children (a layer, a grid: free, the control goes last).</summary>
        private enum Flow { Column, Row, Wrap, Free }

        private static Flow FlowOf(object control) => Get(control, "Type")?.ToString() switch
        {
            "Column" => Flow.Column,
            "Row" => Flow.Row,
            "Wrap" => Flow.Wrap,
            _ => Flow.Free,
        };

        /// <summary>The line at the edge of <paramref name="r"/> a placed control goes to (x, y, width, height; one of
        /// them 0): across the parent <paramref name="p"/> in a column, along <paramref name="r"/> in a row or wrap; none
        /// in a layer or grid.</summary>
        private static float[] Edge(Flow flow, float[] p, float[] r, bool before) => flow switch
        {
            Flow.Column => new[] { p[0], before ? r[1] : r[1] + r[3], p[2], 0f },
            Flow.Row or Flow.Wrap => new[] { before ? r[0] : r[0] + r[2], r[1], 0f, r[3] },
            _ => null,
        };

        private static bool Accepts(object control) => control != null && control != PageWrapper && NotAContainer(control) == null;

        /// <summary>
        /// Place mode: the control under the point (as Pick finds it) takes the new one when it holds children, at the
        /// index the point is at (a column by its children's vertical middles, a row by their horizontal ones, a wrap in
        /// reading order, a layer or grid last). Otherwise the new one goes next to it in its parent: before it in the
        /// first half along the parent's direction, else after (in a layer or grid after). Nothing where neither takes it
        /// (a template's cell). The page's own wrapper is not shown: its empty area is the snippet's root control's.
        /// The line where it goes, in a stack, row or wrap. UI thread.
        /// </summary>
        private static (long? Container, int? Index, float[] Line) PlaceTarget(float x, float y)
        {
            var target = Pick(x, y) is { } picked ? Find(picked) : null;
            if (target != null && target == PageWrapper)
                target = AppChildren(target).FirstOrDefault();
            if (target == null)
                return (null, null, null);
            static float Cx(float[] r) => r[0] + r[2] / 2;
            static float Cy(float[] r) => r[1] + r[3] / 2;
            float[] RectOf(object c) => Rect(Call(c, "GetAccessibilityPixelRect"));
            if (Accepts(target))
            {
                var flow = FlowOf(target);
                var kids = AppChildren(target).Select(RectOf).ToList();
                var i = flow switch
                {
                    Flow.Column => kids.FindIndex(r => Cy(r) > y),
                    Flow.Row => kids.FindIndex(r => Cx(r) > x),
                    Flow.Wrap => kids.FindIndex(r => y >= r[1] && y < r[1] + r[3] ? Cx(r) > x : Cy(r) > y),
                    _ => -1,
                };
                var p = RectOf(target);
                var line = i >= 0 ? Edge(flow, p, kids[i], true) : kids.Count > 0 ? Edge(flow, p, kids[^1], false) : null;
                return (IdOf(target), i >= 0 ? i : null, line);
            }
            var parent = Get(target, "Parent");
            var at = parent != null && IsType(parent.GetType(), "DrawnUi.Draw.SkiaControl") && Accepts(parent) ? AppChildren(parent).IndexOf(target) : -1;
            if (at < 0)
                return (null, null, null);
            var pflow = FlowOf(parent);
            var tr = RectOf(target);
            var before = pflow switch { Flow.Column => y < Cy(tr), Flow.Row or Flow.Wrap => x < Cx(tr), _ => false };
            return (IdOf(parent), at + (before ? 0 : 1), Edge(pflow, RectOf(parent), tr, before));
        }

        #endregion
    }
}
