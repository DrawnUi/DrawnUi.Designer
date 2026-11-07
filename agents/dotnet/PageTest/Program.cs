// Headless check of Attach.AttachPage: a DrawnUi.Net canvas plays the page's canvas, a list plays the page's channel.
using System.Collections.Concurrent;
using System.Text.Json.Nodes;
using DrawnUi.Designer.Agent;
using DrawnUi.Draw;
using DrawnUi.Testing;

var failures = 0;
void Check(bool ok, string what)
{
    Console.WriteLine($"{(ok ? "ok  " : "FAIL")} {what}");
    if (!ok)
        failures++;
}

using var host = new HeadlessCanvasHost(400, 300, 2f);
var label = new SkiaLabel { Text = "Hello page", FontSize = 20 };
// the page's own wrapper (the Fiddle's _root) around the snippet's column
var column = new SkiaLayout { Type = LayoutType.Column, Children = new List<SkiaControl> { label } };
var wrapper = new SkiaLayout { HorizontalOptions = DrawnUi.Views.LayoutOptions.Fill, VerticalOptions = DrawnUi.Views.LayoutOptions.Fill, Children = new List<SkiaControl> { column } };
host.Canvas.Content = wrapper;
host.AdvanceFrames(3, 16);

var outbox = new BlockingCollection<string>();
var handle = Attach.AttachPage(host.Canvas, text =>
{
    outbox.Add(text);
    return Task.CompletedTask;
});

JsonNode Next(string type)
{
    while (outbox.TryTake(out var text, 10000))
    {
        var node = JsonNode.Parse(text);
        if ((string)(node["type"] ?? node["role"]) == type)
            return node;
    }
    throw new TimeoutException("no " + type);
}

// a message sent before the hello went out waits for it
var early = handle("{\"type\":\"getTree\"}");

var hello = Next("app");
Check((string)hello["engine"] == "dotnet-blazor", $"hello engine {hello["engine"]}");
Check((string)hello["platform"] == "browser", $"hello platform {hello["platform"]}");
Check((int)hello["pid"] == 0, $"hello pid {hello["pid"]}");
Check(hello["project"] == null && hello["agent"] == null, "hello has no project and no agent path");

var tree = Next("tree");
var nodes = tree["nodes"].AsArray();
var labelNode = nodes.FirstOrDefault(n => (string)n["text"] == "Hello page");
Check(labelNode != null, $"first tree: {nodes.Count} nodes, the label among them");
Check((float)tree["width"] == 400 && (float)tree["height"] == 300, $"canvas pixel size {tree["width"]}x{tree["height"]} (a 400x300 px surface, 200x150 pt at scale 2)");

await early;
Check(Next("tree") != null, "the early getTree was answered after the hello");

// A control the app adds by itself (a dialog after the first tree): a new tree follows without a getTree,
// with DrawnUi that raises Super.ChildrenChanged.
if (typeof(Super).GetEvent("ChildrenChanged") != null)
{
    var added = new SkiaLabel { Text = "Added by the app" };
    column.AddSubView(added);
    host.AdvanceFrames(2, 16);
    var pushed = Next("tree");
    Check(pushed["nodes"].AsArray().Any(n => (string)n["text"] == "Added by the app"), "a control the app adds is sent in a new tree");
    column.RemoveSubView(added);
    host.AdvanceFrames(2, 16);
    Check(!Next("tree")["nodes"].AsArray().Any(n => (string)n["text"] == "Added by the app"), "and its removal too");
}
else
    Console.WriteLine("skip a control the app adds: this DrawnUi has no Super.ChildrenChanged");

// Actions the app declares: DrawnUi's DesignerActions (when this DrawnUi has it) and Hot Preview [Preview] commands.
if (typeof(Super).Assembly.GetType("DrawnUi.Draw.DesignerActions") is { } designerActions)
{
    var ran = "";
    var add = designerActions.GetMethod("Add")!;
    var added = (IDisposable)add.Invoke(null, new object[] { "Levels/Level 3", (Action)(() => ran = "level 3") })!;
    var list = Next("actions")["actions"].AsArray().Select(a => (string)a["path"]).ToList();
    Check(list.SequenceEqual(new[] { "Levels/Level 3", "Previews/Game lost" }), "actions: the declared one, then the [Preview] command: " + string.Join(", ", list));
    _ = handle("{\"type\":\"runAction\",\"path\":\"Levels/Level 3\"}");
    for (var i = 0; i < 50 && ran == ""; i++)
        await Task.Delay(20);
    Check(ran == "level 3", "runAction runs a DesignerActions action");
    _ = handle("{\"type\":\"runAction\",\"path\":\"Previews/Game lost\"}");
    for (var i = 0; i < 50 && !Previews.Lost; i++)
        await Task.Delay(20);
    Check(Previews.Lost, "runAction runs a [Preview] command");
    added.Dispose();
    Check(!Next("actions")["actions"].AsArray().Any(a => (string)a["path"] == "Levels/Level 3"), "a removed action leaves the list");
}
else
    Console.WriteLine("skip actions: this DrawnUi has no DesignerActions");

var id = (long)labelNode["id"];
await handle($"{{\"type\":\"getProps\",\"node\":{id}}}");
var props = Next("props")["props"].AsArray();
Check(props.Any(p => (string)p["name"] == "Text" && (string)p["value"] == "Hello page"), $"getProps: {props.Count} props, Text read");
// only what can be set: IsClippedToBounds (a bindable property with a setter) is listed, the computed
// WillClipBounds (a getter only) is not, nor is any read-only one
Check(props.Any(p => (string)p["name"] == "IsClippedToBounds") && !props.Any(p => (string)p["name"] == "WillClipBounds"),
    "getProps lists only settable properties: IsClippedToBounds yes, WillClipBounds no");

// two messages without waiting: handled one after the other, in order
var set = handle($"{{\"type\":\"setProperty\",\"request\":2,\"node\":{id},\"name\":\"Text\",\"value\":\"Changed\"}}");
var read = handle($"{{\"type\":\"getProps\",\"node\":{id}}}");
await Task.WhenAll(set, read);
var after = Next("props")["props"].AsArray();
Check(after.Any(p => (string)p["name"] == "Text" && (string)p["value"] == "Changed") && label.Text == "Changed", "setProperty then getProps, in order: Text changed");

await handle("{\"type\":\"snapshot\",\"request\":3}");
var shot = Next("snapshot");
Check((int)shot["width"] == 400 && ((string)shot["pngBase64"]).Length > 100, $"snapshot {shot["width"]}x{shot["height"]}");

await handle($"{{\"type\":\"pick\",\"request\":4,\"x\":20,\"y\":20}}");
Check(Next("picked") != null, "pick answered");

JsonNode Next2Tree() { _ = handle("{\"type\":\"getTree\"}"); return Next("tree"); }

// the selection box takes the designer's color; a bad value falls back to magenta
var boxRect = Next2Tree()["nodes"].AsArray().First(n => (long)n["id"] == id)["rect"];
SkiaSharp.SKColor BoxPixel(string colorJson)
{
    handle($"{{\"type\":\"highlight\",\"node\":{id}{colorJson}}}").GetAwaiter().GetResult();
    host.RenderFrame(16);
    var file = Path.Combine(Path.GetTempPath(), $"pagetest-box-{Environment.ProcessId}.png");
    host.SavePng(file);
    using var bitmap = SkiaSharp.SKBitmap.Decode(file);
    File.Delete(file);
    // the right edge: the box is a 2 px stroke just outside the control (the label sits at the left edge)
    return bitmap.GetPixel((int)((float)boxRect[0] + (float)boxRect[2]), (int)((float)boxRect[1] + (float)boxRect[3] / 2));
}
var green = BoxPixel(",\"color\":\"#FF00FF00\"");
var blue = BoxPixel(",\"color\":\"#0000FF\"");
var fallback = BoxPixel(",\"color\":\"nonsense\"");
var none = BoxPixel("");
Check(green == new SkiaSharp.SKColor(0, 255, 0) && blue == new SkiaSharp.SKColor(0, 0, 255) && fallback == new SkiaSharp.SKColor(255, 0, 255) && none == new SkiaSharp.SKColor(255, 0, 255),
    $"box color: #FF00FF00 -> {green}, #0000FF -> {blue}, bad -> {fallback}, none -> {none}");
await handle("{\"type\":\"highlight\",\"node\":null}");

// the controls the app can create; insert, remove; place mode with a tap
await handle("{\"type\":\"getControls\"}");
var kinds = Next("controls")["controls"].AsArray();
Check(kinds.Count > 10 && kinds.Any(k => (string)k["typeName"] == "DrawnUi.Draw.SkiaLabel" && (string)k["group"] == "Text"), $"getControls: {kinds.Count} controls, SkiaLabel in Text");
var specialized = new[] { "ContentWithBackdrop", "InfiniteLayout", "GridLayout", "AnimatedFramesRenderer" };
Check(!kinds.Any(k => specialized.Contains(((string)k["typeName"]).Split('.').Last())), "getControls: no specialized controls the page does not use");
var layoutNodes = Next2Tree()["nodes"].AsArray().Where(n => ((string)n["typeName"]).EndsWith("SkiaLayout")).ToList();
var wrapperId = (long)layoutNodes.First(n => n["parent"] == null)["id"];
var columnId = (long)layoutNodes.First(n => (long?)n["parent"] == wrapperId)["id"];
Check((bool?)layoutNodes.First(n => (long)n["id"] == wrapperId)["pageRoot"] == true && layoutNodes.Where(n => (long)n["id"] != wrapperId).All(n => n["pageRoot"] == null),
    "page mode: the wrapper node has pageRoot true, no other node carries pageRoot");
await handle($"{{\"type\":\"insert\",\"request\":7,\"parent\":{columnId},\"index\":0,\"typeName\":\"DrawnUi.Draw.SkiaLabel\"}}");
var ins = Next("inserted");
var insTree = Next("tree")["nodes"].AsArray();
Check(ins["node"] != null && (int)ins["index"] == 0 && insTree.Any(n => (long?)n["id"] == (long?)ins["node"] && (string)n["text"] == "Label"), $"insert: node {ins["node"]} at {ins["index"]}, shown as \"Label\"");
await handle($"{{\"type\":\"remove\",\"request\":8,\"node\":{ins["node"]}}}");
Check(!Next("tree")["nodes"].AsArray().Any(n => (long?)n["id"] == (long?)ins["node"]), "remove: gone from the tree");
await handle($"{{\"type\":\"insert\",\"request\":9,\"parent\":{id},\"typeName\":\"DrawnUi.Draw.SkiaLabel\"}}");
var refused = Next("error");
Check(Next("inserted")["node"] == null, $"insert into a label refused: {refused["message"]}");
await handle("{\"type\":\"placeMode\",\"typeName\":\"DrawnUi.Draw.SkiaShape\"}");
var robot = new GestureRobot(host);
robot.Tap(30, 22); // on the label's lower half: into the column, after it
host.AdvanceFrames(3, 16);
var placed = Next("inserted");
var placedTree = placed["node"] == null ? new JsonArray() : Next("tree")["nodes"].AsArray();
Check(placed["node"] != null && (long)placed["parent"] == columnId && placedTree.Any(n => (long?)n["id"] == (long?)placed["node"] && ((string)n["typeName"]).EndsWith("SkiaShape")),
    $"place mode: a tap on the label put a SkiaShape into the column at {placed["index"]}");

// index counts among the parent's children in tree order: 1 = between the label and the placed shape
await handle($"{{\"type\":\"insert\",\"request\":10,\"parent\":{columnId},\"index\":1,\"typeName\":\"DrawnUi.Draw.SkiaButton\"}}");
Next("inserted");
var order = Next("tree")["nodes"].AsArray().Where(n => (long?)n["parent"] == columnId).Select(n => ((string)n["typeName"]).Split('.').Last()).ToList();
Check(order.SequenceEqual(new[] { "SkiaLabel", "SkiaButton", "SkiaShape" }), "insert at index 1: " + string.Join(", ", order));

// a pasted control: its properties come after the defaults; one it lacks or a bad value is skipped with a note
await handle($"{{\"type\":\"insert\",\"request\":11,\"parent\":{columnId},\"typeName\":\"DrawnUi.Draw.SkiaLabel\",\"props\":[[\"Text\",\"Pasted\"],[\"FontSize\",\"30\"],[\"NoSuchProperty\",\"1\"],[\"FontSize\",\"big\"]]}}");
var pasted = Next("inserted");
var note = Next("error");
var pastedNode = Next("tree")["nodes"].AsArray().First(n => (long?)n["id"] == (long?)pasted["node"]);
await handle($"{{\"type\":\"getProps\",\"node\":{pasted["node"]}}}");
var pastedFont = (string)Next("props")["props"].AsArray().First(p => (string)p["name"] == "FontSize")["value"];
Check((string)pastedNode["text"] == "Pasted" && pastedFont == "30" && ((string)note["message"]).Contains("NoSuchProperty") && ((string)note["message"]).Contains("FontSize"),
    $"insert with props: text {pastedNode["text"]}, FontSize {pastedFont}; note: {note["message"]}");

// the page's wrapper is not the snippet's code: never an insert target; a click on its empty area is the snippet's
// root's (at the end of a short column); with a leaf on top it is refused and place mode stays on
await handle($"{{\"type\":\"insert\",\"request\":15,\"parent\":{wrapperId},\"typeName\":\"DrawnUi.Draw.SkiaLabel\"}}");
var wrapperNote = Next("error");
Check(Next("inserted")["node"] == null && ((string)wrapperNote["message"]).Contains("snippet"), $"insert into the page wrapper refused: {wrapperNote["message"]}");
column.ClearChildren();
column.AddSubView(new SkiaLabel { Text = "Top" }); // a short column: the lower canvas is the wrapper's alone
host.AdvanceFrames(2, 16);
await handle("{\"type\":\"placeMode\",\"typeName\":\"DrawnUi.Draw.SkiaShape\"}");
robot.Tap(100, 140); // below the column: only the wrapper is there
host.AdvanceFrames(3, 16);
var wrapperArea = Next("inserted");
Check((long?)wrapperArea["parent"] == columnId && (int?)wrapperArea["index"] == 1 && column.Views.Count == 2,
    $"place mode: a click on the wrapper's empty area goes into the snippet's column, last: {wrapperArea}");
Next("tree");
column.RemoveSubView(column.Views[1]);
host.AdvanceFrames(2, 16);
while (outbox.TryTake(out _)) { }
await handle("{\"type\":\"placeMode\",\"typeName\":null}");
var leaf = new SkiaLabel { Text = "Leaf snippet" };
wrapper.RemoveSubView(column);
wrapper.AddSubView(leaf);
host.AdvanceFrames(2, 16);
await handle("{\"type\":\"getTree\"}");
Next("tree");
await handle("{\"type\":\"placeMode\",\"typeName\":\"DrawnUi.Draw.SkiaShape\"}");
robot.Tap(100, 140);
host.AdvanceFrames(3, 16);
var leafNote = Next("error");
robot.Tap(100, 140); // still in place mode: refused again
host.AdvanceFrames(3, 16);
var leafNote2 = Next("error");
Check((string)leafNote["message"] == "Click inside the snippet's layout" && (string)leafNote2["message"] == (string)leafNote["message"] && leaf.Views.Count == 0,
    $"place mode with a leaf snippet: refused twice, still in place mode: {leafNote["message"]}");
await handle("{\"type\":\"placeMode\",\"typeName\":null}");
wrapper.RemoveSubView(leaf);
wrapper.AddSubView(column);
host.AdvanceFrames(2, 16);

// move: the same object (same id) goes to another place, within its parent or into another; refusals answer
// moved with node null after an error note
var mA = new SkiaLabel { Text = "A" };
var mB = new SkiaLabel { Text = "B" };
var mC = new SkiaLabel { Text = "C" };
var mBox = new SkiaShape { Padding = new DrawnUi.Views.Thickness(2), Children = new List<SkiaControl> { mC } };
var mList = new SkiaLayout { Type = LayoutType.Row, ItemsSource = new List<string> { "x", "y" }, ItemTemplate = new DataTemplate(() => new SkiaLabel { Text = "cell" }) };
var mButton = new SkiaButton { Text = "Btn" };
column.ClearChildren();
foreach (var c in new SkiaControl[] { mA, mB, mBox, mList, mButton })
    column.AddSubView(c);
host.AdvanceFrames(3, 16);
var mNodes = Next2Tree()["nodes"].AsArray();
long IdOfText(string text) => (long)mNodes.First(n => (string)n["text"] == text)["id"];
var aId = IdOfText("A");
var bId = IdOfText("B");
var boxId = (long)mNodes.First(n => (long?)n["parent"] == columnId && ((string)n["typeName"]).EndsWith("SkiaShape"))["id"];
var listId = (long)mNodes.First(n => (long?)n["parent"] == columnId && ((string)n["typeName"]).EndsWith("SkiaLayout"))["id"];
var buttonId = (long)mNodes.First(n => (long?)n["parent"] == columnId && ((string)n["typeName"]).EndsWith("SkiaButton"))["id"];
var buttonPart = mNodes.FirstOrDefault(n => (long?)n["parent"] == buttonId);
var cell = mNodes.FirstOrDefault(n => (long?)n["parent"] == listId);
Check((bool?)mNodes.First(n => (long)n["id"] == listId)["templated"] == true
      && mNodes.Where(n => (long)n["id"] != listId).All(n => (bool?)n["templated"] != true),
    "templated: true on the items layout, on no other node");
List<string> Names(SkiaControl parent) => parent.Views.Select(v => v is SkiaLabel l ? l.Text : v.GetType().Name).ToList();

await handle($"{{\"type\":\"move\",\"request\":20,\"node\":{aId},\"parent\":{columnId},\"index\":1}}");
var mv1 = Next("moved");
var mv1Tree = Next("tree")["nodes"].AsArray();
Check((long?)mv1["node"] == aId && (int)mv1["index"] == 1 && Names(column).Take(3).SequenceEqual(new[] { "B", "A", "SkiaShape" })
      && mv1Tree.Any(n => (long)n["id"] == aId && (long?)n["parent"] == columnId),
    $"move within the parent: A to index 1 -> {string.Join(", ", Names(column))}, same id {mv1["node"]}");
await handle($"{{\"type\":\"move\",\"request\":21,\"node\":{aId},\"parent\":{boxId},\"index\":0}}");
var mv2 = Next("moved");
var mv2Tree = Next("tree")["nodes"].AsArray();
Check((long?)mv2["node"] == aId && (long?)mv2["parent"] == boxId && Names(mBox).SequenceEqual(new[] { "A", "C" }) && !column.Views.Contains(mA)
      && mA.Parent == mBox && mv2Tree.Any(n => (long)n["id"] == aId && (long?)n["parent"] == boxId),
    $"move across parents: A into the shape at 0 -> {string.Join(", ", Names(mBox))}, same object, same id");

async Task Refused(string what, long node, long parent)
{
    await handle($"{{\"type\":\"move\",\"request\":22,\"node\":{node},\"parent\":{parent},\"index\":0}}");
    var note = Next("error");
    var moved = Next("moved");
    Check(moved["node"] == null && (long?)moved["parent"] == parent, $"move refused, {what}: {note["message"]}");
}
await Refused("parent not a container", aId, bId);
await Refused("parent templated", bId, listId);
await Refused("into itself", boxId, boxId);
await Refused("into its own child", columnId, boxId);
await Refused("into the page wrapper", bId, wrapperId);
Check(buttonPart != null && cell != null, $"move test has a library part ({buttonPart?["typeName"]}) and a template cell ({cell?["typeName"]})");
if (buttonPart != null)
    await Refused("a library part", (long)buttonPart["id"], boxId);
if (cell != null)
    await Refused("a template cell", (long)cell["id"], boxId);
Check(Names(column).SequenceEqual(new[] { "B", "SkiaShape", "SkiaLayout", "SkiaButton" }) && Names(mBox).SequenceEqual(new[] { "A", "C" }), "refused moves changed nothing");
// every cell of a templated layout is the same template: nothing goes into, out of or is taken from any control
// inside a cell, at any depth
var deepList = new SkiaLayout
{
    Type = LayoutType.Column, ItemsSource = new List<string> { "p", "q" },
    ItemTemplate = new DataTemplate(() => new SkiaLayout { Type = LayoutType.Column, Children = new List<SkiaControl> { new SkiaLabel { Text = "deep" } } }),
};
column.AddSubView(deepList);
host.AdvanceFrames(3, 16);
var dNodes = Next2Tree()["nodes"].AsArray();
var dCell = dNodes.First(n => n["templateIndex"] != null && dNodes.Any(m => (long?)m["parent"] == (long)n["id"] && (string)m["text"] == "deep"));
var dCellId = (long)dCell["id"];
var deepId = (long)dNodes.First(n => (long?)n["parent"] == dCellId)["id"];
JsonNode Within(string type, int ms)
{
    var until = DateTime.UtcNow.AddMilliseconds(ms);
    while (DateTime.UtcNow < until && outbox.TryTake(out var text, 100))
    {
        var node = JsonNode.Parse(text);
        if ((string)(node["type"] ?? node["role"]) == type)
            return node;
    }
    return null;
}
async Task CellRefused(string what, string msg)
{
    await handle(msg);
    var note = Within("error", 2000);
    Check(note != null && ((string)note["message"]).Contains("template"), $"inside a cell, {what}: {note?["message"]}");
}
await CellRefused("insert into the cell", $"{{\"type\":\"insert\",\"request\":30,\"parent\":{dCellId},\"typeName\":\"DrawnUi.Draw.SkiaLabel\"}}");
await CellRefused("move into the cell", $"{{\"type\":\"move\",\"request\":31,\"node\":{bId},\"parent\":{dCellId},\"index\":0}}");
await CellRefused("move out of the cell", $"{{\"type\":\"move\",\"request\":32,\"node\":{deepId},\"parent\":{boxId},\"index\":0}}");
await CellRefused("remove", $"{{\"type\":\"remove\",\"request\":33,\"node\":{deepId}}}");
host.AdvanceFrames(2, 16);
var dAfter = Next2Tree()["nodes"].AsArray();
Check(dAfter.Any(n => (long)n["id"] == deepId && (long?)n["parent"] == dCellId) && Names(mBox).SequenceEqual(new[] { "A", "C" }), "inside a cell: nothing changed");
column.RemoveSubView(deepList);
host.AdvanceFrames(2, 16);
while (outbox.TryTake(out _)) { }
// where a placed control lands (placeAt = a place-mode click there): into the control under the point when it holds
// children, at the index the point is at; else next to it, before or after by the half the point is in; nothing
// inside a template's cell
SkiaLabel L(string text) => new SkiaLabel { Text = text, WidthRequest = 30, HeightRequest = 14 };
var pA = L("pA");
var ps1 = L("ps1");
var pr1 = L("pr1");
var pStack = new SkiaLayout { Type = LayoutType.Column, Spacing = 10, WidthRequest = 150, Children = new List<SkiaControl> { ps1, L("ps2") } };
var pRow = new SkiaLayout { Type = LayoutType.Row, Spacing = 4, Children = new List<SkiaControl> { pr1, L("pr2") } };
var pLayer = new SkiaLayout { WidthRequest = 150, HeightRequest = 24, Children = new List<SkiaControl> { L("pin") } };
var pList = new SkiaLayout { Type = LayoutType.Column, ItemsSource = new List<string> { "x" }, ItemTemplate = new DataTemplate(() => new SkiaLayout { WidthRequest = 60, HeightRequest = 14 }) };
var pRoot = new SkiaLayout { Type = LayoutType.Column, Padding = new DrawnUi.Views.Thickness(4), Spacing = 2, Children = new List<SkiaControl> { pA, pStack, pRow, pLayer, pList } };
wrapper.RemoveSubView(column);
wrapper.AddSubView(pRoot);
host.AdvanceFrames(3, 16);
var pNodes = Next2Tree()["nodes"].AsArray();
long PId(object c) => (long)pNodes.First(n => (string)n["text"] == (c as SkiaLabel)?.Text || false)["id"];
float[] PRect(long nodeId) => pNodes.First(n => (long)n["id"] == nodeId)["rect"].AsArray().Select(v => (float)v).ToArray();
long ParentOf(long nodeId) => (long)pNodes.First(n => (long)n["id"] == nodeId)["parent"];
var pRootId = ParentOf(PId(pA));
var pStackId = ParentOf(PId(ps1));
var pRowId = ParentOf(PId(pr1));
var pLayerId = ParentOf(PId(pLayer.Views[0]));
async Task<(long? Parent, int? Index)> PlaceAt(float x, float y)
{
    await handle($"{{\"type\":\"placeAt\",\"typeName\":\"DrawnUi.Draw.SkiaLabel\",\"x\":{x.ToString(System.Globalization.CultureInfo.InvariantCulture)},\"y\":{y.ToString(System.Globalization.CultureInfo.InvariantCulture)}}}");
    var ins = Within("inserted", 3000);
    if (ins?["node"] == null)
        return (null, null);
    Within("tree", 3000);
    await handle($"{{\"type\":\"remove\",\"request\":40,\"node\":{ins["node"]}}}");
    Within("tree", 3000);
    while (outbox.TryTake(out _)) { }
    return ((long?)ins["parent"], (int?)ins["index"]);
}
var sr = PRect(pStackId);
var s1r = PRect(PId(ps1));
var at1 = await PlaceAt(sr[0] + sr[2] - 4, s1r[1] + s1r[3] + 10);
Check(at1 == (pStackId, 1), $"place: into a stack at the index the point is at -> {at1}");
var ar = PRect(PId(pA));
var at2 = await PlaceAt(ar[0] + 10, ar[1] + 4);
var at3 = await PlaceAt(ar[0] + 10, ar[1] + ar[3] - 4);
Check(at2 == (pRootId, 0) && at3 == (pRootId, 1), $"place: beside a label in a column, top half before, bottom half after -> {at2}, {at3}");
var r1r = PRect(PId(pr1));
var at4 = await PlaceAt(r1r[0] + 4, r1r[1] + 10);
var at5 = await PlaceAt(r1r[0] + r1r[2] - 4, r1r[1] + 10);
Check(at4 == (pRowId, 0) && at5 == (pRowId, 1), $"place: beside a label in a row, left half before, right half after -> {at4}, {at5}");
var lr = PRect(pLayerId);
var at6 = await PlaceAt(lr[0] + lr[2] - 6, lr[1] + lr[3] - 6);
Check(at6 == (pLayerId, 1), $"place: a layer's empty area, last -> {at6}");
var pCell = pNodes.First(n => n["templateIndex"] != null);
var cr = PRect((long)pCell["id"]);
var at7 = await PlaceAt(cr[0] + 10, cr[1] + 10);
Check(at7 == (null, null), $"place: nothing inside a template's cell -> {at7}");
var at8 = await PlaceAt(380, 280);
Check(at8 == (pRootId, 5), $"place: the page's own wrapper's empty area is the snippet's root, last -> {at8}");
// a control that fills the canvas (the page's wrapper here): its box shows along the canvas's edges
await handle($"{{\"type\":\"highlight\",\"node\":{wrapperId}}}");
host.RenderFrame(16);
var edgeFile = Path.Combine(Path.GetTempPath(), $"pagetest-edge-{Environment.ProcessId}.png");
host.SavePng(edgeFile);
using (var edgeBitmap = SkiaSharp.SKBitmap.Decode(edgeFile))
{
    File.Delete(edgeFile);
    var magenta = new SkiaSharp.SKColor(255, 0, 255);
    Check(edgeBitmap.GetPixel(0, 150) == magenta && edgeBitmap.GetPixel(200, 0) == magenta && edgeBitmap.GetPixel(399, 150) == magenta && edgeBitmap.GetPixel(200, 299) == magenta,
        $"a box around a control filling the canvas shows inside its edges: {edgeBitmap.GetPixel(0, 150)}");
}
await handle("{\"type\":\"highlight\",\"node\":null}");
wrapper.RemoveSubView(pRoot);
wrapper.AddSubView(column);
host.AdvanceFrames(2, 16);
while (outbox.TryTake(out _)) { }
// a parent whose Children is its own observed collection: the move goes through that collection
var obs = new SkiaLayout { Type = LayoutType.Row };
obs.Children.Add(new SkiaLabel { Text = "X" });
obs.Children.Add(new SkiaLabel { Text = "Y" });
mBox.AddSubView(obs);
host.AdvanceFrames(2, 16);
var yId = (long)Next2Tree()["nodes"].AsArray().First(n => (string)n["text"] == "Y")["id"];
await handle($"{{\"type\":\"move\",\"request\":23,\"node\":{yId},\"parent\":{IdOfObs()},\"index\":0}}");
long IdOfObs() => (long)Next2Tree()["nodes"].AsArray().First(n => ((string)n["typeName"]).EndsWith("SkiaLayout") && n["parent"] != null && (long?)n["parent"] != wrapperId && (long?)n["parent"] != columnId)["id"];
Next("moved");
Next("tree");
Check(obs.Children.Select(c => ((SkiaLabel)c).Text).SequenceEqual(new[] { "Y", "X" }) && Names(obs).SequenceEqual(new[] { "Y", "X" }), "move within an observed Children: the app's collection is reordered (Y, X)");

// deep copy: a chip (a shape sized by its padding and its label), copied, then pasted into the same column
var chip = new SkiaShape
{
    Padding = new DrawnUi.Views.Thickness(10, 4),
    BackgroundColor = Colors.Navy,
    Children = new List<SkiaControl> { new SkiaLabel { Text = "Chip", TextColor = Colors.Orange, FontSize = 14 } }
};
column.ClearChildren(); // room for both chips inside the 150 pt canvas
column.AddSubView(chip);
host.AdvanceFrames(2, 16);
await handle("{\"type\":\"getTree\"}");
var chipNodes = Next("tree")["nodes"].AsArray();
var chipId = (long)chipNodes.Last(n => ((string)n["typeName"]).EndsWith("SkiaShape") && (long?)n["parent"] == columnId)["id"];
await handle($"{{\"type\":\"copy\",\"request\":12,\"node\":{chipId}}}");
var copied = Next("copied");
var chipSpec = copied["spec"];
Console.WriteLine("     spec: " + chipSpec?.ToJsonString());
await handle($"{{\"type\":\"copy\",\"request\":13,\"node\":999999}}");
Check(Next("copied")["spec"] == null, "copy of a node that is gone: spec null");
var rootPairs = new JsonArray(chipSpec["props"].AsArray().Select(p => (JsonNode)new JsonArray((string)p["name"], (string)p["value"])).ToArray());
await handle(new JsonObject
{
    ["type"] = "insert", ["request"] = 14, ["parent"] = columnId, ["typeName"] = (string)chipSpec["typeName"],
    ["props"] = rootPairs, ["children"] = chipSpec["children"].DeepClone(),
}.ToJsonString());
var chipCopy = Next("inserted");
Next("tree");
host.AdvanceFrames(2, 16);
var copyShape = column.Views.OfType<SkiaShape>().LastOrDefault();
var copyLabel = copyShape?.Views.OfType<SkiaLabel>().FirstOrDefault();
Check(chipCopy["node"] != null && copyShape != chip && chip.MeasuredSize.Pixels.Width > 0 && copyShape?.MeasuredSize.Pixels == chip.MeasuredSize.Pixels
      && copyLabel?.Text == "Chip" && copyLabel.TextColor.ToSKColor() == Colors.Orange.ToSKColor(),
    $"copy + paste of a chip: size {copyShape?.MeasuredSize.Pixels} (original {chip.MeasuredSize.Pixels}), label {copyLabel?.Text} {copyLabel?.TextColor.ToSKColor()}");

// the app rebuilds its controls in place (DrawFiddle after each run): "rebuilt" sends the new tree
await handle($"{{\"type\":\"highlight\",\"node\":{id}}}");
var layout = column;
layout.ClearChildren();
layout.AddSubView(new SkiaLabel { Text = "Rebuilt label" });
host.AdvanceFrames(2, 16);
await handle("{\"type\":\"rebuilt\"}");
var rebuilt = Next("tree")["nodes"].AsArray();
Check(rebuilt.Any(n => (string)n["text"] == "Rebuilt label") && !rebuilt.Any(n => (string)n["text"] == "Changed"), "rebuilt: the new tree goes out without the old label");

// a new canvas: the agent moves to it
using var host2 = new HeadlessCanvasHost(200, 100, 1f);
host2.Canvas.Content = new SkiaLabel { Text = "Second canvas" };
host2.AdvanceFrames(3, 16);
var handle2 = Attach.AttachPage(host2.Canvas, text =>
{
    outbox.Add(text);
    return Task.CompletedTask;
});
Next("app");
var tree2 = Next("tree");
Check(tree2["nodes"].AsArray().Any(n => (string)n["text"] == "Second canvas"), "a second AttachPage moves the agent to the new canvas");

// a canvas the agent has seen and the app has left must be free to go (no static of the agent keeps it)
static bool Collected(WeakReference weak)
{
    // DrawnUi disposes on later frames and background work: let those run first
    using (var flush = new HeadlessCanvasHost(50, 50, 1f))
    {
        flush.Canvas.Content = new SkiaLabel { Text = "flush" };
        for (var f = 0; f < 20; f++)
        {
            flush.RenderFrame(16);
            Thread.Sleep(20);
        }
    }
    for (var i = 0; i < 2; i++)
    {
        GC.Collect();
        GC.WaitForPendingFinalizers();
    }
    GC.Collect();
    return !weak.IsAlive;
}

[System.Runtime.CompilerServices.MethodImpl(System.Runtime.CompilerServices.MethodImplOptions.NoInlining)]
static WeakReference ShowAndLeave(bool agentSeesIt, BlockingCollection<string> box)
{
    var leaving = new HeadlessCanvasHost(200, 100, 1f);
    var weak = new WeakReference(leaving.Canvas);
    leaving.Canvas.Content = new SkiaLayout
    {
        Type = LayoutType.Column,
        Children = new List<SkiaControl> { new SkiaLabel { Text = "leaving page" }, new SkiaScroll { HeightRequest = 40, Content = new SkiaLabel { Text = "in a scroll" } } },
    };
    leaving.AdvanceFrames(3, 16);
    if (agentSeesIt)
    {
        var receive = Attach.AttachPage(leaving.Canvas, text => { box.Add(text); return Task.CompletedTask; });
        receive("{\"type\":\"getTree\"}").GetAwaiter().GetResult();
        var node = JsonNode.Parse(box.Last(t => t.Contains("\"type\":\"tree\"")))["nodes"][1]["id"];
        receive($"{{\"type\":\"highlight\",\"node\":{node}}}").GetAwaiter().GetResult(); // the frame hooks go on this canvas
        receive("{\"type\":\"pickMode\",\"on\":true}").GetAwaiter().GetResult();         // and pick mode on its root
        leaving.AdvanceFrames(2, 16);
    }
    // the platform takes the canvas off the screen (HeadlessCanvasHost.Dispose does not: its canvas stays subscribed
    // to the static Super.NeedGlobalRefresh, which only DisconnectedHandler undoes)
    leaving.Canvas.DisconnectedHandler();
    leaving.Dispose();
    return weak;
}

var baseline = ShowAndLeave(false, outbox);
Check(Collected(baseline), "a left canvas the agent never saw is collected (baseline)");
if (Environment.GetEnvironmentVariable("PAGETEST_WAIT") == "1")
{
    // for a memory dump: dotnet-dump collect -p <pid>, then gcroot the canvas
    Console.WriteLine($"WAIT {Environment.ProcessId}");
    Thread.Sleep(90000);
}
// the last canvas the agent saw, left while the agent still points at it (no other canvas came): it must go too
var seen = ShowAndLeave(true, outbox);
Thread.Sleep(500);
Check(Collected(seen), "a left canvas the agent saw last (tree, highlight, pick mode on) is collected");

Console.WriteLine(failures == 0 ? "PASS" : $"{failures} FAILED");
return failures == 0 ? 0 : 1;

/// <summary>A Hot Preview command, as an app declares it (the attribute below stands in for Hot Preview's).</summary>
static class Previews
{
    public static bool Lost;

    [HotPreview.Preview("Game lost")]
    public static void State_Game_Lost() => Lost = true;
}

namespace HotPreview
{
    /// <summary>Hot Preview's attribute, matched by its full name.</summary>
    [AttributeUsage(AttributeTargets.Method)]
    sealed class PreviewAttribute(string displayName = null) : Attribute
    {
        public string DisplayName { get; } = displayName;
    }
}
