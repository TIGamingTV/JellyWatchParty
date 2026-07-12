using System.Reflection;
using System.Runtime.Loader;
using System.Text.RegularExpressions;
using MediaBrowser.Model.Tasks;
using Microsoft.Extensions.Logging;
using Newtonsoft.Json.Linq;

namespace JellyWatchParty.Plugin;

/// <summary>
/// Scheduled task that injects the JellyWatchParty client script into index.html.
/// First attempts to register with the File Transformation plugin (if installed).
/// Falls back to direct injection into the physical index.html file.
/// </summary>
public class FileTransformationIntegration : IScheduledTask
{
    private const string ClientScriptPath = "../JellyWatchParty/ClientScript";
    private const string ScriptTag = $"<script src=\"{ClientScriptPath}\" defer></script>";

    // Matches any <script> tag referencing the plugin's ClientScript endpoint
    // (regardless of the exact src spelling or attribute order), along with the
    // leading indentation and trailing newline InjectScript adds. Used to
    // reverse a direct-file injection so an uninstall leaves index.html clean.
    private static readonly Regex ScriptTagRegex = new(
        @"[ \t]*<script\b[^>]*JellyWatchParty/ClientScript[^>]*>\s*</script>[ \t]*\r?\n?",
        RegexOptions.IgnoreCase | RegexOptions.Compiled);

    private readonly ILogger<FileTransformationIntegration> _logger;

    public string Name => "JellyWatchParty File Transformation Registration";
    public string Key => "JellyWatchPartyFileTransformation";
    public string Description => "Registers automatic script injection with the File Transformation plugin";
    public string Category => "JellyWatchParty";

    public FileTransformationIntegration(ILogger<FileTransformationIntegration> logger)
    {
        _logger = logger;
    }

    public IEnumerable<TaskTriggerInfo> GetDefaultTriggers()
    {
        return new[]
        {
            new TaskTriggerInfo { Type = TaskTriggerInfoType.StartupTrigger }
        };
    }

    public async Task ExecuteAsync(IProgress<double> progress, CancellationToken cancellationToken)
    {
        progress.Report(0);

        // Injection is disabled while the plugin is being uninstalled, so the
        // startup task never re-adds the script we're about to clean up.
        if (!Plugin.InjectionEnabled)
        {
            _logger.LogInformation("[JellyWatchParty] Script injection is disabled; skipping registration.");
            progress.Report(100);
            return;
        }

        if (TryRegisterFileTransformation())
        {
            progress.Report(100);
            return;
        }

        // File Transformation unavailable — inject directly into the physical file
        await InjectIntoIndexHtmlFileAsync(cancellationToken).ConfigureAwait(false);

        progress.Report(100);
    }

    /// <summary>
    /// Attempts to register with the File Transformation plugin via reflection.
    /// Returns true if registration succeeded.
    /// </summary>
    private bool TryRegisterFileTransformation()
    {
        try
        {
            var ftAssembly = AssemblyLoadContext.All
                .SelectMany(ctx => ctx.Assemblies)
                .FirstOrDefault(asm => asm.FullName?.Contains("Jellyfin.Plugin.FileTransformation") ?? false);

            if (ftAssembly == null)
            {
                return false;
            }

            var pluginInterface = ftAssembly.GetType("Jellyfin.Plugin.FileTransformation.PluginInterface");
            if (pluginInterface == null)
            {
                _logger.LogWarning("[JellyWatchParty] File Transformation plugin found but PluginInterface type not available. "
                    + "The installed version may be incompatible.");
                return false;
            }

            var registerMethod = pluginInterface.GetMethod("RegisterTransformation", BindingFlags.Public | BindingFlags.Static);
            if (registerMethod == null)
            {
                _logger.LogWarning("[JellyWatchParty] File Transformation plugin found but RegisterTransformation method not available. "
                    + "The installed version may be incompatible.");
                return false;
            }

            var payload = new JObject
            {
                ["id"] = Guid.Parse(Plugin.PluginGuid),
                ["fileNamePattern"] = @"^index\.html$",
                ["callbackAssembly"] = typeof(FileTransformationIntegration).Assembly.FullName,
                ["callbackClass"] = typeof(FileTransformationIntegration).FullName,
                ["callbackMethod"] = nameof(TransformIndexHtml)
            };

            registerMethod.Invoke(null, new object?[] { payload });

            _logger.LogInformation("[JellyWatchParty] Registered index.html transformation with File Transformation plugin.");
            return true;
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "[JellyWatchParty] Failed to register with File Transformation plugin. "
                + "Falling back to direct index.html injection.");
            return false;
        }
    }

    /// <summary>
    /// Directly injects the script tag into the physical index.html file
    /// in Jellyfin's web directory. This is the fallback when File Transformation
    /// is unavailable (e.g., after an in-process restart on Jellyfin 10.11.6+).
    /// </summary>
    private async Task InjectIntoIndexHtmlFileAsync(CancellationToken cancellationToken)
    {
        var indexPath = ResolveIndexHtmlPath();
        if (string.IsNullOrEmpty(indexPath))
        {
            _logger.LogInformation("[JellyWatchParty] File Transformation plugin not available and JELLYFIN_WEB_DIR not set. "
                + "Script injection will not be automatic — use Custom HTML instead.");
            return;
        }

        if (!File.Exists(indexPath))
        {
            _logger.LogWarning("[JellyWatchParty] index.html not found at '{Path}'. "
                + "Script injection will not be automatic — use Custom HTML instead.", indexPath);
            return;
        }

        try
        {
            var html = await File.ReadAllTextAsync(indexPath, cancellationToken).ConfigureAwait(false);
            var modified = InjectScript(html);

            if (modified == html)
            {
                _logger.LogInformation("[JellyWatchParty] Client script already present in {Path}.", indexPath);
                return;
            }

            await File.WriteAllTextAsync(indexPath, modified, cancellationToken).ConfigureAwait(false);
            _logger.LogInformation("[JellyWatchParty] Injected client script into {Path} (direct fallback).", indexPath);
        }
        catch (UnauthorizedAccessException)
        {
            _logger.LogInformation("[JellyWatchParty] No write permission to {Path}. "
                + "Using controller-level index.html interception instead.", indexPath);
        }
        catch (Exception ex)
        {
            _logger.LogWarning(ex, "[JellyWatchParty] Failed to inject script into {Path}. "
                + "Using controller-level index.html interception instead.", indexPath);
        }
    }

    /// <summary>
    /// Core injection logic: inserts the script tag before &lt;/body&gt; or &lt;/head&gt;
    /// if the script is not already present.
    /// </summary>
    internal static string InjectScript(string contents)
    {
        if (string.IsNullOrEmpty(contents) || contents.Contains("JellyWatchParty/ClientScript", StringComparison.OrdinalIgnoreCase))
        {
            return contents ?? string.Empty;
        }

        var bodyEndIndex = contents.LastIndexOf("</body>", StringComparison.OrdinalIgnoreCase);
        if (bodyEndIndex >= 0)
        {
            return contents.Insert(bodyEndIndex, $"    {ScriptTag}\n");
        }

        var headEndIndex = contents.LastIndexOf("</head>", StringComparison.OrdinalIgnoreCase);
        if (headEndIndex >= 0)
        {
            return contents.Insert(headEndIndex, $"    {ScriptTag}\n");
        }

        return contents;
    }

    /// <summary>
    /// Reverses <see cref="InjectScript"/>: removes any JellyWatchParty client
    /// script tag (and the whitespace it was inserted with) from the given
    /// contents. Returns the input unchanged when no tag is present.
    /// </summary>
    internal static string RemoveScript(string contents)
    {
        if (string.IsNullOrEmpty(contents))
        {
            return contents ?? string.Empty;
        }

        return ScriptTagRegex.Replace(contents, string.Empty);
    }

    /// <summary>
    /// Resolves the physical path to the web client's index.html from
    /// JELLYFIN_WEB_DIR, or null when the variable is not set.
    /// </summary>
    internal static string? ResolveIndexHtmlPath()
    {
        var webDir = Environment.GetEnvironmentVariable("JELLYFIN_WEB_DIR");
        return string.IsNullOrEmpty(webDir) ? null : Path.Combine(webDir, "index.html");
    }

    /// <summary>
    /// Removes a previously injected client script tag from the physical
    /// index.html file. Called when the plugin is uninstalled so the direct
    /// file injection is fully reversed instead of leaving a dangling script
    /// tag that requests a now-nonexistent endpoint. Returns true when the
    /// file was modified.
    /// </summary>
    internal static bool RemoveInjectedScriptFromIndexHtml(ILogger logger)
    {
        var indexPath = ResolveIndexHtmlPath();
        if (string.IsNullOrEmpty(indexPath) || !File.Exists(indexPath))
        {
            return false;
        }

        try
        {
            var html = File.ReadAllText(indexPath);
            var cleaned = RemoveScript(html);
            if (cleaned == html)
            {
                return false;
            }

            File.WriteAllText(indexPath, cleaned);
            logger.LogInformation("[JellyWatchParty] Removed injected client script from {Path}.", indexPath);
            return true;
        }
        catch (Exception ex)
        {
            logger.LogWarning(ex, "[JellyWatchParty] Failed to remove injected client script from {Path}. "
                + "You may need to remove the JellyWatchParty <script> tag from index.html manually.", indexPath);
            return false;
        }
    }

    /// <summary>
    /// Callback invoked by the File Transformation plugin to inject the
    /// JellyWatchParty script tag into index.html.
    /// </summary>
    public static string TransformIndexHtml(object payload)
    {
        var contents = payload is JObject jobj
            ? jobj["contents"]?.ToString()
                ?? jobj["Contents"]?.ToString()
                ?? jobj["content"]?.ToString()
                ?? jobj["Content"]?.ToString()
            : payload?.GetType()
                .GetProperty("contents")?
                .GetValue(payload)?
                .ToString()
                ?? payload?.GetType()
                    .GetProperty("Contents")?
                    .GetValue(payload)?
                    .ToString()
                ?? payload?.GetType()
                    .GetProperty("content")?
                    .GetValue(payload)?
                    .ToString()
                ?? payload?.GetType()
                    .GetProperty("Content")?
                .GetValue(payload)?
                .ToString();

        // Once the plugin is being uninstalled, return index.html untouched so
        // the File Transformation plugin stops serving the injected script even
        // before the next server restart.
        if (!Plugin.InjectionEnabled)
        {
            return contents ?? string.Empty;
        }

        return InjectScript(contents ?? string.Empty);
    }
}
