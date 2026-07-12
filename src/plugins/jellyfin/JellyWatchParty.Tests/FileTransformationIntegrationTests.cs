using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace JellyWatchParty.Plugin.Tests;

public class FileTransformationIntegrationTests
{
    private const string ScriptTag = "<script src=\"../JellyWatchParty/ClientScript\" defer></script>";

    private class FakePayload
    {
        public string? contents { get; set; }
    }

    private static object MakePayload(string? contents) => new FakePayload { contents = contents };

    // -- InjectScript (core logic, used by both FT callback and direct file injection) --

    [Fact]
    public void InjectScript_InjectsBeforeBodyClose()
    {
        var html = "<html><head></head><body><h1>Jellyfin</h1></body></html>";
        var result = FileTransformationIntegration.InjectScript(html);

        Assert.Contains(ScriptTag, result);
        Assert.True(result.IndexOf(ScriptTag) < result.LastIndexOf("</body>"));
    }

    [Fact]
    public void InjectScript_InjectsBeforeHeadClose_WhenNoBody()
    {
        var html = "<html><head><title>Jellyfin</title></head><div>no body tag</div></html>";
        var result = FileTransformationIntegration.InjectScript(html);

        Assert.Contains(ScriptTag, result);
        Assert.True(result.IndexOf(ScriptTag) < result.LastIndexOf("</head>"));
    }

    [Fact]
    public void InjectScript_SkipsInjection_WhenAlreadyPresent()
    {
        var html = $"<html><body>{ScriptTag}</body></html>";
        var result = FileTransformationIntegration.InjectScript(html);

        Assert.Equal(html, result);
    }

    [Fact]
    public void InjectScript_SkipsInjection_WhenAbsolutePathPresent()
    {
        var html = "<html><body><script src=\"/JellyWatchParty/ClientScript\"></script></body></html>";
        var result = FileTransformationIntegration.InjectScript(html);

        Assert.Equal(html, result);
    }

    [Fact]
    public void InjectScript_ReturnsEmpty_WhenNull()
    {
        var result = FileTransformationIntegration.InjectScript(null!);
        Assert.Equal(string.Empty, result);
    }

    [Fact]
    public void InjectScript_ReturnsEmpty_WhenEmpty()
    {
        var result = FileTransformationIntegration.InjectScript(string.Empty);
        Assert.Equal(string.Empty, result);
    }

    [Fact]
    public void InjectScript_ReturnsUnchanged_WhenNoBodyOrHead()
    {
        var html = "<html><div>no head or body</div></html>";
        var result = FileTransformationIntegration.InjectScript(html);

        Assert.Equal(html, result);
    }

    // -- TransformIndexHtml (FT callback, extracts contents from payload) --

    [Fact]
    public void TransformIndexHtml_InjectsScript_WhenNotPresent()
    {
        var html = "<html><head></head><body><h1>Jellyfin</h1></body></html>";
        var result = FileTransformationIntegration.TransformIndexHtml(MakePayload(html));

        Assert.Contains(ScriptTag, result);
        Assert.True(result.IndexOf(ScriptTag) < result.LastIndexOf("</body>"));
    }

    [Fact]
    public void TransformIndexHtml_SkipsInjection_WhenAlreadyPresent()
    {
        var html = $"<html><body>{ScriptTag}</body></html>";
        var result = FileTransformationIntegration.TransformIndexHtml(MakePayload(html));

        Assert.Equal(html, result);
    }

    [Fact]
    public void TransformIndexHtml_ReturnsEmpty_WhenContentIsNull()
    {
        var result = FileTransformationIntegration.TransformIndexHtml(MakePayload(null));

        Assert.Equal(string.Empty, result);
    }

    [Fact]
    public void TransformIndexHtml_DoesNotInject_WhenInjectionDisabled()
    {
        var html = "<html><head></head><body><h1>Jellyfin</h1></body></html>";
        Plugin.InjectionEnabled = false;
        try
        {
            var result = FileTransformationIntegration.TransformIndexHtml(MakePayload(html));
            Assert.Equal(html, result);
            Assert.DoesNotContain(ScriptTag, result);
        }
        finally
        {
            Plugin.InjectionEnabled = true;
        }
    }

    // -- RemoveScript (reverses InjectScript) --

    [Fact]
    public void RemoveScript_RemovesInjectedTag()
    {
        var html = "<html><head></head><body><h1>Jellyfin</h1></body></html>";
        var injected = FileTransformationIntegration.InjectScript(html);

        Assert.Contains(ScriptTag, injected);
        Assert.DoesNotContain(ScriptTag, FileTransformationIntegration.RemoveScript(injected));
    }

    [Fact]
    public void RemoveScript_IsInverseOfInjectScript()
    {
        var html = "<html><head></head><body><h1>Jellyfin</h1></body></html>";
        var roundTripped = FileTransformationIntegration.RemoveScript(FileTransformationIntegration.InjectScript(html));

        Assert.Equal(html, roundTripped);
    }

    [Fact]
    public void RemoveScript_RemovesAbsolutePathVariant()
    {
        var html = "<html><body><script src=\"/JellyWatchParty/ClientScript\"></script></body></html>";
        var result = FileTransformationIntegration.RemoveScript(html);

        Assert.DoesNotContain("JellyWatchParty/ClientScript", result);
    }

    [Fact]
    public void RemoveScript_ReturnsUnchanged_WhenNoTagPresent()
    {
        var html = "<html><body><h1>Jellyfin</h1></body></html>";
        var result = FileTransformationIntegration.RemoveScript(html);

        Assert.Equal(html, result);
    }

    [Fact]
    public void RemoveScript_ReturnsEmpty_WhenNull()
    {
        Assert.Equal(string.Empty, FileTransformationIntegration.RemoveScript(null!));
    }

    // -- RemoveInjectedScriptFromIndexHtml (physical file cleanup on uninstall) --

    [Fact]
    public void RemoveInjectedScriptFromIndexHtml_CleansPhysicalFile()
    {
        var tempDir = Path.Combine(Path.GetTempPath(), "jwp-test-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(tempDir);
        var originalWebDir = Environment.GetEnvironmentVariable("JELLYFIN_WEB_DIR");
        try
        {
            var original = "<html><head></head><body><h1>Jellyfin</h1></body></html>";
            var indexPath = Path.Combine(tempDir, "index.html");
            File.WriteAllText(indexPath, FileTransformationIntegration.InjectScript(original));
            Environment.SetEnvironmentVariable("JELLYFIN_WEB_DIR", tempDir);

            var changed = FileTransformationIntegration.RemoveInjectedScriptFromIndexHtml(NullLogger.Instance);

            Assert.True(changed);
            Assert.Equal(original, File.ReadAllText(indexPath));
        }
        finally
        {
            Environment.SetEnvironmentVariable("JELLYFIN_WEB_DIR", originalWebDir);
            Directory.Delete(tempDir, recursive: true);
        }
    }

    [Fact]
    public void RemoveInjectedScriptFromIndexHtml_ReturnsFalse_WhenNothingToRemove()
    {
        var tempDir = Path.Combine(Path.GetTempPath(), "jwp-test-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(tempDir);
        var originalWebDir = Environment.GetEnvironmentVariable("JELLYFIN_WEB_DIR");
        try
        {
            var original = "<html><body><h1>Jellyfin</h1></body></html>";
            File.WriteAllText(Path.Combine(tempDir, "index.html"), original);
            Environment.SetEnvironmentVariable("JELLYFIN_WEB_DIR", tempDir);

            Assert.False(FileTransformationIntegration.RemoveInjectedScriptFromIndexHtml(NullLogger.Instance));
        }
        finally
        {
            Environment.SetEnvironmentVariable("JELLYFIN_WEB_DIR", originalWebDir);
            Directory.Delete(tempDir, recursive: true);
        }
    }

    [Fact]
    public void RemoveInjectedScriptFromIndexHtml_ReturnsFalse_WhenWebDirUnset()
    {
        var originalWebDir = Environment.GetEnvironmentVariable("JELLYFIN_WEB_DIR");
        try
        {
            Environment.SetEnvironmentVariable("JELLYFIN_WEB_DIR", null);
            Assert.False(FileTransformationIntegration.RemoveInjectedScriptFromIndexHtml(NullLogger.Instance));
        }
        finally
        {
            Environment.SetEnvironmentVariable("JELLYFIN_WEB_DIR", originalWebDir);
        }
    }
}
