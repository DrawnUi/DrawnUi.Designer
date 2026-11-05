using System.Windows;
using DrawnUi.Draw;
using Style = DrawnUi.Draw.Style;
using Setter = DrawnUi.Draw.Setter;

namespace DesignerSample;

/// <summary>Starts DrawnUI with the app fonts; labels and buttons default to OpenSans.</summary>
public partial class App : Application
{
    /// <inheritdoc/>
    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);

        Super.UseDrawnUi()
            .ConfigureFonts(fonts => fonts
                .AddFont("fonts/OpenSans-Regular.ttf", "FontText")
                .AddFont("fonts/OpenSans-Semibold.ttf", "FontText", DrawnUi.Draw.FontWeight.SemiBold))
            .ConfigureStyles(styles => styles
                .AddStyle(new Style
                {
                    TargetType = typeof(SkiaLabel),
                    ApplyToDerivedTypes = true,
                    Setters = { new Setter { Property = SkiaLabel.FontFamilyProperty, Value = "FontText" } },
                })
                .AddStyle(new Style
                {
                    TargetType = typeof(SkiaButton),
                    ApplyToDerivedTypes = true,
                    Setters = { new Setter { Property = SkiaButton.FontFamilyProperty, Value = "FontText" } },
                }))
            .Build();
    }
}
