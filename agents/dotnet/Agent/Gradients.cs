using System.Collections;
using System.Globalization;
using System.Reflection;
using System.Text.Json;

namespace DrawnUi.Designer.Agent
{
    internal static partial class AgentHost
    {
        /// <summary>DrawnUi's gradient (FillGradient, StrokeGradient, BackgroundGradient...): sent and set as one JSON text.</summary>
        private static bool IsGradient(Type type) => IsType(type, "DrawnUi.Draw.SkiaGradient");

        /// <summary>
        /// {"type":"Linear","colors":["#AARRGGBB",...],"stops":[0,0.5,1] or null,"angle":45 or null,"start":[x,y],
        /// "end":[x,y],"opacity":1}. Stops are null when the gradient has none (colors spread evenly).
        /// </summary>
        private static string GradientJson(object gradient)
        {
            var stops = Get(gradient, "ColorPositions") is IList { Count: > 0 } positions
                ? positions.Cast<object>().Select(p => Convert.ToDouble(p, CultureInfo.InvariantCulture)).ToArray()
                : null;
            float F(string name) => Convert.ToSingle(Get(gradient, name) ?? 0f, CultureInfo.InvariantCulture);
            return JsonSerializer.Serialize(new
            {
                type = Get(gradient, "Type")?.ToString(),
                colors = (Get(gradient, "Colors") as IEnumerable)?.Cast<object>().Select(ColorHex).ToArray() ?? Array.Empty<string>(),
                stops,
                angle = Get(gradient, "Angle") is { } angle ? Convert.ToDouble(angle, CultureInfo.InvariantCulture) : (double?)null,
                start = new[] { F("StartXRatio"), F("StartYRatio") },
                end = new[] { F("EndXRatio"), F("EndYRatio") },
                opacity = F("Opacity"),
            });
        }

        /// <summary>
        /// Sets a gradient property from <see cref="GradientJson"/> text: the control's own gradient is updated in place
        /// (everything else on it stays), a new one is made when it has none, "" removes it. The whole text is read
        /// before anything changes: invalid text changes nothing. UI thread.
        /// </summary>
        private static void SetGradient(object control, PropDef def, string text)
        {
            if (string.IsNullOrWhiteSpace(text))
            {
                _setValue.Invoke(control, new[] { def.Property, null });
                return;
            }

            var gradient = _getValue.Invoke(control, new[] { def.Property });
            var gradientType = gradient?.GetType() ?? (def.ReturnType.IsAbstract ? null : def.ReturnType)
                               ?? throw new InvalidOperationException($"cannot make a {def.ReturnType.Name}");
            var colorsProp = FindProp(gradientType, "Colors");
            var colorType = colorsProp?.PropertyType.GetGenericArguments().FirstOrDefault()
                            ?? throw new InvalidOperationException($"{gradientType.Name} has no Colors list");

            // read and convert everything first
            using var doc = JsonDocument.Parse(text);
            var root = doc.RootElement;
            object kind = null;
            if (root.TryGetProperty("type", out var t) && t.ValueKind == JsonValueKind.String)
                kind = Enum.Parse(FindProp(gradientType, "Type").PropertyType, t.GetString(), true);
            IList colors = null;
            if (root.TryGetProperty("colors", out var c) && c.ValueKind == JsonValueKind.Array)
            {
                colors = (IList)Activator.CreateInstance(typeof(List<>).MakeGenericType(colorType));
                foreach (var hex in c.EnumerateArray())
                    colors.Add(ColorFromHex(colorType, hex.GetString() ?? "")
                               ?? throw new FormatException($"\"{hex}\" is not a color (#AARRGGBB)"));
            }
            List<double> stops = null;
            var stopsGiven = root.TryGetProperty("stops", out var st);
            if (stopsGiven && st.ValueKind == JsonValueKind.Array)
                stops = st.EnumerateArray().Select(x => x.GetDouble()).ToList();
            double? angle = root.TryGetProperty("angle", out var a) && a.ValueKind == JsonValueKind.Number ? a.GetDouble() : null;
            float[] Pair(string name) => root.TryGetProperty(name, out var p) && p.ValueKind == JsonValueKind.Array
                ? p.EnumerateArray().Select(x => (float)x.GetDouble()).ToArray() is { Length: 2 } xy ? xy : throw new FormatException($"\"{name}\" needs [x, y]")
                : null;
            var start = Pair("start");
            var end = Pair("end");
            float? opacity = root.TryGetProperty("opacity", out var o) && o.ValueKind == JsonValueKind.Number ? (float)o.GetDouble() : null;

            var fresh = gradient == null;
            gradient ??= Activator.CreateInstance(gradientType);
            if (kind != null)
                Set(gradient, "Type", kind);
            if (colors != null)
                Set(gradient, "Colors", colors);
            if (stopsGiven) // null: none, the colors spread evenly (the property takes a list only)
                Set(gradient, "ColorPositions", stops ?? new List<double>());
            if (angle != null)
                Set(gradient, "Angle", angle);
            else
            {
                if (root.TryGetProperty("angle", out _))
                    Set(gradient, "Angle", null); // explicit null: the start and end points rule
                if (start != null)
                {
                    Set(gradient, "StartXRatio", start[0]);
                    Set(gradient, "StartYRatio", start[1]);
                }
                if (end != null)
                {
                    Set(gradient, "EndXRatio", end[0]);
                    Set(gradient, "EndYRatio", end[1]);
                }
            }
            if (opacity != null)
                Set(gradient, "Opacity", opacity.Value);
            if (fresh)
                _setValue.Invoke(control, new[] { def.Property, gradient });
        }
    }
}
