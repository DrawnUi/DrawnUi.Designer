using System.Windows;
using DesignerSample.Pages;
using DrawnUi.Draw;
using DrawnUi.Views;
using Color = DrawnUi.Color;

namespace DesignerSample;

/// <summary>
/// Hosts a SkiaShell with two routes. Hot reload: the shell re-creates the visible page from its route
/// factory, and the root page from <see cref="SkiaShell.RootBuilder"/>, after every C# edit.
/// </summary>
public partial class MainWindow : Window
{
    private SkiaShell _shell;

    /// <summary>Creates the window and mounts the shell.</summary>
    public MainWindow()
    {
        InitializeComponent();

        Drawn.Canvas.BackgroundColor = Color.Parse("#212529");
        Drawn.Content = new SkiaShell
        {
            Routes = new Dictionary<string, Func<ShellArguments, SkiaControl>>
            {
                ["home"] = _ => new HomePage(Navigate),
                ["details"] = _ => new DetailsPage(),
            },
            Titles = new Dictionary<string, string>
            {
                ["home"] = "Home",
                ["details"] = "Details",
            },
            RootBuilder = () => new HomePage(Navigate),
        }.Assign(out _shell);
    }

    private void Navigate(string route) => _ = _shell.GoToAsync(route);
}
