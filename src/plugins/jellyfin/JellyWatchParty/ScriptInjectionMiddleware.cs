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

    public ScriptInjectionMiddleware(RequestDelegate next)
    {
        _next = next;
    }

    public async Task InvokeAsync(HttpContext context, ILogger<ScriptInjectionMiddleware> logger)
    {
        var path = context.Request.Path.Value?.TrimEnd('/');
        if (Plugin.InjectionEnabled && path is "/web" or "/web/index.html")
        {
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
        }

        await _next(context);
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
