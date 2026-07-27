using Xunit;

namespace JellyWatchParty.Plugin.Tests;

/// <summary>
/// Serialises the test classes that mutate process-wide injection state
/// (<see cref="Plugin.InjectionEnabled"/>, the middleware's cached index.html
/// and its File Transformation probe). xunit runs test classes in parallel by
/// default, which would otherwise let these overwrite each other's setup.
/// </summary>
[CollectionDefinition(Name, DisableParallelization = true)]
public class InjectionStateCollection
{
    public const string Name = "InjectionState";
}
