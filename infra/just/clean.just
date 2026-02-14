# ============================================================================
# Clean module
# ============================================================================

set working-directory := "../.."

import "common.just"

[default]
[doc('Clean all build artifacts')]
default: plugin server ft
    @rm -rf dist {{project_name}}-release.zip
    @echo -e "{{GREEN}}✓ Cleaned{{RESET}}"

[doc('Clean plugin build artifacts')]
plugin:
    #!/usr/bin/env bash
    set -uo pipefail
    echo -e "{{YELLOW}}▶ Cleaning plugin artifacts...{{RESET}}"
    rm -rf {{plugin_dir}}/dist {{plugin_dir}}/bin {{plugin_dir}}/obj 2>/dev/null \
        || (echo -e "{{YELLOW}}⚠ Some files owned by root, using sudo...{{RESET}}" \
            && sudo rm -rf {{plugin_dir}}/dist {{plugin_dir}}/bin {{plugin_dir}}/obj)
    rm -rf {{plugin_dir}}/Web/utils {{plugin_dir}}/Web/ui {{plugin_dir}}/Web/playback {{plugin_dir}}/Web/chat {{plugin_dir}}/Web/ws {{plugin_dir}}/Web/app 2>/dev/null || true
    rm -f {{plugin_dir}}/Web/*.js 2>/dev/null || true

[doc('Clean server build artifacts')]
server:
    @echo -e "{{YELLOW}}▶ Cleaning server artifacts...{{RESET}}"
    @rm -rf {{server_dir}}/target

[doc('Clean File Transformation plugin')]
ft:
    @echo -e "{{YELLOW}}▶ Cleaning File Transformation plugin...{{RESET}}"
    @rm -rf {{ft_dir}}

[doc('Remove Docker images and volumes')]
docker:
    @echo -e "{{RED}}▶ Removing Docker resources...{{RESET}}"
    @{{compose}} down -v --rmi local
    @echo -e "{{GREEN}}✓ Docker resources removed{{RESET}}"
