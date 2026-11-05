using DrawnUi.Draw;
using DrawnUi.Views;
using Color = DrawnUi.Color;

namespace DesignerSample.Pages;

/// <summary>Start screen: a title, two cards, a row of buttons and a status line.</summary>
public class HomePage : SkiaLayer
{
    private static readonly Color Muted = Color.Parse("#ADB5BD");

    private SkiaLabel _title;
    private SkiaLabel _status;
    private int _taps;

    /// <summary>Builds the page; <paramref name="navigate"/> opens a shell route.</summary>
    public HomePage(Action<string> navigate)
    {
        HorizontalOptions = LayoutOptions.Fill;
        VerticalOptions = LayoutOptions.Fill;

        Children = new List<SkiaControl>
        {
            new SkiaStack
            {
                Spacing = 16,
                Padding = new Thickness(24),
                MaximumWidthRequest = 640,
                HorizontalOptions = LayoutOptions.Center,
                Children = new List<SkiaControl>
                {
                    new SkiaLabel("DrawnUI Designer")
                    {
                        FontSize = 28,
                        FontWeight = 600,
                        TextColor = Colors.White,
                        HorizontalOptions = LayoutOptions.Fill,
                        HorizontalTextAlignment = DrawTextAlignment.Center,
                    }.Assign(out _title),

                    Card("Live tree", "Every control of this window is visible to the designer, with its properties."),
                    Card("Edit in place", "Change a property in the designer and this window shows it at once."),

                    new SkiaRow
                    {
                        Spacing = 12,
                        HorizontalOptions = LayoutOptions.Center,
                        Children = new List<SkiaControl>
                        {
                            new SkiaButton("Details") { BackgroundColor = Color.Parse("#0D6EFD") }
                                .OnTapped(me => navigate("details")),
                            new SkiaButton("Count") { BackgroundColor = Color.Parse("#198754") }
                                .OnTapped(me => _status.Text = $"Tapped {++_taps} times"),
                            new SkiaButton("Reset") { BackgroundColor = Color.Parse("#6C757D") }
                                .OnTapped(me =>
                                {
                                    _taps = 0;
                                    _status.Text = "Ready";
                                }),
                        },
                    },

                    new SkiaLabel("Ready")
                    {
                        FontSize = 15,
                        TextColor = Color.Parse("#FFC107"),
                        Margin = new Thickness(0, 8, 0, 0),
                        HorizontalOptions = LayoutOptions.Center,
                    }.Assign(out _status),
                },
            },
        };
    }

    /// <summary>A rounded panel with a heading and a line of text; static, so cached as one image.</summary>
    private static SkiaShape Card(string heading, string text)
    {
        return new SkiaShape
        {
            CornerRadius = 12,
            BackgroundColor = Color.Parse("#343A40"),
            Padding = new Thickness(16),
            HorizontalOptions = LayoutOptions.Fill,
            UseCache = SkiaCacheType.Image,
            Children = new List<SkiaControl>
            {
                new SkiaStack
                {
                    Spacing = 6,
                    Children = new List<SkiaControl>
                    {
                        new SkiaLabel(heading) { FontSize = 18, TextColor = Colors.White },
                        new SkiaLabel(text) { FontSize = 13, TextColor = Muted, HorizontalOptions = LayoutOptions.Fill },
                    },
                },
            },
        };
    }
}
