using System.Text.RegularExpressions;
using Microsoft.Extensions.Logging.Abstractions;
using Xunit;

namespace JellyWatchParty.Plugin.Tests;

[Collection(InjectionStateCollection.Name)]
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

    [Fact]
    public void TransformIndexHtml_RecordsThatFileTransformationReachedUs()
    {
        FileTransformationIntegration.ResetForTests();
        Assert.False(FileTransformationIntegration.TransformationInvoked);

        FileTransformationIntegration.TransformIndexHtml(
            MakePayload("<html><head></head><body></body></html>"));

        Assert.True(FileTransformationIntegration.TransformationInvoked);
    }

    [Fact]
    public void TransformIndexHtml_RecordsTheCall_EvenWhileUninstalling()
    {
        // The flag answers "does File Transformation reach us at all", which is
        // true regardless of what we choose to return - so an uninstall in
        // progress must not make a working File Transformation look broken.
        FileTransformationIntegration.ResetForTests();
        Plugin.InjectionEnabled = false;
        try
        {
            FileTransformationIntegration.TransformIndexHtml(
                MakePayload("<html><head></head><body></body></html>"));

            Assert.True(FileTransformationIntegration.TransformationInvoked);
        }
        finally
        {
            Plugin.InjectionEnabled = true;
            FileTransformationIntegration.ResetForTests();
        }
    }

    // -- IndexHtmlPattern (the key we register with File Transformation) --

    /// <summary>
    /// Faithful model of File Transformation's dispatch
    /// (WebFileTransformationService). Testing our pattern as a bare regex is
    /// the wrong abstraction and hid a real outage: what decides whether our
    /// callback runs is which single pipeline File Transformation *selects*,
    /// not whether our pattern happens to match.
    /// </summary>
    private static class FakeFileTransformation
    {
        private static string Normalize(string path) => path.TrimStart('/');

        /// <summary>
        /// Returns the registration key whose pipeline would run for
        /// <paramref name="rawSubpath"/>, or null when the file is served
        /// untransformed. <paramref name="rawSubpath"/> is what ASP.NET's
        /// static file middleware hands the provider: a PathString, so always
        /// leading-slashed.
        /// </summary>
        public static string? ResolvePipelineKey(IEnumerable<string> registrations, string rawSubpath)
        {
            // AddTransformation normalises the key it stores.
            var keys = registrations.Select(Normalize).ToList();

            // NeedsTransformation(subpath): exact key on the normalised path,
            // else regex against the RAW subpath.
            var needsTransformation = keys.Contains(Normalize(rawSubpath))
                || keys.Any(k => Regex.IsMatch(rawSubpath, k));
            if (!needsTransformation)
            {
                return null;
            }

            // RunTransformation(path): normalises first, then picks exactly ONE
            // pipeline - exact key match wins and the regex keys are skipped.
            var path = Normalize(rawSubpath);
            return keys.Contains(path) ? path : keys.FirstOrDefault(k => Regex.IsMatch(path, k));
        }
    }

    // The literal key every other plugin registers, as seen in the server log:
    // Media Bar, Custom Tabs and Plugin Pages all use exactly this.
    private const string EcosystemIndexHtmlKey = "index.html";

    [Fact]
    public void IndexHtmlPattern_JoinsThePipelineOtherPluginsAlreadyRegistered()
    {
        // The regression that took the button away. File Transformation keys
        // pipelines by registration string and runs only ONE of them, so a
        // pattern registered under any other spelling is silently skipped the
        // moment a plugin registers the literal "index.html".
        var selected = FakeFileTransformation.ResolvePipelineKey(
            new[] { EcosystemIndexHtmlKey, FileTransformationIntegration.IndexHtmlPattern },
            "/index.html");

        Assert.Equal(FileTransformationIntegration.IndexHtmlPattern, selected);
    }

    [Fact]
    public void IndexHtmlPattern_RunsWhenNoOtherPluginIsInstalled()
    {
        var selected = FakeFileTransformation.ResolvePipelineKey(
            new[] { FileTransformationIntegration.IndexHtmlPattern },
            "/index.html");

        Assert.Equal(FileTransformationIntegration.IndexHtmlPattern, selected);
    }

    [Theory]
    [InlineData("/main.jellyfin.bundle.js")]
    [InlineData("/runtime.bundle.js")]
    [InlineData("/userpluginsettings.html")]
    public void IndexHtmlPattern_DoesNotClaimUnrelatedFiles(string rawSubpath)
    {
        Assert.Null(FakeFileTransformation.ResolvePipelineKey(
            new[] { FileTransformationIntegration.IndexHtmlPattern }, rawSubpath));
    }

    // -- IsFileTransformationAvailable (decides whether the middleware stands down) --

    [Fact]
    public void IsFileTransformationAvailable_IsFalse_WhenPluginNotLoaded()
    {
        // No File Transformation assembly is loaded in the test process. A false
        // positive here would make the middleware stand down on servers that
        // have no File Transformation at all, leaving the script uninjected.
        Assert.False(FileTransformationIntegration.IsFileTransformationAvailable());
    }

    [Fact]
    public void IsFileTransformationAvailable_IsStable_AcrossRepeatedCalls()
    {
        // The result is memoised on success, so repeated probing must not start
        // reporting a different answer.
        var first = FileTransformationIntegration.IsFileTransformationAvailable();
        Assert.Equal(first, FileTransformationIntegration.IsFileTransformationAvailable());
        Assert.Equal(first, FileTransformationIntegration.IsFileTransformationAvailable());
    }

    // -- Injection composes with a tag already present in the physical file --

    [Fact]
    public void InjectScript_DoesNotDoubleInject_WhenPhysicalFileAlreadyPatched()
    {
        // Upgrade path: an older version wrote the tag straight into index.html.
        // File Transformation then reads that file and runs our callback over
        // it, which must not add a second copy of the script.
        var physicallyPatched = FileTransformationIntegration.InjectScript(
            "<html><head></head><body><h1>Jellyfin</h1></body></html>");

        var afterTransform = FileTransformationIntegration.TransformIndexHtml(MakePayload(physicallyPatched));

        Assert.Equal(physicallyPatched, afterTransform);
        var occurrences = afterTransform.Split("JellyWatchParty/ClientScript").Length - 1;
        Assert.Equal(1, occurrences);
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
