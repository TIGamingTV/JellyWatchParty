using System.Text;
using MediaBrowser.Controller;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Hosting;
using Microsoft.Extensions.Logging;
using JellyWatchParty.Plugin.Services;

namespace JellyWatchParty.Plugin;

/// <summary>
/// ASP.NET Core middleware that intercepts requests for the Jellyfin web client
/// index.html and injects the JellyWatchParty client script tag.
///
/// Registered via IStartupFilter so it runs BEFORE Jellyfin's static file
/// middleware, which would otherwise serve the unmodified index.html.
///
/// This is a LAST-RESORT fallback, used only when the File Transformation
/// plugin is not available. It answers the request from the physical
/// index.html and never calls the rest of the pipeline, so anything else that
/// serves index.html - notably File Transformation, through which plugins like
/// Jellyfin Enhanced, Media Bar and Custom Tabs inject themselves - would be
/// bypassed entirely. Whenever File Transformation is present it owns
/// index.html and this middleware stands down.
/// </summary>
public class ScriptInjectionMiddleware
{
    private sealed class CachedContent
    {
        public required byte[] Content { get; init; }
        public required string ETag { get; init; }
    }

    private readonly RequestDelegate _next;

    // Caches only successful loads, not failures - a transient failure (e.g.
    // index.html not fully written yet at first request) must not
    // permanently disable script injection for the process's whole
    // lifetime. Guarded by _loadLock only while unpopulated; once set, reads
    // never take the lock. A reference-type wrapper (rather than a
    // (byte[], string)? tuple) is used so the field can be `volatile` -
    // C# doesn't allow `volatile` on Nullable<T>/struct fields, and without
    // it a multi-field struct read isn't guaranteed atomic across threads.
    private static volatile CachedContent? _cachedContent;
    private static readonly object _loadLock = new();

    // Probes for a usable File Transformation plugin. Overridable so tests can
    // exercise both the "stands down" and "takes over" branches without having
    // to load a real File Transformation assembly.
    internal static Func<bool> FileTransformationProbe { get; set; }
        = FileTransformationIntegration.IsFileTransformationAvailable;

    // 0 until the stand-down reason has been logged, so it is reported once per
    // process rather than on every page load.
    private static int _deferralLogged;

    // Latched once File Transformation has been handed a request for index.html
    // and demonstrably did not run our transformation. From then on we stop
    // deferring to it - see VerifyFileTransformationRan.
    private static volatile bool _fileTransformationUnverified;

    public ScriptInjectionMiddleware(RequestDelegate next)
    {
        _next = next;
    }

    public async Task InvokeAsync(HttpContext context, ILogger<ScriptInjectionMiddleware> logger)
    {
        var path = context.Request.Path.Value?.TrimEnd('/');
        if (!Plugin.InjectionEnabled || path is not ("/web" or "/web/index.html"))
        {
            await _next(context);
            return;
        }

        if (ShouldDeferToFileTransformation(logger))
        {
            // Let File Transformation serve index.html, then check that it
            // really did run our transformation. If it did not, we stop
            // deferring and take the request over from here on.
            await _next(context);
            VerifyFileTransformationRan(context, logger);
            return;
        }

        var cached = GetOrLoadContent(logger);
        if (cached != null)
        {
            var requestETag = context.Request.Headers.IfNoneMatch.FirstOrDefault();
            if (!string.IsNullOrEmpty(requestETag) && requestETag == cached.ETag)
            {
                context.Response.StatusCode = 304;
                return;
            }

            context.Response.ContentType = "text/html; charset=utf-8";
            context.Response.Headers.CacheControl = "no-cache";
            context.Response.Headers.ETag = cached.ETag;
            context.Response.ContentLength = cached.Content.Length;
            await context.Response.Body.WriteAsync(cached.Content);
            return;
        }

        await _next(context);
    }

    /// <summary>
    /// Checks, after File Transformation has been given a request for
    /// index.html to serve, that it actually invoked our transformation
    /// callback. When it did not, the plugin is loaded but not functional -
    /// which on its own would leave us deferring forever to something that
    /// never injects the script, with nothing in the log to say so. Latching
    /// <see cref="_fileTransformationUnverified"/> hands index.html back to
    /// this middleware from the next request onwards.
    /// </summary>
    private static void VerifyFileTransformationRan(HttpContext context, ILogger logger)
    {
        if (_fileTransformationUnverified || FileTransformationIntegration.TransformationInvoked)
        {
            return;
        }

        // Anything other than a rendered 200 - a 304 from the browser's cache,
        // a 404, an error - means File Transformation was never asked to
        // produce a body, so this request says nothing either way.
        if (context.Response.StatusCode != StatusCodes.Status200OK)
        {
            return;
        }

        _fileTransformationUnverified = true;
        logger.LogWarning("[JellyWatchParty] The File Transformation plugin is loaded but did not run the "
            + "Watch Party transformation while serving index.html, so the client script was not injected. "
            + "This usually means the installed File Transformation build does not support this Jellyfin "
            + "version. Falling back to request-level injection from the next page load; note that other "
            + "plugins' index.html changes will not be applied while this fallback is active.");
    }

    /// <summary>
    /// True when the File Transformation plugin is available and should be left
    /// to serve index.html. Serving it ourselves would short-circuit the
    /// pipeline and silently drop the index.html changes made by every other
    /// plugin that injects through File Transformation; our own script still
    /// gets in via the transformation registered in
    /// <see cref="FileTransformationIntegration"/>.
    /// </summary>
    private static bool ShouldDeferToFileTransformation(ILogger logger)
    {
        if (_fileTransformationUnverified || !FileTransformationProbe())
        {
            return false;
        }

        if (Interlocked.Exchange(ref _deferralLogged, 1) == 0)
        {
            logger.LogInformation("[JellyWatchParty] File Transformation plugin detected; "
                + "leaving index.html to it so other plugins' injections are preserved. "
                + "The Watch Party script is added through the registered transformation instead.");
        }

        return true;
    }

    /// <summary>
    /// Clears the process-wide state this middleware memoises, so tests do not
    /// leak a cached index.html or a "already logged" flag between cases.
    /// </summary>
    internal static void ResetForTests()
    {
        _cachedContent = null;
        _deferralLogged = 0;
        _fileTransformationUnverified = false;
        FileTransformationProbe = FileTransformationIntegration.IsFileTransformationAvailable;
        FileTransformationIntegration.ResetForTests();
    }

    private static CachedContent? GetOrLoadContent(ILogger logger)
    {
        var cached = _cachedContent;
        if (cached != null)
        {
            return cached;
        }

        lock (_loadLock)
        {
            if (_cachedContent != null)
            {
                return _cachedContent;
            }

            try
            {
                var webDir = Environment.GetEnvironmentVariable("JELLYFIN_WEB_DIR")
                    ?? "/usr/share/jellyfin/web";
                var indexPath = Path.Combine(webDir, "index.html");
                var html = File.ReadAllText(indexPath);
                var modified = FileTransformationIntegration.InjectScript(html);

                var bytes = Encoding.UTF8.GetBytes(modified);
                var hash = System.Security.Cryptography.SHA256.HashData(bytes);
                var etag = $"\"{Convert.ToBase64String(hash)[..16]}\"";
                _cachedContent = new CachedContent { Content = bytes, ETag = etag };
                return _cachedContent;
            }
            catch (Exception ex)
            {
                logger.LogWarning(ex, "[JellyWatchParty] Failed to inject client script into index.html - " +
                    "the Watch Party button will not appear until this succeeds. Will retry on the next request.");
                return null;
            }
        }
    }
}

/// <summary>
/// Startup filter that registers the script injection middleware at the very
/// beginning of the pipeline, before static files middleware.
/// </summary>
public class ScriptInjectionStartupFilter : IStartupFilter
{
    public Action<IApplicationBuilder> Configure(Action<IApplicationBuilder> next)
    {
        return app =>
        {
            app.UseMiddleware<ScriptInjectionMiddleware>();
            next(app);
        };
    }
}

/// <summary>
/// Registers plugin services with Jellyfin's DI container.
/// This is called during ConfigureServices, before the middleware pipeline is built.
/// </summary>
public class ServiceRegistrator : MediaBrowser.Controller.Plugins.IPluginServiceRegistrator
{
    /// <inheritdoc />
    public void RegisterServices(IServiceCollection serviceCollection, IServerApplicationHost applicationHost)
    {
        serviceCollection.AddSingleton<IStartupFilter, ScriptInjectionStartupFilter>();

        // Singleton + hosted-service-wrapping-the-same-instance: eagerly
        // subscribes to ISessionManager's playback events at server startup,
        // while remaining directly injectable into the controller for
        // admin-triggered start/stop actions.
        serviceCollection.AddSingleton<HostBridgeManager>();
        serviceCollection.AddHostedService(sp => sp.GetRequiredService<HostBridgeManager>());
    }
}
