---
title: Contributing
parent: Development
nav_order: 2
---

# Contributing Guide

Thank you for your interest in contributing to JellyWatchParty!

## Getting Started

1. **Fork the repository** on GitHub.
2. **Clone your fork**:
   ```bash
   git clone https://github.com/YOUR-USERNAME/JellyWatchParty.git
   cd JellyWatchParty
   ```
3. **Set up the development environment** (see [Setup]({{ '/development/setup/' | relative_url }})):
   ```bash
   just setup   # git hooks, once per clone
   just up
   ```
4. **Create a branch** from `main`:
   ```bash
   git checkout main
   git checkout -b feat/your-feature-name
   ```
   Larger, multi-PR work goes through `develop` first, which publishes the
   `dev` test builds (see [Release]({{ '/development/release/' | relative_url }})).

## Code Style

### Rust

- Format with `rustfmt` and keep `clippy` clean; CI fails on either.
- Prefer descriptive names and document public items.

```bash
just fmt            # cargo fmt in src/server
just lint server    # cargo clippy -- -D warnings
just lint discord   # bot: cargo clippy --all-targets -- -D warnings
(cd src/integrations/discord-bot && cargo fmt)
```

The pre-commit hook (`.githooks/pre-commit`, enabled by `just setup`) runs
`cargo fmt --check` in `src/server` whenever `.rs` files are staged. It does
not check the bot crate, but CI does.

### C#

Follow the [.NET coding conventions](https://learn.microsoft.com/dotnet/csharp/fundamentals/coding-style/coding-conventions):
PascalCase for public members, `_camelCase` for private fields, `I` prefix for
interfaces, XML comments on public APIs. `dotnet format` is available but not
enforced by CI.

### JavaScript

- Each file is an IIFE that attaches to `window.JellyWatchParty`; don't add
  other globals.
- Prefer `const`, use meaningful names, comment non-obvious sync logic.
- New client files must be added to the explicit file lists in
  `infra/just/common.just` (`client_js_files`), `.github/workflows/ci.yml`
  and `.github/workflows/publish.yml` (two jobs); load order matters.

## Commit Messages

[Conventional Commits](https://www.conventionalcommits.org/), with a scope
where it helps:

```
type(scope): imperative summary

Optional body explaining why.

Closes #123
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `ci`,
`chore`. Examples from the history:

```
fix(docker): move runtime images to alpine 3.24 to patch openssl/zlib CVEs
ci(security): pin trivy-action and report only fixable high+ CVEs
chore(plugin): bump version to 4.1.5.0
```

Do not add AI signature lines (`Co-Authored-By: Claude` etc.).

## Pull Request Process

### Before submitting

- [ ] `just fmt`, `just lint` and `just test` pass
- [ ] Docs updated if behaviour or configuration changed
- [ ] Branch is up to date with its target branch

### Opening the PR

1. Push your branch: `git push origin feat/your-feature-name`
2. Open the PR against `main` (or `develop` for work that should ship as a
   `dev` test build first).
3. Describe the change, link related issues, say how you tested it, and add
   screenshots for UI changes.

### Review and merge

- Address feedback in new commits; avoid force-pushing during review.
- A maintainer merges approved PRs. Releases are cut from `main`; see
  [Release]({{ '/development/release/' | relative_url }}).

## Development Guidelines

### Features

1. Open an issue first for significant changes.
2. One feature per PR.
3. Update the relevant docs.
4. Add tests where the code is testable (server, bot, plugin and the client
   helpers all have suites); test manually otherwise.

### Bug fixes

1. Reproduce first.
2. Keep the fix minimal; don't refactor unrelated code.
3. Add a regression test if applicable.

## Testing

```bash
just test           # server, bot, plugin (Docker) and client suites
```

Manual checks before submitting sync-related changes:

- [ ] Create / join / leave a room
- [ ] Play, pause and seek sync
- [ ] Reconnect after a network drop
- [ ] Chrome and Firefox

See [Testing]({{ '/development/testing/' | relative_url }}) for details.

## Continuous Integration

Every PR to `develop` or `main` runs the CI and Security workflows and must
pass before merging. See [CI/CD]({{ '/development/ci/' | relative_url }}) for
the jobs and how to fix common failures.

## Code of Conduct

Be respectful and patient, keep discussions on-topic, and accept
constructive criticism. Report unacceptable behaviour to the maintainers
privately.

## Questions

Open a GitHub issue for bugs, feature requests and questions; check existing
issues first.

## License

By contributing, you agree that your contributions are licensed under the
project's [MIT License](https://github.com/TIGamingTV/JellyWatchParty/blob/main/LICENSE).
