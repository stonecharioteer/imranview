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
