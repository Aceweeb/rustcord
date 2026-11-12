#!/usr/bin/env bash
# Verify the claims Rustcord makes about running locally.
#
#   scripts/check-local.sh
#
# Exits non-zero if a telemetry or database dependency is linked, so the claim
# fails loudly rather than drifting. Everything else is printed for inspection.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
status=0

echo "== telemetry / analytics crates in Cargo.lock =="
if grep -Eiq '^name = "(sentry[a-z-]*|posthog[a-z-]*|mixpanel[a-z-]*|amplitude[a-z-]*|datadog[a-z-]*|newrelic[a-z-]*|opentelemetry[a-z-]*|segment[a-z-]*|bugsnag[a-z-]*|analytics[a-z-]*)"' Cargo.lock; then
    echo "FOUND — see Cargo.lock"
    status=1
else
    echo "none"
fi

echo
echo "== database crates in Cargo.lock =="
if grep -Eiq '^name = "(rusqlite|libsqlite3-sys|sqlx|sled|redb|rocksdb|duckdb|heed|lmdb)"' Cargo.lock; then
    echo "FOUND — see Cargo.lock"
    status=1
else
    echo "none"
fi

echo
echo "== hosts named in this project's source (test fixtures included) =="
grep -rhoE 'https?://[a-z0-9.-]+|wss://[a-z0-9.-]+' src/ | sed 's|.*://||' | sort -u | sed 's/^/  /'

echo
echo "== local state =="
echo "  settings: ${XDG_CONFIG_HOME:-$HOME/.config}/rustcord/settings.json"
echo "  keyring:  rustcord/discord_bot_token, rustcord/discord_oauth_identity"
echo "  messages, images, logs: memory only"

echo
echo "== binary check (release build, if present) =="
if [ -x target/release/rustcord ]; then
    if strings -a target/release/rustcord |
        grep -Eiq 'sentry\.io|posthog\.com|mixpanel\.com|amplitude\.com|segment\.io|google-analytics\.com|bugsnag\.com|datadoghq\.com'; then
        echo "a telemetry endpoint appears in the binary"
        status=1
    else
        echo "no telemetry endpoint in the binary"
    fi
else
    echo "not built yet — run cargo build --release"
fi

exit "$status"
