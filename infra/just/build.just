# ============================================================================
# Build module
# ============================================================================

set working-directory := "../.."

import "common.just"

[default]
[doc('Build the Jellyfin plugin (copies JS + dotnet publish)')]
plugin:
    @echo -e "{{GREEN}}▶ Building Jellyfin plugin...{{RESET}}"
    @mkdir -p {{plugin_dir}}/Web {{plugin_dir}}/dist {{plugin_dir}}/obj {{plugin_dir}}/bin
    @cp {{client_dir}}/plugin.js {{plugin_dir}}/Web/plugin.js
    @for f in {{client_js_files}}; do cp {{client_dir}}/$f {{plugin_dir}}/Web/; done
    @{{compose_tools}} run --rm plugin-builder
    @echo -e "{{GREEN}}✓ Plugin built: {{plugin_dir}}/dist/{{RESET}}"

[doc('Build the session server locally (Rust)')]
server:
    @echo -e "{{GREEN}}▶ Building session server...{{RESET}}"
    @cd {{server_dir}} && cargo build --release
    @echo -e "{{GREEN}}✓ Server built: {{server_dir}}/target/release/{{RESET}}"

[doc('Rebuild session server Docker image')]
image:
    @echo -e "{{GREEN}}▶ Building session server Docker image...{{RESET}}"
    @{{compose}} build session-server
    @echo -e "{{GREEN}}✓ Docker image built{{RESET}}"

[doc('Build everything (plugin + server image)')]
all: plugin image
    @echo -e "{{GREEN}}✓ All components built{{RESET}}"
