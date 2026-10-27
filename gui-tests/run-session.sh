#!/usr/bin/env bash
# Inner half of run.sh: one suite inside a private D-Bus session.
#
# Usage: run-session.sh <test-binary> [cargo test args…]
set -euo pipefail

DIR=$(cd "$(dirname "$0")" && pwd)
suite=${1:?usage: run-session.sh <test-binary> [cargo test args…]}
shift

launcher_pid=""
registry_pid=""
cleanup() {
    [ -n "$registry_pid" ] && kill "$registry_pid" 2>/dev/null || true
    [ -n "$launcher_pid" ] && kill "$launcher_pid" 2>/dev/null || true
}
trap cleanup EXIT

get_a11y_address() {
    gdbus call --session \
        --dest org.a11y.Bus \
        --object-path /org/a11y/bus \
        --method org.a11y.Bus.GetAddress 2>/dev/null |
        sed -n "s/^('\(.*\)',)$/\1/p"
}

# Fedora's SELinux policy blocks D-Bus activation of the accessibility
# launcher inside a nested session. Start the launcher and registry explicitly;
# on systems where activation works this block is skipped.
if [ -z "$(get_a11y_address)" ]; then
    /usr/libexec/at-spi-bus-launcher --launch-immediately >/dev/null 2>&1 &
    launcher_pid=$!
    for _ in $(seq 1 50); do
        if [ -n "$(get_a11y_address)" ]; then
            break
        fi
        sleep 0.1
    done
    if [ -z "$(get_a11y_address)" ]; then
        echo "error: could not start the AT-SPI accessibility bus" >&2
        exit 1
    fi
    /usr/libexec/at-spi2-registryd --use-gnome-session >/dev/null 2>&1 &
    registry_pid=$!
    sleep 0.5
fi

cd "$DIR"
exec cargo test --test "$suite" "$@" -- --test-threads=1
