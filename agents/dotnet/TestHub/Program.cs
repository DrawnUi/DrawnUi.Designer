// Throwaway hub: accepts one app agent on ws://127.0.0.1:<port> and checks every request kind against it.
// Usage: dotnet run -- [port] [route] [syncDir]
//        dotnet run -- dump <out.json> <route|root> [port]   saves that screen's tree (pretty) and exits
//        dotnet run -- snap <out.png> [port] [route]   prints the tree's text previews and saves a snapshot of the canvas
//        dotnet run -- hidden <dir> [port]   snapshot, then asks for the app window to be minimized (prints MINIMIZE, waits
//                                            for <dir>\ack) and checks openScreen("details"), tree, setProperty, getProps,
//                                            snapshot with no frames presented; PNGs go to <dir>
//        dotnet run -- focus <port> [grant]   sends `focus`, after AllowSetForegroundWindow(app pid) with "grant"
//        dotnet run -- nav <port> <route> [route...]   opens the routes in turn and prints the pages in the tree after each
//        dotnet run -- pick <dir> <port>   pick mode and the props filter on DesignerSample; synthetic input goes through
//                                          an input probe hook reading <dir>\cmd.txt; prints HOVER when a capture may run
//        dotnet run -- props <port> <field suffix> [name=value...]   prints the props of the control held in that field, with choices
//        dotnet run -- audit <dir> <port>   every screen: props whose text is only a type name; CornerRadius round trips
//        dotnet run -- colors <dir> <port>   color props of the first label as #AARRGGBB, TextColor set and read back
//        dotnet run -- tour <dir> <port>   any app: hello, screens, tree, props, setProperty, snapshot, every screen opened
//                                          in turn, then MINIMIZE (waits for <dir>\ack) and the same while hidden;
//                                          PNGs go to <dir>
//        dotnet run -- picks <dir> <port>   any app: pick mode with synthetic input from the input probe (<dir>\cmd.txt):
//                                           hover (prints HOVER for a capture), a picking click on a button that must not
//                                           reach the app, a normal click that must; Esc; then focus (prints FOCUS, waits)
//        dotnet run -- reload <seconds> <port> <route> <text>   opens the route, then waits for the control showing that
//                                           text to be rebuilt (hot reload): prints READY, then CHANGED <TickCount64>
//        dotnet run -- follow <dir> <port> <button text> [back button text]   trees follow navigation: every screen opened
//                                           from the hub, then taps in the app (input probe, <dir>\cmd.txt) on that
//                                           button and the back button; counts the trees each step brings
//        dotnet run -- passive <port> <seconds>   prints the trees the app pushes without being asked (TREE <TickCount64>)
//        dotnet run -- watch <seconds> [port]   polls getTree every 100 ms and reports when the controls are rebuilt
//                                               (hot reload): prints CHANGED <Environment.TickCount64>
// With syncDir, it prints CAPTURE <n> and waits for <syncDir>\ack (a capture tool watching the output), and asks
// a frame probe to scroll by creating <syncDir>\scroll.flag.
using System.Net;
using System.Net.Sockets;
using System.Net.WebSockets;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

var dump = args.Length >= 3 && args[0] == "dump";
var watch = args.Length >= 2 && args[0] == "watch";
var snap = args.Length >= 2 && args[0] == "snap";
var hidden = args.Length >= 2 && args[0] == "hidden";
var focus = args.Length >= 2 && args[0] == "focus";
var nav = args.Length >= 3 && args[0] == "nav";
var pick = args.Length >= 3 && args[0] == "pick";
var propsMode = args.Length >= 3 && args[0] == "props";
var tour = args.Length >= 3 && args[0] == "tour";
var picks = args.Length >= 3 && args[0] == "picks";
var reload = args.Length >= 5 && args[0] == "reload";
var follow = args.Length >= 4 && args[0] == "follow";
var passive = args.Length >= 3 && args[0] == "passive";
var colors = args.Length >= 3 && args[0] == "colors";
var audit = args.Length >= 3 && args[0] == "audit";
var revert = args.Length >= 3 && args[0] == "revert";
var hlprobe = args.Length >= 4 && args[0] == "hlprobe";
var gradient = args.Length >= 3 && args[0] == "gradient";
var scrollend = args.Length >= 4 && args[0] == "scrollend";
var cellhl = args.Length >= 4 && args[0] == "cellhl";
var controlsMode = args.Length >= 4 && args[0] == "controls";
var port = passive ? int.Parse(args[1]) : follow ? int.Parse(args[2]) : reload ? int.Parse(args[2]) : tour || picks || colors || audit || revert || hlprobe || gradient || scrollend || cellhl || controlsMode ? int.Parse(args[2]) : propsMode ? int.Parse(args[2]) : propsMode ? int.Parse(args[1]) : pick ? int.Parse(args[2]) : focus || nav ? int.Parse(args[1]) : hidden ? int.Parse(args[1]) : hidden ? (args.Length > 2 ? int.Parse(args[2]) : 54270) : snap ? (args.Length > 2 ? int.Parse(args[2]) : 54270) : watch ? (args.Length > 2 ? int.Parse(args[2]) : 54270) : dump ? (args.Length > 3 ? int.Parse(args[3]) : 54270) : args.Length > 0 ? int.Parse(args[0]) : 54270;
var route = colors || audit || revert || hlprobe || gradient || scrollend || cellhl || controlsMode || watch || snap || hidden || focus || nav || pick || propsMode || tour || picks || reload || follow || passive ? "" : dump ? args[2] : args.Length > 1 ? args[1] : "text";
var sync = !colors && !audit && !revert && !hlprobe && !gradient && !scrollend && !cellhl && !controlsMode && !dump && !watch && !snap && !hidden && !focus && !nav && !pick && !propsMode && !tour && !picks && !reload && !follow && !passive && args.Length > 2 ? args[2] : null;
var listener = new TcpListener(IPAddress.Loopback, port);
listener.Start();
Console.WriteLine($"hub listening on {port}");

using var tcp = await listener.AcceptTcpClientAsync();
var stream = tcp.GetStream();
var request = new StringBuilder();
var one = new byte[1];
while (!request.ToString().EndsWith("\r\n\r\n") && await stream.ReadAsync(one) == 1)
    request.Append((char)one[0]);
var key = request.ToString().Split("\r\n").First(l => l.StartsWith("Sec-WebSocket-Key:", StringComparison.OrdinalIgnoreCase))[18..].Trim();
var accept = Convert.ToBase64String(SHA1.HashData(Encoding.ASCII.GetBytes(key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11")));
await stream.WriteAsync(Encoding.ASCII.GetBytes($"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"));
using var ws = WebSocket.CreateFromStream(stream, true, null, TimeSpan.FromSeconds(30));

async Task<JsonNode> Receive(params string[] types)
{
    using var cts = new CancellationTokenSource(10000);
    var buffer = new byte[1 << 20];
    while (true)
    {
        using var ms = new MemoryStream();
        WebSocketReceiveResult r;
        do
        {
            r = await ws.ReceiveAsync(buffer, cts.Token);
            ms.Write(buffer, 0, r.Count);
        } while (!r.EndOfMessage);
        var text = Encoding.UTF8.GetString(ms.ToArray());
        var node = JsonNode.Parse(text);
        var t = (string)(node["type"] ?? node["role"]);
        Console.WriteLine($"<- {t} ({text.Length} bytes){(text.Length < 300 ? " " + text : "")}");
        if (types.Contains(t))
            return node;
    }
}

Task Send(object msg)
{
    var json = JsonSerializer.Serialize(msg);
    Console.WriteLine("-> " + json);
    return ws.SendAsync(Encoding.UTF8.GetBytes(json), WebSocketMessageType.Text, true, CancellationToken.None);
}

static string Prop(JsonNode props, string name) =>
    props["props"]?.AsArray().FirstOrDefault(p => (string)p["name"] == name)?["value"]?.ToString();

void Summary(JsonNode tree, int bytes)
{
    var nodes = tree["nodes"].AsArray();
    Console.WriteLine($"tree rev {tree["revision"]} size {tree["width"]}x{tree["height"]} nodes {nodes.Count} frame {bytes} bytes " +
                      $"visible {nodes.Count(n => (bool)n["visible"])}");
    foreach (var g in nodes.GroupBy(n => ((string)n["typeName"]).Split('.').Last()).OrderByDescending(g => g.Count()).Take(8))
        Console.WriteLine($"   {g.Key} x{g.Count()}");
}

async Task<JsonNode> Tree()
{
    await Send(new { type = "getTree" });
    return await Receive("tree");
}

async Task<JsonNode> Props(long node)
{
    await Send(new { type = "getProps", node });
    return await Receive("props", "error");
}

async Task Capture(int n)
{
    if (sync == null)
        return;
    Console.WriteLine($"CAPTURE {n}");
    var ack = Path.Combine(sync, "ack");
    for (var i = 0; i < 300 && !File.Exists(ack); i++)
        await Task.Delay(100);
    File.Delete(ack);
}

void Phase(string name) => Console.WriteLine($"PHASE {name} {Environment.TickCount64}");

if (controlsMode)
{
    // controls <dir> <port> <route|->: the controls list, a live insert and remove, a refused insert, place mode on/off
    await Receive("app");
    await Receive("screens");
    var kt = await Receive("tree");
    if (args[3] != "-")
    {
        await Send(new { type = "openScreen", route = args[3] });
        kt = await Receive("tree");
    }
    await Send(new { type = "getControls" });
    var ks = (await Receive("controls"))["controls"].AsArray();
    Console.WriteLine($"controls: {ks.Count}; " + string.Join(", ", ks.GroupBy(k => (string)k["group"]).Select(g => $"{g.Key} {g.Count()}")));
    foreach (var g in ks.Where(k => !(bool)k["own"]).GroupBy(k => (string)k["group"]))
        Console.WriteLine($"library {g.Key}: " + string.Join(", ", g.Select(k => ((string)k["typeName"]).Replace("DrawnUi.", ""))));
    Console.WriteLine("own: " + string.Join(", ", ks.Where(k => (bool)k["own"]).Select(k => $"{k["typeName"]} ({k["group"]}{((bool)k["container"] ? ", container" : "")})")));
    foreach (var k in ks.Where(k => ((string)k["typeName"]).Split('.').Last() is "SkiaLabel" or "SkiaButton" or "SkiaStack" or "SkiaShape" or "SkiaImage" or "SkiaSlider"))
        Console.WriteLine($"   {((string)k["typeName"]).Split('.').Last()}: {k["group"]}, container {k["container"]}, defaults {k["defaults"].ToJsonString()}");
    var kn = kt["nodes"].AsArray();
    foreach (var cand in kn.Where(n => !(bool)n["internal"] && n["templateIndex"] == null && ((string)n["typeName"]).Split('.').Last() is "SkiaStack" or "SkiaLayout" or "SkiaLayer" or "SkiaRow"))
    {
        await Send(new { type = "insert", request = 20, parent = (long)cand["id"], index = 0, typeName = "DrawnUi.Draw.SkiaLabel" });
        var got = await Receive("inserted", "error");
        if ((string)got["type"] == "error")
        {
            Console.WriteLine($"insert into {cand["id"]} {((string)cand["typeName"]).Split('.').Last()} refused: {got["message"]}");
            await Receive("inserted");
            continue;
        }
        var afterInsert = await Receive("tree");
        var shown = afterInsert["nodes"].AsArray().FirstOrDefault(n => (long?)n["id"] == (long?)got["node"]);
        Console.WriteLine($"inserted node {got["node"]} into {cand["id"]} at {got["index"]}: tree shows {shown?["typeName"]} \"{shown?["text"]}\" visible {shown?["visible"]} rect {shown?["rect"]?.ToJsonString()}");
        await Send(new { type = "snapshot", request = 21 });
        var ish = await Receive("snapshot", "error");
        if ((string)ish["type"] == "snapshot")
            File.WriteAllBytes(Path.Combine(args[1], "inserted.png"), Convert.FromBase64String((string)ish["pngBase64"]));
        await Send(new { type = "remove", request = 22, node = (long)got["node"] });
        var gone = await Receive("tree", "error");
        Console.WriteLine($"remove -> {((string)gone["type"] == "error" ? "ERROR " + gone["message"] : gone["nodes"].AsArray().Any(n => (long?)n["id"] == (long?)got["node"]) ? "still there" : "gone")}");
        break;
    }
    var templated = kn.FirstOrDefault(n => kn.Any(c => c["templateIndex"] != null && (long?)c["parent"] == (long?)n["id"]));
    if (templated != null)
    {
        await Send(new { type = "insert", request = 23, parent = (long)templated["id"], typeName = "DrawnUi.Draw.SkiaLabel" });
        var rf = await Receive("error");
        await Receive("inserted");
        Console.WriteLine($"insert into the templated {((string)templated["typeName"]).Split('.').Last()}: {rf["message"]}");
    }
    await Send(new { type = "placeMode", typeName = "DrawnUi.Draw.SkiaShape" });
    await Send(new { type = "placeMode", typeName = (string)null });
    await Send(new { type = "getTree" });
    await Receive("tree");
    Console.WriteLine("place mode on, then off: the app still answers");
    return;
}

if (cellhl)
{
    // cellhl <dir> <port> <route>: highlight a recycled cell (and a plain control), snapshot each, pick at the cell's centre
    await Receive("app");
    await Receive("screens");
    await Receive("tree");
    await Send(new { type = "openScreen", route = args[3] });
    var ct = await Receive("tree");
    if (args.Length > 4 && args[4] == "scrolled")
    {
        // scroll the list first (probe build), wait for the tree pushed after it comes to rest
        var all = ct["nodes"].AsArray();
        var byId = all.ToDictionary(n => (long)n["id"]);
        var firstCell = all.First(n => n["templateIndex"] != null);
        var up = firstCell;
        while (up != null && !((string)up["typeName"]).EndsWith("Scroll"))
            up = up["parent"] == null ? null : byId[(long)up["parent"]];
        var scrollNode = up ?? throw new InvalidOperationException("no scroll above the cells");
        await Send(new { type = "probe-scroll", request = 93, node = (long)scrollNode["id"] });
        ct = await Receive("tree");
    }
    var cn = ct["nodes"].AsArray();
    var cell = cn.First(n => n["templateIndex"] != null && (bool)n["visible"]);
    var plain = cn.First(n => n["templateIndex"] == null && n["text"] != null && (bool)n["visible"]);
    // a text control inside the cell (what a row like "item 1" in Layers usually is)
    var inCell = new HashSet<long> { (long)cell["id"] };
    foreach (var m in cn)
        if (m["parent"] != null && inCell.Contains((long)m["parent"]))
            inCell.Add((long)m["id"]);
    var cellText = cn.First(n => (long)n["id"] != (long)cell["id"] && inCell.Contains((long)n["id"]) && n["text"] != null);
    foreach (var (node, file) in new[] { (cell, "cell.png"), (cellText, "celltext.png"), (plain, "plain.png") })
    {
        Console.WriteLine($"node {node["id"]} {((string)node["typeName"]).Split('.').Last()} templateIndex {node["templateIndex"]} text \"{node["text"]}\" rect {node["rect"].ToJsonString()} visible {node["visible"]}");
        await Send(new { type = "highlight", node = (long)node["id"] });
        await Task.Delay(400);
        await Send(new { type = "probe-hl", request = 90, node = (long)node["id"] });
        var info = await Receive("error");
        Console.WriteLine("   probe: " + info["message"]);
        // the box is drawn on the app's own frames, not in the snapshot: a window capture is taken from outside
        Console.WriteLine($"SHOT {file}");
        var ack = Path.Combine(args[1], "ack");
        for (var i = 0; i < 300 && !File.Exists(ack); i++)
            await Task.Delay(100);
        File.Delete(ack);
    }
    var cr = cell["rect"];
    await Send(new { type = "pick", request = 92, x = (float)cr[0] + (float)cr[2] / 2, y = (float)cr[1] + (float)cr[3] / 2 });
    var pk = await Receive("picked");
    var pickedNode = cn.FirstOrDefault(n => (long?)n["id"] == (long?)pk["node"]);
    Console.WriteLine($"pick at the cell's centre -> node {pk["node"]} {pickedNode?["typeName"]} templateIndex {pickedNode?["templateIndex"]}");
    await Send(new { type = "highlight", node = (long?)null });
    return;
}

if (scrollend)
{
    // scrollend <dir> <port> <route|->: scrolls the first list with recycled cells to its end (needs a probe build with
    // probe-scroll) and lists every tree that arrives: one, after the scroll stops, with other ContextIndex values
    static string Cells(JsonNode t)
    {
        var idx = t["nodes"].AsArray().Where(n => n["templateIndex"] != null).Select(n => (long)n["templateIndex"]).Distinct().OrderBy(i => i).ToList();
        return idx.Count == 0 ? "no cells" : $"cells {idx.Count}, ContextIndex {idx.First()}..{idx.Last()}";
    }
    await Receive("app");
    await Receive("screens");
    var st = await Receive("tree");
    if (args[3] != "-")
    {
        await Send(new { type = "openScreen", route = args[3] });
        st = await Receive("tree");
    }
    var sn = st["nodes"].AsArray();
    var cellParents = sn.Where(n => n["templateIndex"] != null).Select(n => (long?)n["parent"]).ToHashSet();
    JsonNode list = null;
    foreach (var n in sn.Where(n => ((string)n["typeName"]).EndsWith("Scroll")))
    {
        // the scroll above the cells' layout
        var ids = new HashSet<long?> { (long)n["id"] };
        foreach (var m in sn)
            if (ids.Contains((long?)m["parent"]))
                ids.Add((long)m["id"]);
        if (cellParents.Overlaps(ids)) { list = n; break; }
    }
    if (list == null) { Console.WriteLine("no scroll with recycled cells on this screen"); return; }
    Console.WriteLine($"scroll {list["id"]} {((string)list["typeName"]).Split('.').Last()}; before: {Cells(st)}");
    await Task.Delay(1000);
    var t0 = System.Diagnostics.Stopwatch.StartNew();
    await Send(new { type = "probe-scroll", request = 80, node = (long)list["id"] });
    using (var cts = new CancellationTokenSource(5000))
    {
        var buf = new byte[1 << 22];
        try
        {
            while (true)
            {
                using var ms = new MemoryStream();
                WebSocketReceiveResult part;
                do { part = await ws.ReceiveAsync(buf, cts.Token); ms.Write(buf, 0, part.Count); } while (!part.EndOfMessage);
                var msg = JsonNode.Parse(Encoding.UTF8.GetString(ms.ToArray()));
                var mt = (string)msg["type"];
                Console.WriteLine($"{t0.ElapsedMilliseconds,5} ms <- {mt}{(mt == "tree" ? ": " + Cells(msg) : mt == "error" ? ": " + msg["message"] : "")}");
            }
        }
        catch (OperationCanceledException)
        {
        }
    }
    Console.WriteLine("5 s after the scroll started: done");
    return;
}

if (gradient)
{
    // gradient <dir> <port>: the first FillGradient on the cards screen, read, changed, removed and put back
    await Receive("app");
    await Receive("screens");
    await Receive("tree");
    await Send(new { type = "openScreen", route = "cards" });
    var gt = await Receive("tree");
    long gid = 0;
    string original = null;
    foreach (var n in gt["nodes"].AsArray())
    {
        var fg = (await Props((long)n["id"]))["props"]?.AsArray().FirstOrDefault(x => (string)x["name"] == "FillGradient");
        if (fg != null && (string)fg["value"] is { Length: > 0 } v)
        {
            gid = (long)n["id"];
            original = v;
            Console.WriteLine($"node {gid} {n["typeName"]} FillGradient ({fg["typeName"]}, isSet {fg["isSet"]}, isDefault {fg["isDefault"]}) = {v}");
            break;
        }
    }
    async Task Shot(string file)
    {
        await Send(new { type = "snapshot", request = 70 });
        var sh = await Receive("snapshot", "error");
        if ((string)sh["type"] == "snapshot")
            File.WriteAllBytes(Path.Combine(args[1], file), Convert.FromBase64String((string)sh["pngBase64"]));
    }
    async Task SetG(string value)
    {
        await Send(new { type = "setProperty", request = 71, node = gid, name = "FillGradient", value });
        var back = await Props(gid);
        var failed = (string)back["type"] == "error";
        if (failed)
            back = await Receive("props"); // the props that follow the error
        Console.WriteLine($"set {value} -> {(failed ? "ERROR, then reads " : "")}{Prop(back, "FillGradient")}");
    }
    await Shot("g0.png");
    await SetG("{\"type\":\"Linear\",\"colors\":[\"#FFFF0000\",\"#FF0000FF\"],\"stops\":null,\"angle\":90}");
    await Shot("g1.png");
    await SetG("{\"type\":\"Circular\",\"colors\":[\"#FFFFFF00\",\"#FF00FF00\",\"#FF000000\"],\"stops\":[0,0.3,1]}");
    await Shot("g2.png");
    await SetG("{not json");
    await SetG("");
    await Shot("g3.png");
    await SetG(original);
    await Shot("g4.png");
    return;
}

if (hlprobe)
{
    // highlight cost while a scroll moves: hlprobe <dir> <port> <route with a SkiaScroll>; the agent must be a probe build
    await Receive("app");
    await Receive("screens");
    await Receive("tree");
    await Send(new { type = "openScreen", route = args[3] });
    var ht = await Receive("tree");
    var hn = ht["nodes"].AsArray();
    var scroll = hn.First(n => ((string)n["typeName"]).EndsWith("Scroll"));
    var inside = hn.Where(n => ((string)n["typeName"]).EndsWith("Label") && (bool)n["visible"]).Skip(2).First();
    Console.WriteLine($"scroll {scroll["id"]} {scroll["typeName"]}, highlight {inside["id"]} \"{inside["text"]}\"");
    await Send(new { type = "probe", request = 60, node = (long)inside["id"] });
    Console.WriteLine("micro: " + (await Receive("error"))["message"]);
    await Send(new { type = "scrollprobe", request = 61, node = (long)inside["id"], scroll = (long)scroll["id"] });
    using (var waitLong = new CancellationTokenSource(15000))
    {
        var buf = new byte[1 << 20];
        while (true)
        {
            var got = await ws.ReceiveAsync(buf, waitLong.Token);
            var gotText = Encoding.UTF8.GetString(buf, 0, got.Count);
            var gotType = (string)JsonNode.Parse(gotText)["type"];
            Console.WriteLine($"<- {gotType} during the scroll");
            if (gotType == "error") { Console.WriteLine("live: " + JsonNode.Parse(gotText)["message"]); break; }
        }
    }
    return;
}

if (revert)
{
    // runtime state finder: every editable prop of a few controls is set to another value; a prop the engine puts back
    // (or keeps from changing) after a frame is listed; the original value is restored after each try
    await Receive("app");
    await Receive("screens");
    var rt = await Receive("tree");
    var picksR = rt["nodes"].AsArray().GroupBy(n => ((string)n["typeName"]).Split('.').Last()).Select(g => g.First()).Take(8).ToList();
    var found = new SortedDictionary<string, string>();
    foreach (var n in picksR)
    {
        var nodeId = (long)n["id"];
        var ps0 = (await Props(nodeId))["props"]?.AsArray();
        if (ps0 == null)
            continue;
        foreach (var x in ps0)
        {
            var pname = (string)x["name"];
            var tn = (string)x["typeName"];
            var old = (string)x["value"] ?? "";
            string other = tn switch
            {
                "System.Boolean" => old == "true" ? "false" : "true",
                "System.Double" or "System.Single" => old.Length == 0 ? "3" : (double.Parse(old, System.Globalization.CultureInfo.InvariantCulture) + 3).ToString(System.Globalization.CultureInfo.InvariantCulture),
                "System.Int32" => old.Length == 0 ? "3" : (int.Parse(old) + 3).ToString(),
                "System.String" => old + "X",
                _ when x["choices"] is JsonArray ch && ch.Count > 1 => (string)ch.First(c => (string)c != old),
                _ when tn.EndsWith("Color") => old == "#FF123456" ? "#FF654321" : "#FF123456",
                _ when tn.EndsWith("Thickness") => "1,2,3,4",
                _ => null,
            };
            if (other == null || pname is "IsVisible" or "Opacity" or "AutoSize")
                continue;
            await Send(new { type = "setProperty", request = 50, node = nodeId, name = pname, value = other });
            await Task.Delay(150);
            await Tree(); // a frame
            var now = Prop(await Props(nodeId), pname);
            if (now != other)
                found[$"{pname}"] = $"{n["typeName"].ToString().Split('.').Last()}: was {old}, set {other}, reads {now}{(now == old ? " (put back)" : "")}";
            await Send(new { type = "setProperty", request = 51, node = nodeId, name = pname, value = old });
        }
    }
    Console.WriteLine($"props that did not keep the value set: {found.Count}");
    foreach (var f in found)
        Console.WriteLine($"   {f.Key}  ({f.Value})");
    return;
}

if (audit)
{
    // every screen, every node: properties whose text is only a type name (no useful ToString); then CornerRadius round trips
    await Receive("app");
    var ascr = (await Receive("screens"))["screens"].AsArray().Select(x => (string)x["route"]).ToList();
    await Receive("tree");
    var bad = new SortedSet<string>();
    var allNames = new SortedSet<string>();
    var seen = new SortedSet<string>();
    (long Id, string Type)? cornered = null;
    foreach (var screenRoute in ascr)
    {
        await Send(new { type = "openScreen", route = screenRoute });
        var at = await Receive("tree");
        foreach (var n in at["nodes"].AsArray())
        {
            var ap = await Props((long)n["id"]);
            foreach (var x in ap["props"]?.AsArray() ?? new JsonArray())
            {
                var tn = (string)x["typeName"];
                var v = (string)x["value"];
                allNames.Add($"{((string)n["typeName"]).Split('.').Last()}.{x["name"]}");
                if (!tn.StartsWith("System.") || tn.StartsWith("System.Nullable"))
                    seen.Add($"{tn.Split('.').Last()} e.g. {x["name"]}={v}");
                if (v == tn || v == tn.Split('.').Last() || (v != null && v.StartsWith("[") && v.Contains(':')))
                    bad.Add($"{tn} ({x["name"]}) = {v}");
                if (tn.EndsWith(".CornerRadius") && cornered == null)
                    cornered = ((long)n["id"], (string)n["typeName"]);
            }
        }
    }
    File.WriteAllLines(Path.Combine(args[1], "names.txt"), allNames);
    Console.WriteLine("struct and other non-System property types seen:");
    foreach (var x in seen.GroupBy(x => x.Split(' ')[0]).Select(g => g.First()))
        Console.WriteLine("   " + x);
    Console.WriteLine($"values that are only a type name: {bad.Count}");
    foreach (var b in bad)
        Console.WriteLine("   " + b);
    var dt = await Tree();
    var dl = dt["nodes"].AsArray().First(n => ((string)n["typeName"]).EndsWith("Label") && n["text"] != null);
    var dps = (await Props((long)dl["id"]))["props"].AsArray();
    Console.WriteLine($"isDefault on {dl["typeName"]} \"{dl["text"]}\": true {dps.Count(x => (bool?)x["isDefault"] == true)}, false {dps.Count(x => (bool?)x["isDefault"] == false)}, null {dps.Count(x => x["isDefault"] == null)}");
    foreach (var name in new[] { "ExpandDirtyRegion", "IsEnabled", "InputTransparent", "IsHovered", "Text", "FontSize" })
        if (dps.FirstOrDefault(x => (string)x["name"] == name) is { } q)
            Console.WriteLine($"   {name} = {q["value"]}  isSet {q["isSet"]}  isDefault {q["isDefault"]}");
    Console.WriteLine("   set and not default: " + string.Join(", ", dps.Where(x => (bool)x["isSet"] && (bool?)x["isDefault"] == false).Select(x => $"{x["name"]}={x["value"]}")));
    Console.WriteLine("   set but default: " + string.Join(", ", dps.Where(x => (bool)x["isSet"] && (bool?)x["isDefault"] == true).Select(x => (string)x["name"])));
    var originNode = (long)dt["nodes"][0]["id"];
    var hasOrigin = Prop(await Props(originNode), "TransformOrigin") != null;
    foreach (var v in hasOrigin ? new[] { "0.3,0.7", "0.5,0.5" } : Array.Empty<string>())
    {
        await Send(new { type = "setProperty", request = 41, node = originNode, name = "TransformOrigin", value = v });
        Console.WriteLine($"root set TransformOrigin = {v} -> reads {Prop(await Props(originNode), "TransformOrigin")}");
    }
    if (cornered is { } c)
        foreach (var v in new[] { "4,8,12,16", "6", "0" })
        {
            await Send(new { type = "setProperty", request = 40, node = c.Id, name = "CornerRadius", value = v });
            Console.WriteLine($"{c.Type.Split('.').Last()} set CornerRadius = {v} -> reads {Prop(await Props(c.Id), "CornerRadius")}");
        }
    return;
}

if (colors)
{
    // the color properties of the first visible label as the agent sends them (#AARRGGBB), then TextColor set to red
    await Receive("app");
    await Receive("screens");
    var ct = await Receive("tree");
    var lbl = ct["nodes"].AsArray().First(n => ((string)n["typeName"]).EndsWith("Label") && n["text"] != null && (bool)n["visible"]);
    var cps = (await Props((long)lbl["id"]))["props"].AsArray();
    foreach (var x in cps.Where(x => ((string)x["typeName"]).EndsWith("Color")))
        Console.WriteLine($"   {x["name"]} ({x["typeName"]}) = {x["value"]}");
    foreach (var v in new[] { "#FFFF0000", "#8000FF00", "#00F" })
    {
        await Send(new { type = "setProperty", request = 30, node = (long)lbl["id"], name = "TextColor", value = v });
        Console.WriteLine($"set TextColor = {v} -> reads {Prop(await Props((long)lbl["id"]), "TextColor")}");
    }
    await Send(new { type = "setProperty", request = 31, node = (long)lbl["id"], name = "TextColor", value = "#FFFF0000" });
    await Send(new { type = "snapshot", request = 32 });
    var cs = await Receive("snapshot", "error");
    File.WriteAllBytes(Path.Combine(args[1], "red.png"), Convert.FromBase64String((string)cs["pngBase64"]));
    Console.WriteLine($"label rect {lbl["rect"]}; snapshot red.png");
    return;
}

if (passive)
{
    // asks nothing after the start: prints every tree the app pushes by itself, with its time
    await Receive("app");
    await Receive("screens");
    await Receive("tree");
    Console.WriteLine("READY");
    var stop = DateTime.UtcNow.AddSeconds(int.Parse(args[2]));
    while (DateTime.UtcNow < stop)
    {
        try
        {
            var t = await Receive("tree");
            Console.WriteLine($"TREE {Environment.TickCount64} {t["nodes"].AsArray().Count} nodes, revision {t["revision"]}");
        }
        catch (OperationCanceledException)
        {
            break; // 10 s without a tree: Receive gave up
        }
    }
    return;
}

if (follow)
{
    // every frame goes into a queue, so a step can wait for trees with a timeout without cancelling a receive
    var frames = System.Threading.Channels.Channel.CreateUnbounded<JsonNode>();
    _ = Task.Run(async () =>
    {
        var buffer = new byte[1 << 20];
        try
        {
            while (true)
            {
                using var ms = new MemoryStream();
                WebSocketReceiveResult r;
                do
                {
                    r = await ws.ReceiveAsync(buffer, CancellationToken.None);
                    ms.Write(buffer, 0, r.Count);
                } while (!r.EndOfMessage);
                await frames.Writer.WriteAsync(JsonNode.Parse(Encoding.UTF8.GetString(ms.ToArray())));
            }
        }
        catch
        {
            frames.Writer.TryComplete();
        }
    });
    async Task<List<JsonNode>> Trees(int quietMs, int firstMs)
    {
        // the trees that arrive: wait up to firstMs for the first, then until quietMs pass with none
        var got = new List<JsonNode>();
        var deadline = DateTime.UtcNow.AddMilliseconds(firstMs);
        while (true)
        {
            var wait = got.Count == 0 ? deadline - DateTime.UtcNow : TimeSpan.FromMilliseconds(quietMs);
            if (wait <= TimeSpan.Zero)
                return got;
            using var cts = new CancellationTokenSource(wait);
            try
            {
                var frame = await frames.Reader.ReadAsync(cts.Token);
                if ((string)frame["type"] == "tree")
                    got.Add(frame);
            }
            catch (OperationCanceledException)
            {
                return got;
            }
        }
    }
    string Shown(JsonNode t) => $"{t["nodes"].AsArray().Count} nodes, \"{t["nodes"].AsArray().FirstOrDefault(n => n["text"] != null && (bool)n["visible"])?["text"]}\"";
    string Report(List<JsonNode> trees) => trees.Count == 0 ? "NO TREE" : $"{trees.Count} tree(s), last: {Shown(trees[^1])}";
    JsonNode lastTree = null;
    async Task Tap(string text)
    {
        bool Shows(JsonNode n) => ((string)n["text"])?.EndsWith(text) == true && (bool)n["visible"];
        var target = lastTree["nodes"].AsArray().LastOrDefault(n => Shows(n) && (bool?)n["internal"] != true)
                     ?? lastTree["nodes"].AsArray().LastOrDefault(Shows);
        if (target == null)
        {
            Console.WriteLine($"no visible control shows \"{text}\"");
            return;
        }
        var file = Path.Combine(args[1], "cmd.txt");
        File.WriteAllText(file, FormattableString.Invariant($"click {(float)target["rect"][0] + (float)target["rect"][2] / 2} {(float)target["rect"][1] + (float)target["rect"][3] / 2}"));
        for (var i = 0; i < 100 && File.Exists(file); i++)
            await Task.Delay(30);
    }

    async Task<JsonNode> Next(string type)
    {
        while (true)
        {
            var frame = await frames.Reader.ReadAsync();
            if ((string)frame["type"] == type || (string)frame["role"] == type)
                return frame;
        }
    }
    await Next("app");
    var fs = await Next("screens");
    lastTree = await Next("tree");
    Console.WriteLine($"start: {Shown(lastTree)}");
    var froutes = fs["screens"].AsArray().Select(x => (string)x["route"]).ToList();
    foreach (var fr in froutes.Skip(1).Concat(froutes.Take(1)))
    {
        await Send(new { type = "openScreen", route = fr });
        var trees = await Trees(1500, 6000);
        if (trees.Count > 0)
            lastTree = trees[^1];
        Console.WriteLine($"openScreen({fr}) -> {Report(trees)}");
    }
    await Tap(args[3]);
    var tapped = await Trees(1500, 6000);
    if (tapped.Count > 0)
        lastTree = tapped[^1];
    Console.WriteLine($"tap \"{args[3]}\" in the app -> {Report(tapped)}");
    if (args.Length > 4)
    {
        await Tap(args[4]);
        var back = await Trees(1500, 6000);
        if (back.Count > 0)
            lastTree = back[^1];
        Console.WriteLine($"tap \"{args[4]}\" in the app -> {Report(back)}");
    }
    var idle = await Trees(1, 2000);
    Console.WriteLine($"idle 2 s -> {idle.Count} tree(s)");
    Console.WriteLine("DONE");
    return;
}

if (reload)
{
    var rh = await Receive("app");
    Console.WriteLine($"hello: {rh["name"]} {rh["engine"]} pid {rh["pid"]}");
    await Receive("screens");
    await Receive("tree");
    await Send(new { type = "openScreen", route = args[3] });
    await Task.Delay(1500);
    JsonNode Shown(JsonNode t) => t["nodes"].AsArray().FirstOrDefault(n => (string)n["text"] == args[4]);
    var shown = Shown(await Tree());
    if (shown == null)
    {
        Console.WriteLine($"no control shows \"{args[4]}\"");
        return;
    }
    Console.WriteLine($"\"{args[4]}\": node {shown["id"]} FontSize={Prop(await Props((long)shown["id"]), "FontSize")}");
    Console.WriteLine("READY");
    var until = DateTime.UtcNow.AddSeconds(int.Parse(args[1]));
    var reopened = false;
    var edited = false;
    while (DateTime.UtcNow < until)
    {
        await Task.Delay(200);
        var now = Shown(await Tree());
        if (now != null && (long)now["id"] != (long)shown["id"])
        {
            Console.WriteLine($"CHANGED {Environment.TickCount64} node {now["id"]} FontSize={Prop(await Props((long)now["id"]), "FontSize")}{(reopened ? " (after reopening the screen)" : "")}");
            return;
        }
        edited |= File.Exists(Path.Combine(Path.GetTempPath(), "dmt-edited"));
        if (edited && !reopened && File.GetLastWriteTimeUtc(Path.Combine(Path.GetTempPath(), "dmt-edited")) < DateTime.UtcNow.AddSeconds(-12))
        {
            reopened = true;
            Console.WriteLine($"no rebuild in place 12 s after the save; reopening {args[3]}");
            await Send(new { type = "openScreen", route = "MainPage" });
            await Task.Delay(1200);
            await Send(new { type = "openScreen", route = args[3] });
            await Task.Delay(1200);
        }
    }
    // no rebuild in place: open the screen again, which builds the page anew with the new code
    Console.WriteLine("NO CHANGE");
    return;
}

if (picks)
{
    async Task Cmd(string command)
    {
        var file = Path.Combine(args[1], "cmd.txt");
        File.WriteAllText(file, command);
        for (var i = 0; i < 100 && File.Exists(file); i++)
            await Task.Delay(30);
        await Task.Delay(300);
    }
    async Task Wait(string marker)
    {
        Console.WriteLine(marker);
        var ack = Path.Combine(args[1], "ack");
        for (var i = 0; i < 300 && !File.Exists(ack); i++)
            await Task.Delay(100);
        File.Delete(ack);
    }
    string Center(JsonNode n) => FormattableString.Invariant($"{(float)n["rect"][0] + (float)n["rect"][2] / 2} {(float)n["rect"][1] + (float)n["rect"][3] / 2}");
    string Page(JsonNode t) => $"{t["nodes"].AsArray().Count} nodes, first text \"{t["nodes"].AsArray().FirstOrDefault(n => n["text"] != null)?["text"]}\"";

    await Receive("app");
    var picksScreens = await Receive("screens");
    var pt = await Receive("tree");
    var pn = pt["nodes"].AsArray();
    var button = pn.First(n => ((string)n["typeName"]).EndsWith("Button") && (bool?)n["internal"] != true && (bool)n["visible"] && n["text"] != null);
    var heading = pn.First(n => ((string)n["typeName"]).EndsWith("Label") && (bool)n["visible"] && n["text"] != null && (bool?)n["internal"] != true);
    Console.WriteLine($"start: {Page(pt)}; button {button["id"]} \"{button["text"]}\", label {heading["id"]} \"{heading["text"]}\"");

    await Send(new { type = "pickMode", on = true });
    await Cmd("move " + Center(heading));
    await Wait("HOVER");
    await Cmd("click " + Center(button));
    var got = await Receive("picked");
    Console.WriteLine($"picked {got["node"]} (button {button["id"]}): {((long?)got["node"] == (long)button["id"] ? "MATCH" : "DIFFERENT")}");
    await Task.Delay(800);
    Console.WriteLine($"after the picking click: {Page(await Tree())}");
    await Send(new { type = "highlight", node = (long?)null });

    await Cmd("click " + Center(button));
    await Task.Delay(1500);
    Console.WriteLine($"after a normal click: {Page(await Tree())}");
    await Send(new { type = "openScreen", route = (string)picksScreens["screens"][0]["route"] });
    await Task.Delay(1200);

    await Send(new { type = "pickMode", on = true });
    await Cmd("esc");
    var esc = await Receive("picked", "error").WaitAsync(TimeSpan.FromSeconds(3)).ContinueWith(t => t.IsCompletedSuccessfully ? t.Result : null);
    Console.WriteLine(esc == null ? "Esc: no answer" : $"Esc: picked node {(esc["node"] == null ? "null" : esc["node"].ToString())}");
    await Send(new { type = "pickMode", on = false });

    await Wait("FOCUS");
    await Send(new { type = "focus" });
    await Task.Delay(1000);
    await Wait("FOCUSED");
    Console.WriteLine("DONE");
    return;
}

if (tour)
{
    async Task<long> TourShot(string file)
    {
        var clock = System.Diagnostics.Stopwatch.StartNew();
        await Send(new { type = "snapshot", request = 9 });
        var shot = await Receive("snapshot", "error");
        if ((string)shot["type"] != "snapshot")
            return -1;
        File.WriteAllBytes(Path.Combine(args[1], file), Convert.FromBase64String((string)shot["pngBase64"]));
        Console.WriteLine($"snapshot {file}: {shot["width"]}x{shot["height"]} in {clock.ElapsedMilliseconds} ms");
        return clock.ElapsedMilliseconds;
    }
    void TourSummary(string tlabel, JsonNode t)
    {
        var all = t["nodes"].AsArray();
        var pages = all.Select(n => ((string)n["typeName"]).Split('.').Last()).Where(n => n.EndsWith("Page") || n.EndsWith("Cards")).Distinct();
        Console.WriteLine($"{tlabel}: canvas {t["width"]}x{t["height"]}, {all.Count} nodes, {all.Count(n => (bool)n["visible"])} visible, " +
                          $"{all.Count(n => n["field"] != null)} with field, {all.Count(n => (bool?)n["internal"] == true)} internal; " +
                          $"texts: {string.Join(" | ", all.Where(n => n["text"] != null && (bool)n["visible"]).Take(4).Select(n => (string)n["text"]))}");
    }

    var hi = await Receive("app");
    Console.WriteLine($"hello: {hi["name"]} engine {hi["engine"]} platform {hi["platform"]} pid {hi["pid"]}\n   project {hi["project"]}\n   agent {hi["agent"]}");
    var scr = await Receive("screens");
    var routes = scr["screens"].AsArray().Select(x => (string)x["route"]).ToList();
    Console.WriteLine($"screens: {string.Join(", ", scr["screens"].AsArray().Select(x => $"{x["route"]}={x["title"]}"))}");
    var t0 = await Receive("tree");
    TourSummary("tree", t0);
    var tlabel = t0["nodes"].AsArray().First(n => ((string)n["typeName"]).EndsWith("Label") && n["text"] != null && (bool)n["visible"]);
    var lp = (await Props((long)tlabel["id"]))["props"].AsArray();
    Console.WriteLine($"props of {tlabel["typeName"]} \"{tlabel["text"]}\": {lp.Count}; groups " +
                      string.Join(", ", lp.GroupBy(x => (string)x["group"]).Select(g => $"{g.Key} {g.Count()}")));
    await Send(new { type = "setProperty", request = 3, node = (long)tlabel["id"], name = "Text", value = "DESIGNER-OK" });
    Console.WriteLine($"setProperty Text -> getProps reads \"{Prop(await Props((long)tlabel["id"]), "Text")}\"");
    var lr = tlabel["rect"];
    await Send(new { type = "pick", request = 4, x = (float)lr[0] + (float)lr[2] / 2, y = (float)lr[1] + (float)lr[3] / 2 });
    var pk = await Receive("picked");
    Console.WriteLine($"pick at the tlabel's centre -> node {pk["node"]} (tlabel {tlabel["id"]})");
    await Send(new { type = "highlight", node = (long)tlabel["id"] });
    await TourShot("start.png");
    await Send(new { type = "highlight", node = (long?)null });

    foreach (var tr in routes.Skip(1).Concat(routes.Take(1)))
    {
        await Send(new { type = "openScreen", route = tr });
        await Task.Delay(1200);
        TourSummary($"after openScreen({tr})", await Tree());
    }
    await TourShot("back-home.png");

    Console.WriteLine("MINIMIZE");
    var ackFile = Path.Combine(args[1], "ack");
    for (var i = 0; i < 300 && !File.Exists(ackFile); i++)
        await Task.Delay(100);
    File.Delete(ackFile);
    await Task.Delay(800);
    var hiddenRoute = routes.Count > 1 ? routes[^1] : routes[0];
    await Send(new { type = "openScreen", route = hiddenRoute });
    TourSummary($"hidden, after openScreen({hiddenRoute})", await Tree());
    await TourShot("hidden.png");
    await Send(new { type = "openScreen", route = routes[0] });
    TourSummary($"hidden, after openScreen({routes[0]})", await Tree());
    Console.WriteLine("DONE");
    return;
}

if (propsMode)
{
    var helloApp = await Receive("app");
    Console.WriteLine($"hello agent: {helloApp["agent"]}");
    await Receive("screens");
    var shownTree = await Receive("tree");
    var holder = shownTree["nodes"].AsArray().First(n => ((string)n["field"] ?? "").EndsWith(args[2]));
    var all = (await Props((long)holder["id"]))["props"].AsArray();
    Console.WriteLine($"{holder["typeName"]} {holder["field"]}: {all.Count} props, {all.Count(x => x["choices"] != null)} with choices");
    foreach (var x in all.Where(x => x["choices"] != null))
        Console.WriteLine($"   {x["name"]} = {x["value"]}  [{string.Join(", ", x["choices"].AsArray().Select(c => (string)c))}]");
    foreach (var x in all.Where(x => (string)x["typeName"] == "System.Boolean").Take(3))
        Console.WriteLine($"   {x["name"]} (bool) choices = {(x["choices"] == null ? "null" : x["choices"].ToJsonString())}");
    Console.WriteLine($"flags: {string.Join(", ", all.Where(x => (bool?)x["flags"] == true).Select(x => (string)x["name"]))}");
    // then: name=value pairs set in turn, each read back
    foreach (var pair in args.Skip(3))
    {
        var (setName, setValue) = (pair[..pair.IndexOf('=')], pair[(pair.IndexOf('=') + 1)..]);
        await Send(new { type = "setProperty", request = 20, node = (long)holder["id"], name = setName, value = setValue });
        var back = (await Props((long)holder["id"]))["props"]?.AsArray().FirstOrDefault(x => (string)x["name"] == setName);
        Console.WriteLine($"set {setName} = \"{setValue}\" -> reads \"{back?["value"]}\"");
    }
    return;
}

if (pick)
{
    async Task Input(string command)
    {
        var file = Path.Combine(args[1], "cmd.txt");
        File.WriteAllText(file, command);
        for (var i = 0; i < 100 && File.Exists(file); i++)
            await Task.Delay(30);
        await Task.Delay(250); // the canvas processes input at the start of its next frame
    }
    async Task Click(float x, float y)
    {
        await Input(FormattableString.Invariant($"move {x} {y}"));
        await Input(FormattableString.Invariant($"down {x} {y}"));
        await Input(FormattableString.Invariant($"up {x} {y}"));
    }
    async Task<string> Status()
    {
        var t = await Tree();
        return (string)t["nodes"].AsArray().First(n => ((string)n["field"] ?? "").EndsWith("._status"))["text"];
    }
    string Short(JsonNode t, long? id) => id == null ? "none"
        : t["nodes"].AsArray().Where(n => (long)n["id"] == id).Select(n => $"{((string)n["typeName"]).Split('.').Last()} \"{n["text"]}\" ({n["id"]})").FirstOrDefault();

    await Receive("app");
    await Receive("screens");
    var tree0 = await Receive("tree");
    var nodes0 = tree0["nodes"].AsArray();

    // props filter and groups on the title label
    var title = nodes0.First(n => ((string)n["field"] ?? "").EndsWith("._title"));
    var p = await Props((long)title["id"]);
    var list = p["props"].AsArray();
    Console.WriteLine($"SkiaLabel props after the filter: {list.Count}");
    foreach (var g in list.GroupBy(x => (string)x["group"]).OrderBy(g => g.Key))
        Console.WriteLine($"   {g.Key} ({g.Count()}): {string.Join(", ", g.Select(x => (string)x["name"]))}");

    var count = nodes0.First(n => (string)n["typeName"] == "DrawnUi.Draw.SkiaButton" && (string)n["text"] == "Count");
    float bx = (float)count["rect"][0] + (float)count["rect"][2] / 2, by = (float)count["rect"][1] + (float)count["rect"][3] / 2;
    var titleRect = title["rect"];
    float tx = (float)titleRect[0] + (float)titleRect[2] / 2, ty = (float)titleRect[1] + (float)titleRect[3] / 2;
    Console.WriteLine($"status at start: \"{await Status()}\"");

    // 1. pick mode: hover the title, then click the Count button
    await Send(new { type = "pickMode", on = true });
    await Input(FormattableString.Invariant($"move {tx} {ty}"));
    Console.WriteLine("HOVER");
    await Task.Delay(1500); // a capture of the window may run now
    await Click(bx, by);
    var pickedMsg = await Receive("picked");
    Console.WriteLine($"picked: {Short(tree0, (long?)pickedMsg["node"])} (request {pickedMsg["request"]})");
    Console.WriteLine($"status after the pick click (the app must not have seen it): \"{await Status()}\"");

    // 2. pick mode is off again: a click reaches the app
    await Click(bx, by);
    Console.WriteLine($"status after a normal click: \"{await Status()}\"");

    // 3. Esc cancels
    await Send(new { type = "pickMode", on = true });
    await Input("esc");
    var cancelled = await Receive("picked");
    Console.WriteLine($"Esc: picked node {(cancelled["node"] == null ? "null" : cancelled["node"].ToString())}");
    await Click(bx, by);
    Console.WriteLine($"status after a click following Esc: \"{await Status()}\"");

    // 4. pickMode false cancels
    await Send(new { type = "pickMode", on = true });
    await Send(new { type = "pickMode", on = false });
    await Click(bx, by);
    Console.WriteLine($"status after a click following pickMode false: \"{await Status()}\"");
    Console.WriteLine("DONE");
    return;
}

if (nav)
{
    string Pages(JsonNode t) => string.Join(", ", t["nodes"].AsArray()
        .Where(n => ((string)n["typeName"]).EndsWith("Page"))
        .Select(n => ((string)n["typeName"]).Split('.').Last() + ((bool)n["visible"] ? "" : " (hidden)")));
    await Receive("app");
    await Receive("screens");
    Console.WriteLine($"start: {Pages(await Receive("tree"))}");
    foreach (var step in args.Skip(2))
    {
        await Send(new { type = "openScreen", route = step });
        await Task.Delay(900);
        Console.WriteLine($"after openScreen({step}): {Pages(await Tree())}");
    }
    return;
}

if (focus)
{
    var app = await Receive("app");
    await Receive("screens");
    await Receive("tree");
    Console.WriteLine("READY");
    await Task.Delay(1500);
    if (args.Length > 2 && args[2] == "grant")
        Console.WriteLine($"AllowSetForegroundWindow({app["pid"]}): {AllowSetForegroundWindow((int)app["pid"])}");
    Console.WriteLine($"FOCUS {Environment.TickCount64}");
    await Send(new { type = "focus" });
    await Task.Delay(1500);
    return;
}

if (hidden)
{
    async Task<long> Shot(string file)
    {
        var clock = System.Diagnostics.Stopwatch.StartNew();
        await Send(new { type = "snapshot", request = 9 });
        var shot = await Receive("snapshot", "error");
        if ((string)shot["type"] != "snapshot")
            return -1;
        File.WriteAllBytes(Path.Combine(args[1], file), Convert.FromBase64String((string)shot["pngBase64"]));
        Console.WriteLine($"snapshot {file}: {shot["width"]}x{shot["height"]} in {clock.ElapsedMilliseconds} ms");
        return clock.ElapsedMilliseconds;
    }
    string Rect(JsonNode n) => string.Join(",", n["rect"].AsArray().Select(v => ((float)v).ToString("0")));

    await Receive("app");
    await Receive("screens");
    await Receive("tree");
    await Shot("normal.png");
    await Shot("normal2.png");

    Console.WriteLine("MINIMIZE");
    var ack = Path.Combine(args[1], "ack");
    for (var i = 0; i < 300 && !File.Exists(ack); i++)
        await Task.Delay(100);
    File.Delete(ack);
    await Task.Delay(1000);

    var clock2 = System.Diagnostics.Stopwatch.StartNew();
    await Send(new { type = "openScreen", route = "details" });
    var hTree = await Tree();
    Console.WriteLine($"openScreen + getTree while minimized: {clock2.ElapsedMilliseconds} ms");
    var hNodes = hTree["nodes"].AsArray();
    Console.WriteLine($"tree: {hNodes.Count} nodes, visible {hNodes.Count(n => (bool)n["visible"])}");
    foreach (var n in hNodes.Where(n => (bool)n["visible"] && (((string)n["typeName"]).Contains("Details") || n["text"] != null || ((string)n["typeName"]).EndsWith("SkiaSlider"))))
        Console.WriteLine($"   {((string)n["typeName"]).Split('.').Last()} \"{n["text"]}\" rect [{Rect(n)}]");
    var value = hNodes.First(n => ((string)n["text"] ?? "").StartsWith("Volume"));
    await Send(new { type = "setProperty", request = 10, node = (long)value["id"], name = "Text", value = "Volume: 77 (set while minimized)" });
    var hProps = await Props((long)value["id"]);
    Console.WriteLine($"getProps Text after setProperty: \"{Prop(hProps, "Text")}\"");
    var hAfter = (await Tree())["nodes"].AsArray().First(n => (long)n["id"] == (long)value["id"]);
    Console.WriteLine($"label after setProperty: \"{hAfter["text"]}\" rect [{Rect(hAfter)}] (was [{Rect(value)}])");
    await Shot("minimized.png");
    await Shot("minimized2.png");
    Console.WriteLine("DONE");
    return;
}

if (snap)
{
    await Receive("app");
    await Receive("screens");
    var shown = await Receive("tree");
    if (args.Length > 3)
    {
        await Send(new { type = "openScreen", route = args[3] });
        await Task.Delay(1500);
        shown = await Tree();
    }
    var texts = shown["nodes"].AsArray().Where(n => n["text"] != null).ToList();
    Console.WriteLine($"nodes {shown["nodes"].AsArray().Count}, with text {texts.Count}");
    foreach (var n in texts)
        Console.WriteLine($"   {((string)n["typeName"]).Split('.').Last()} \"{n["text"]}\"");
    var clock = System.Diagnostics.Stopwatch.StartNew();
    await Send(new { type = "snapshot", request = 7 });
    var shot = await Receive("snapshot", "error");
    if ((string)shot["type"] == "snapshot")
    {
        var png = Convert.FromBase64String((string)shot["pngBase64"]);
        File.WriteAllBytes(args[1], png);
        Console.WriteLine($"snapshot request {shot["request"]}: {shot["width"]}x{shot["height"]}, {png.Length} bytes PNG in {clock.ElapsedMilliseconds} ms");
    }
    return;
}

if (watch)
{
    var hello = await Receive("app");
    Console.WriteLine($"project: {hello["project"]}");
    await Receive("screens");
    var first = await Receive("tree");
    JsonNode Title(JsonNode t) => t["nodes"].AsArray().FirstOrDefault(n => ((string)n["field"] ?? "").EndsWith("._title"));
    var title = Title(first);
    Console.WriteLine($"title node {title?["id"]} FontSize={Prop(await Props((long)title["id"]), "FontSize")}");
    Console.WriteLine("READY");
    var end = DateTime.UtcNow.AddSeconds(int.Parse(args[1]));
    while (DateTime.UtcNow < end)
    {
        await Task.Delay(100);
        await Send(new { type = "getTree" });
        var now = await Receive("tree");
        var t = Title(now);
        if (t != null && (long)t["id"] != (long)title["id"])
        {
            Console.WriteLine($"CHANGED {Environment.TickCount64} title node {t["id"]} FontSize={Prop(await Props((long)t["id"]), "FontSize")}");
            return;
        }
    }
    Console.WriteLine("NO CHANGE");
    return;
}

if (dump)
{
    await Receive("app");
    await Receive("screens");
    await Receive("tree");
    if (route != "root")
        await Send(new { type = "openScreen", route });
    await Task.Delay(1500); // settle: page transition done, images loaded
    var settled = await Tree();
    File.WriteAllText(args[1], settled.ToJsonString(new JsonSerializerOptions { WriteIndented = true }) + "\n");
    Summary(settled, settled.ToJsonString().Length);
    return;
}

await Receive("app");
var screens = await Receive("screens");
Console.WriteLine($"screens {screens["screens"].AsArray().Count}");
var tree = await Receive("tree");
Summary(tree, tree.ToJsonString().Length);
Console.WriteLine("node keys: " + string.Join(",", tree["nodes"][0].AsObject().Select(p => p.Key)));
var nodes = tree["nodes"].AsArray();
Console.WriteLine($"tags non-null: {nodes.Count(n => n["tag"] != null)}, empty-string tags: {nodes.Count(n => (string)n["tag"] == "")}");
float W = (float)tree["width"], H = (float)tree["height"];
var offCanvas = nodes.Where(n => (float)n["rect"][2] > 0 && ((float)n["rect"][1] >= H || (float)n["rect"][1] + (float)n["rect"][3] <= 0)).ToList();
Console.WriteLine($"nodes with rect off the canvas: {offCanvas.Count}, of them visible=true: {offCanvas.Count(n => (bool)n["visible"])}");

// an on-canvas label near the vertical centre, so it stays on the canvas after one wheel notch
var label = nodes.Where(n => ((string)n["typeName"]).EndsWith("SkiaLabel") && (bool)n["visible"]
                             && (float)n["rect"][2] > 20 && (float)n["rect"][3] > 10
                             && (float)n["rect"][0] >= 0 && (float)n["rect"][0] + (float)n["rect"][2] <= W
                             && (float)n["rect"][1] > H * 0.35 && (float)n["rect"][1] + (float)n["rect"][3] < H * 0.8)
    .OrderBy(n => Math.Abs((float)n["rect"][1] - H / 2)).First();
var id = (long)label["id"];
var props = await Props(id);
Console.WriteLine($"props for {id}: {props["props"]?.AsArray().Count}, Text=\"{Prop(props, "Text")}\" rect=[{string.Join(",", label["rect"].AsArray())}]");

var r = label["rect"];
float cx = (float)r[0] + (float)r[2] / 2, cy = (float)r[1] + (float)r[3] / 2;
await Send(new { type = "pick", request = 1, x = cx, y = cy });
var picked = await Receive("picked");
Console.WriteLine($"pick -> {picked["node"]} {((long?)picked["node"] == id ? "MATCH" : "DIFFERENT")}");

// frame counting windows for a probe: idle, highlighted, idle again
await Task.Delay(1000);
Phase("idle");
await Task.Delay(3000);
await Send(new { type = "highlight", node = id });
await Task.Delay(500);
Phase("highlight");
await Task.Delay(3000);
Phase("highlight-end");
await Capture(1);

if (sync != null)
{
    File.WriteAllText(Path.Combine(sync, "scroll.flag"), "");
    await Task.Delay(1500);
    var moved = (await Tree())["nodes"].AsArray().First(n => (long)n["id"] == id);
    Console.WriteLine($"after scroll: rect=[{string.Join(",", moved["rect"].AsArray())}] visible={moved["visible"]}");
    await Capture(2);
}

await Send(new { type = "highlight", node = (long?)null });
await Task.Delay(500);
await Capture(3);

await Send(new { type = "setProperty", request = 2, node = id, name = "Text", value = "DESIGNER-OK" });
await Send(new { type = "setProperty", request = 4, node = id, name = "TextColor", value = "#FFFF8000" });
await Send(new { type = "setProperty", request = 5, node = id, name = "Margin", value = "1,2,3,4" });
await Send(new { type = "setProperty", request = 6, node = id, name = "HorizontalOptions", value = "Center" });
await Task.Delay(300);
var after = await Props(id);
Console.WriteLine($"after set: Text={Prop(after, "Text")} TextColor={Prop(after, "TextColor")} Margin={Prop(after, "Margin")} HorizontalOptions={Prop(after, "HorizontalOptions")}");
await Send(new { type = "setProperty", request = 3, node = id, name = "FontSize", value = "abc" });
await Receive("error");
await Send(new { type = "getProps", node = 999999 });
await Receive("error");

await Send(new { type = "openScreen", route });
await Task.Delay(2000);
var tree3 = await Tree();
Summary(tree3, tree3.ToJsonString().Length);
Console.WriteLine("page types now: " + string.Join(", ", tree3["nodes"].AsArray().Select(n => (string)n["typeName"]).Where(t => t.Contains("Page")).Distinct()));
Console.WriteLine("DONE");

[System.Runtime.InteropServices.DllImport("user32.dll")]
static extern bool AllowSetForegroundWindow(int processId);
