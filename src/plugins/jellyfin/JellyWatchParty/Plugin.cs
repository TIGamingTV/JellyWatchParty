using MediaBrowser.Common.Configuration;
using MediaBrowser.Common.Plugins;
using MediaBrowser.Model.Plugins;
using MediaBrowser.Model.Serialization;
using Microsoft.Extensions.Logging;
using JellyWatchParty.Plugin.Configuration;

namespace JellyWatchParty.Plugin;

public class Plugin : BasePlugin<PluginConfiguration>, IHasWebPages
{
    /// <summary>
    /// The plugin's unique identifier (GUID).
    /// This constant is used both in Plugin.Id and should match configPage.html.
    /// </summary>
    public const string PluginGuid = "0f2fd0fd-09ff-4f49-9f1c-4a8f421a4b7d";

    /// <summary>
    /// Singleton instance - standard Jellyfin plugin pattern.
    /// Thread-safe: set once during plugin initialization by Jellyfin's plugin loader.
    /// </summary>
    public static Plugin? Instance { get; private set; }

    /// <summary>
    /// Gates all client-script injection paths (the startup middleware, the
    /// File Transformation callback, and the direct file fallback). Set to
    /// false when the plugin is being uninstalled so injection stops
    /// immediately, without waiting for a server restart.
    /// </summary>
    public static bool InjectionEnabled { get; internal set; } = true;

    private readonly ILogger<Plugin> _logger;

    public Plugin(IApplicationPaths applicationPaths, IXmlSerializer xmlSerializer, ILogger<Plugin> logger)
        : base(applicationPaths, xmlSerializer)
    {
        Instance = this;
        _logger = logger;

        if (string.IsNullOrEmpty(Configuration.JwtSecret))
        {
            _logger.LogWarning("[JellyWatchParty] JwtSecret is not configured. Authentication is DISABLED. " +
                "Set a JwtSecret (min {MinLength} characters) in the plugin configuration to enable authentication.",
                PluginConfiguration.MinJwtSecretLength);
        }
        else if (!Configuration.HasUsableJwtSecret)
        {
            _logger.LogWarning("[JellyWatchParty] JwtSecret is too short ({Length} chars, minimum {MinLength}). " +
                "Authentication is DISABLED until you set a longer secret (or clear it to explicitly disable auth).",
                Configuration.JwtSecret.Length, PluginConfiguration.MinJwtSecretLength);
        }
        else
        {
            _logger.LogInformation("[JellyWatchParty] JWT authentication is enabled.");
        }

        foreach (var warning in PluginConfiguration.ValidateSessionServerUrl(Configuration.SessionServerUrl))
        {
            _logger.LogWarning("[JellyWatchParty] SessionServerUrl: {Warning}", warning);
        }
    }

    public override string Name => "JellyWatchParty";

    public override Guid Id => new(PluginGuid);

    /// <summary>
    /// Gets the plugin description.
    /// </summary>
    public override string Description => "Watch movies together in sync with friends";

    /// <summary>
    /// Gets the plugin version from the assembly (fixes L17).
    /// </summary>
    public static string PluginVersion => typeof(Plugin).Assembly.GetName().Version?.ToString(3) ?? "1.0.0";

    /// <summary>
    /// Called by Jellyfin when the plugin is being uninstalled. Reverses the
    /// direct index.html injection so the web client is left clean instead of
    /// referencing the plugin's (now removed) ClientScript endpoint, and stops
    /// any remaining in-process injection paths from re-adding the script.
    /// </summary>
    public override void OnUninstalling()
    {
        InjectionEnabled = false;

        try
        {
            FileTransformationIntegration.RemoveInjectedScriptFromIndexHtml(_logger);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "[JellyWatchParty] Failed to clean up injected client script during uninstall.");
        }

        base.OnUninstalling();
    }

    public IEnumerable<PluginPageInfo> GetPages()
    {
        return new[]
        {
            new PluginPageInfo
            {
                Name = "JellyWatchParty",
                EmbeddedResourcePath = GetType().Namespace + ".Web.configPage.html"
            }
        };
    }
}
