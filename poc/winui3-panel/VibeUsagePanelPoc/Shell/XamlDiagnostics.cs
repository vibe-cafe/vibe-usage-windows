using System;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Markup;

namespace VibeUsage.PanelPoc.Shell;

/// <summary>
/// WinUI reports a broken XAML file only as "XAML parsing failed". When the
/// panel can't be built, this resolves every resource it relies on and loads
/// each construct on its own, so the log names the culprit.
/// </summary>
internal static class XamlDiagnostics
{
    private const string Ns =
        "xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\" " +
        "xmlns:x=\"http://schemas.microsoft.com/winfx/2006/xaml\"";

    private static readonly string[] Keys =
    {
        "DefaultButtonStyle", "BodyStrongTextBlockStyle", "BodyTextBlockStyle", "CaptionTextBlockStyle",
        "TitleTextBlockStyle", "ControlCornerRadius", "AccentFillColorDefaultBrush",
        "CardBackgroundFillColorDefaultBrush", "CardStrokeColorDefaultBrush", "DividerStrokeColorDefaultBrush",
        "SystemFillColorCautionBrush", "SystemFillColorCriticalBrush", "TextFillColorPrimaryBrush",
        "TextFillColorSecondaryBrush", "TextFillColorTertiaryBrush", "SolidBackgroundFillColorBaseBrush",
        "PanelCardStyle", "PanelSectionTitleStyle", "PanelBodyStyle", "PanelBodyStrongStyle",
        "PanelBodyStrongCriticalStyle", "PanelBodyStrongCautionStyle", "PanelCaptionStyle",
        "PanelCaptionSecondaryStyle", "PanelCaptionTertiaryStyle", "PanelCaptionCriticalStyle",
        "PanelCaptionCautionStyle", "PanelBarStyle", "PanelBarCriticalStyle", "PanelBarCautionStyle",
        "PanelRingStyle", "PanelRingCriticalStyle", "PanelRingCautionStyle", "PanelChartBarStyle",
        "PanelDividerStyle", "PanelGlyphPathStyle", "PanelGlyphPathTertiaryStyle", "PanelIconButtonStyle",
        "PanelTextButtonStyle", "PanelRowButtonStyle", "SettingsCardStyle", "BareButtonStyle",
    };

    private static readonly (string Name, string Xaml)[] Snippets =
    {
        ("grid-language", "<Grid " + Ns + " Language=\"zh-CN\" />"),
        ("theme-dictionaries", "<Grid " + Ns + "><Grid.Resources><ResourceDictionary><ResourceDictionary.ThemeDictionaries>" +
            "<ResourceDictionary x:Key=\"Default\"><SolidColorBrush x:Key=\"ButtonBackgroundPointerOver\" Color=\"#0FFFFFFF\" /></ResourceDictionary>" +
            "<ResourceDictionary x:Key=\"Light\"><SolidColorBrush x:Key=\"ButtonBackgroundPointerOver\" Color=\"#09000000\" /></ResourceDictionary>" +
            "<ResourceDictionary x:Key=\"HighContrast\" /></ResourceDictionary.ThemeDictionaries></ResourceDictionary></Grid.Resources></Grid>"),
        ("focus-sink", "<ContentControl " + Ns + " Width=\"1\" Height=\"1\" Opacity=\"0\" IsTabStop=\"True\" UseSystemFocusVisuals=\"False\" AutomationProperties.Name=\"x\" />"),
        ("bitmap-icon", "<BitmapIcon " + Ns + " Width=\"18\" Height=\"18\" ShowAsMonochrome=\"True\" UriSource=\"ms-appx:///Assets/Glyph.png\" />"),
        ("icon-button", "<Button " + Ns + " Style=\"{StaticResource PanelIconButtonStyle}\" ToolTipService.ToolTip=\"x\"><FontIcon Glyph=\"&#xE895;\" FontSize=\"16\" /></Button>"),
        ("progress-ring", "<ProgressRing " + Ns + " Width=\"16\" Height=\"16\" IsActive=\"False\" Visibility=\"Collapsed\" />"),
        ("selector-bar", "<SelectorBar " + Ns + " Margin=\"6,2,6,0\" />"),
        ("scroll-viewer", "<ScrollViewer " + Ns + " HorizontalScrollMode=\"Disabled\" HorizontalScrollBarVisibility=\"Disabled\" VerticalScrollMode=\"Auto\" VerticalScrollBarVisibility=\"Auto\" />"),
        ("caption-secondary", "<TextBlock " + Ns + " Style=\"{StaticResource PanelCaptionSecondaryStyle}\" TextTrimming=\"CharacterEllipsis\" />"),
        ("section-title", "<TextBlock " + Ns + " Style=\"{StaticResource PanelSectionTitleStyle}\" />"),
        ("card", "<Border " + Ns + " Style=\"{StaticResource PanelCardStyle}\" Padding=\"4\" />"),
        ("combo", "<ComboBox " + Ns + " MinWidth=\"0\" AutomationProperties.Name=\"x\" />"),
        ("dropdown", "<DropDownButton " + Ns + " Background=\"Transparent\" BorderThickness=\"0\" Padding=\"8,6,6,6\"><FontIcon Glyph=\"&#xE71C;\" FontSize=\"14\" />" +
            "<DropDownButton.Flyout><MenuFlyout Placement=\"BottomEdgeAlignedRight\"><MenuFlyoutItem Text=\"a\" IsEnabled=\"False\" />" +
            "<ToggleMenuFlyoutItem Text=\"b\" IsChecked=\"True\" /><MenuFlyoutSeparator /></MenuFlyout></DropDownButton.Flyout></DropDownButton>"),
        ("title-text", "<TextBlock " + Ns + " Style=\"{StaticResource TitleTextBlockStyle}\" />"),
        ("body-text", "<TextBlock " + Ns + " Style=\"{StaticResource PanelBodyStyle}\" />"),
        ("expander", "<Expander " + Ns + " HorizontalAlignment=\"Stretch\" HorizontalContentAlignment=\"Stretch\" Header=\"x\"><StackPanel Spacing=\"10\" /></Expander>"),
    };

    public static void Attach(Application app)
    {
        try
        {
            app.DebugSettings.XamlResourceReferenceFailed += (_, e) => PocLog.Write("xaml resource reference failed: " + e.Message);
            app.DebugSettings.BindingFailed += (_, e) => PocLog.Write("xaml binding failed: " + e.Message);
        }
        catch (Exception ex)
        {
            PocLog.Write("debug settings unavailable: " + ex.Message);
        }
    }

    public static void Run()
    {
        foreach (var key in Keys)
        {
            try
            {
                XamlReader.Load("<ContentControl " + Ns + " Tag=\"{StaticResource " + key + "}\" />");
            }
            catch (Exception ex)
            {
                PocLog.Write($"diag resource {key}: ERROR {ex.GetType().Name} {ex.Message}");
            }
        }

        foreach (var (name, xaml) in Snippets)
        {
            try
            {
                XamlReader.Load(xaml);
            }
            catch (Exception ex)
            {
                PocLog.Write($"diag snippet {name}: ERROR {ex.GetType().Name} {ex.Message}");
            }
        }
        PocLog.Write("diag done");
    }
}
