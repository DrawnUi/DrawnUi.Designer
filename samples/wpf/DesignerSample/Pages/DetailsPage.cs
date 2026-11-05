using DrawnUi.Draw;
using DrawnUi.Views;
using Color = DrawnUi.Color;

namespace DesignerSample.Pages;

/// <summary>Second screen: a few labels and a slider whose value is shown above it.</summary>
public class DetailsPage : SkiaLayer
{
    private SkiaLabel _value;

    /// <summary>Builds the page.</summary>
    public DetailsPage()
    {
        HorizontalOptions = LayoutOptions.Fill;
        VerticalOptions = LayoutOptions.Fill;

        Children = new List<SkiaControl>
        {
            new SkiaStack
            {
                Spacing = 12,
                Padding = new Thickness(24),
                MaximumWidthRequest = 640,
                HorizontalOptions = LayoutOptions.Center,
                Children = new List<SkiaControl>
                {
                    new SkiaLabel("Details") { FontSize = 24, TextColor = Colors.White },
                    new SkiaLabel("A second route of the shell, opened from the Home page.")
                    {
                        FontSize = 14, TextColor = Color.Parse("#ADB5BD"), HorizontalOptions = LayoutOptions.Fill,
                    },
                    new SkiaLabel("Volume: 40") { FontSize = 16, TextColor = Colors.White }.Assign(out _value),
                    new SkiaSlider
                    {
                        Min = 0,
                        Max = 100,
                        End = 40,
                        Step = 1,
                        AccessibilityLabel = "Volume",
                        HorizontalOptions = LayoutOptions.Fill,
                    }.ObserveSelf((me, prop) =>
                    {
                        if (prop == nameof(SkiaSlider.End))
                            _value.Text = $"Volume: {me.End:0}";
                    }),
                },
            },
        };
    }
}
