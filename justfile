set shell := ["bash", "-euo", "pipefail", "-c"]

run *args:
    #!/usr/bin/env bash
    set -euo pipefail

    args=( {{args}} )
    debug=0
    release=0
    app_args=()

    if [[ ${#args[@]} -gt 0 ]]; then
      for arg in "${args[@]}"; do
        case "$arg" in
          --debug)
            debug=1
            ;;
          --release)
            release=1
            ;;
          *)
            app_args+=("$arg")
            ;;
        esac
      done
    fi

    if [[ "$debug" -eq 1 ]]; then
      export RUST_LOG="${RUST_LOG:-imranview::perf=debug,imranview::worker=debug,imranview::thumb=debug,imranview::state=debug,imranview::ui=debug,imranview::startup=info}"
    fi

    cmd=(cargo run)
    if [[ "$release" -eq 1 ]]; then
      cmd+=(--release)
    fi

    if [[ ${#app_args[@]} -gt 0 ]]; then
      cmd+=(-- "${app_args[@]}")
    fi

    "${cmd[@]}"

perf-gate *logs:
    #!/usr/bin/env bash
    set -euo pipefail

    logs=( {{logs}} )
    if [[ ${#logs[@]} -eq 0 ]]; then
      logs=(debug.log)
    fi

    ./scripts/perf_gate.sh "${logs[@]}"

ci:
    #!/usr/bin/env bash
    set -euo pipefail

    cargo check --all-targets
    RUST_LOG=imranview::perf=debug cargo test --all-targets -- --nocapture 2>&1 | tee perf.log
    ./scripts/perf_gate.sh perf.log

package target='':
    #!/usr/bin/env bash
    set -euo pipefail

    if [[ -n "{{target}}" ]]; then
      ./scripts/package_release.sh "{{target}}"
    else
      ./scripts/package_release.sh
    fi

release ref='' watch='0':
    #!/usr/bin/env bash
    set -euo pipefail

    if ! command -v gh >/dev/null 2>&1; then
      echo "error: gh CLI is not installed or not on PATH" >&2
      exit 1
    fi

    workflow_file="release.yml"
    target_ref="{{ref}}"
    if [[ -z "$target_ref" ]]; then
      target_ref="$(git rev-parse --abbrev-ref HEAD)"
    fi

    gh workflow run "$workflow_file" --ref "$target_ref"
    echo "Triggered Manual Release workflow on ref: $target_ref"

    if [[ "{{watch}}" == "1" ]]; then
      run_id="$(gh run list --workflow "$workflow_file" --branch "$target_ref" --event workflow_dispatch --limit 1 --json databaseId --jq '.[0].databaseId')"
      if [[ -n "$run_id" && "$run_id" != "null" ]]; then
        gh run watch "$run_id"
      else
        echo "warning: could not resolve newly-triggered run id to watch" >&2
      fi
    fi
