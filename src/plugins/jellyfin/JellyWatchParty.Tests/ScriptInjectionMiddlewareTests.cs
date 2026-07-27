using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace JellyWatchParty.Plugin.Tests;

/// <summary>
/// Covers when the request-level fallback is allowed to answer index.html.
/// Serving it short-circuits the pipeline, so it must stand down whenever the
/// File Transformation plugin is present - otherwise it discards the index.html
/// injections of every other plugin that goes through File Transformation.
/// </summary>
[Collection(InjectionStateCollection.Name)]
public class ScriptInjectionMiddlewareTests : IDisposable
{
    private const string ScriptTag = "<script src=\"../JellyWatchParty/ClientScript\" defer></script>";
    private const string IndexHtml = "<html><head></head><body><h1>Jellyfin</h1></body></html>";

    private readonly string _tempDir;
    private readonly string? _originalWebDir;

    public ScriptInjectionMiddlewareTests()
    {
        ScriptInjectionMiddleware.ResetForTests();

        _tempDir = Path.Combine(Path.GetTempPath(), "jwp-mw-test-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_tempDir);
        File.WriteAllText(Path.Combine(_tempDir, "index.html"), IndexHtml);

        _originalWebDir = Environment.GetEnvironmentVariable("JELLYFIN_WEB_DIR");
        Environment.SetEnvironmentVariable("JELLYFIN_WEB_DIR", _tempDir);
    }

    public void Dispose()
    {
        Environment.SetEnvironmentVariable("JELLYFIN_WEB_DIR", _originalWebDir);
        Directory.Delete(_tempDir, recursive: true);
        ScriptInjectionMiddleware.ResetForTests();
        Plugin.InjectionEnabled = true;
        GC.SuppressFinalize(this);
    }

    /// <summary>
    /// Runs the middleware against the given path and returns whether the rest
    /// of the pipeline ran, plus whatever the middleware wrote to the response.
    /// </summary>
    private static async Task<(bool NextCalled, string Body)> InvokeAsync(string path)
    {
        var context = new DefaultHttpContext();
        context.Request.Path = path;
        var body = new MemoryStream();
        context.Response.Body = body;

        var nextCalled = false;
        var middleware = new ScriptInjectionMiddleware(_ =>
        {
            nextCalled = true;
            return Task.CompletedTask;
        });

        await middleware.InvokeAsync(context, NullLogger<ScriptInjectionMiddleware>.Instance);

        return (nextCalled, System.Text.Encoding.UTF8.GetString(body.ToArray()));
    }

    [Theory]
    [InlineData("/web")]
    [InlineData("/web/index.html")]
    public async Task StandsDown_WhenFileTransformationAvailable(string path)
    {
        ScriptInjectionMiddleware.FileTransformationProbe = () => true;

        var (nextCalled, responseBody) = await InvokeAsync(path);

        // The pipeline must continue so File Transformation - and through it
        // every other plugin - still gets to serve index.html.
        Assert.True(nextCalled);
        Assert.Equal(string.Empty, responseBody);
    }

    [Theory]
    [InlineData("/web")]
    [InlineData("/web/index.html")]
    public async Task ServesInjectedIndex_WhenFileTransformationUnavailable(string path)
    {
        ScriptInjectionMiddleware.FileTransformationProbe = () => false;

        var (nextCalled, responseBody) = await InvokeAsync(path);

        Assert.False(nextCalled);
        Assert.Contains(ScriptTag, responseBody);
    }

    [Fact]
    public async Task StandsDown_WhenInjectionDisabled()
    {
        ScriptInjectionMiddleware.FileTransformationProbe = () => false;
        Plugin.InjectionEnabled = false;

        var (nextCalled, responseBody) = await InvokeAsync("/web/index.html");

        Assert.True(nextCalled);
        Assert.Equal(string.Empty, responseBody);
    }

    [Fact]
    public async Task IgnoresUnrelatedPaths()
    {
        ScriptInjectionMiddleware.FileTransformationProbe = () => false;

        var (nextCalled, responseBody) = await InvokeAsync("/web/main.jellyfin.bundle.js");

        Assert.True(nextCalled);
        Assert.Equal(string.Empty, responseBody);
    }

    [Fact]
    public async Task DoesNotCacheIndexHtml_WhileStandingDown()
    {
        // A stand-down must not populate the cache: if it did, a later request
        // could be answered from a snapshot that omits other plugins' scripts.
        ScriptInjectionMiddleware.FileTransformationProbe = () => true;
        await InvokeAsync("/web/index.html");

        ScriptInjectionMiddleware.FileTransformationProbe = () => false;
        File.WriteAllText(
            Path.Combine(_tempDir, "index.html"),
            "<html><body><h1>Updated</h1></body></html>");

        var (_, responseBody) = await InvokeAsync("/web/index.html");

        Assert.Contains("Updated", responseBody);
        Assert.Contains(ScriptTag, responseBody);
    }
}
