# ============================================================================
# Build
# ============================================================================

[group('build')]
[doc('Build the Jellyfin plugin')]
build: build-plugin

[group('build')]
[doc('Build the Jellyfin plugin (copies JS + dotnet publish)')]
build-plugin:
    @echo -e "{{GREEN}}▶ Building Jellyfin plugin...{{RESET}}"
    @mkdir -p {{plugin_dir}}/Web {{plugin_dir}}/dist {{plugin_dir}}/obj {{plugin_dir}}/bin
    @cp {{client_dir}}/plugin.js {{plugin_dir}}/Web/plugin.js
    @for f in {{client_js_files}}; do cp {{client_dir}}/$f {{plugin_dir}}/Web/; done
    @{{compose_tools}} run --rm plugin-builder
    @echo -e "{{GREEN}}✓ Plugin built: {{plugin_dir}}/dist/{{RESET}}"

[group('build')]
[doc('Build the session server locally (Rust)')]
build-server:
    @echo -e "{{GREEN}}▶ Building session server...{{RESET}}"
    @cd {{server_dir}} && cargo build --release
    @echo -e "{{GREEN}}✓ Server built: {{server_dir}}/target/release/{{RESET}}"

[group('build')]
[doc('Rebuild session server Docker image')]
build-server-docker:
    @echo -e "{{GREEN}}▶ Building session server Docker image...{{RESET}}"
    @{{compose}} build session-server
    @echo -e "{{GREEN}}✓ Docker image built{{RESET}}"

[group('build')]
[doc('Build everything (plugin + server image)')]
build-all: build-plugin build-server-docker
    @echo -e "{{GREEN}}✓ All components built{{RESET}}"

[group('build')]
[doc('Clean and rebuild everything')]
rebuild: clean build-all
    @{{compose}} up -d --force-recreate
    @echo -e "{{GREEN}}✓ Stack rebuilt and restarted{{RESET}}"

[group('build')]
[doc('Build release artifacts (zip)')]
release: clean
    @echo -e "{{GREEN}}▶ Building release...{{RESET}}"
    @mkdir -p dist/plugin dist/server
    @just build-plugin
    @cp -r {{plugin_dir}}/dist/* dist/plugin/
    @cd {{server_dir}} && cargo build --release
    @cp {{server_dir}}/target/release/session-server dist/server/ 2>/dev/null || true
    @cd dist && zip -r ../{{project_name}}-release.zip .
    @echo -e "{{GREEN}}✓ Release built: {{project_name}}-release.zip{{RESET}}"

[group('build')]
[doc('Build release Docker image via prod compose')]
release-image:
    @echo -e "{{GREEN}}▶ Building release Docker image...{{RESET}}"
    @{{compose_prod}} build session-server
    @echo -e "{{GREEN}}✓ Release Docker image built{{RESET}}"
