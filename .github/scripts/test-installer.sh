#!/bin/sh
# Runs inside a clean distribution container, as root.
#
#   test-installer.sh binary          install the archive in /dist, then smoke test
#   test-installer.sh user            the same as an unprivileged sudo user
#   test-installer.sh source REF      pipe the script and build REF from GitHub
#
# /repo is the checkout, /display holds the host's Wayland socket and X11 is
# reachable through DISPLAY, so the nested compositor really opens on both.

set -eu

mode=$1
ref=${2:-}

if command -v apt-get >/dev/null 2>&1; then
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get install -y --no-install-recommends ca-certificates curl tar sudo
elif command -v pacman >/dev/null 2>&1; then
    pacman -Syu --noconfirm --needed curl tar sudo
elif command -v dnf >/dev/null 2>&1; then
    dnf install -y curl tar gzip sudo
elif command -v zypper >/dev/null 2>&1; then
    zypper --non-interactive refresh
    zypper --non-interactive install --no-recommends curl tar gzip sudo gawk
fi

as_user=''
home=$HOME
if [ "$mode" = user ]; then
    useradd --create-home tester
    echo 'tester ALL=(ALL) NOPASSWD: ALL' >/etc/sudoers.d/tester
    as_user='sudo -u tester -H'
    home=/home/tester
fi

# Piped, like curl | sh.
installer() {
    # shellcheck disable=SC2086 # as_user may be empty
    cat /repo/install.sh | $as_user sh -s -- "$@"
}

echo "== without a terminal and without --yes nothing may change"
if installer --from-dir /dist </dev/null; then
    echo "the installer went ahead without confirmation" >&2
    exit 1
fi
test ! -e "$home/.local/bin/veyron"

echo "== install"
if [ "$mode" = source ]; then
    installer --yes --ref "$ref"
else
    installer --yes --from-dir /dist
fi
veyron="$home/.local/bin/veyron"
ctl="$home/.local/bin/veyronctl"
"$veyron" --version
"$veyron" --check-config --config "$home/.config/veyron/veyron.toml"

# smoke NAME VAR=VALUE: start the nested compositor on one kind of host.
smoke() {
    name=$1
    shift
    runtime=$(mktemp -d)
    chmod 700 "$runtime"
    env -u DISPLAY -u WAYLAND_DISPLAY "$@" XDG_RUNTIME_DIR="$runtime" \
        "$veyron" --socket "$runtime/control.sock" >"$runtime/log" 2>&1 &
    pid=$!
    tries=0
    until "$ctl" --socket "$runtime/control.sock" ping >/dev/null 2>&1; do
        tries=$((tries + 1))
        if [ "$tries" -gt 100 ] || ! kill -0 "$pid" 2>/dev/null; then
            echo "nested start on $name failed" >&2
            cat "$runtime/log" >&2
            exit 1
        fi
        sleep 0.2
    done
    "$ctl" --socket "$runtime/control.sock" quit
    wait "$pid"
    echo "nested start on $name: ok"
}

echo "== nested compositor"
smoke X11 DISPLAY="$DISPLAY"
smoke Wayland WAYLAND_DISPLAY=/display/veyron-host

if [ "$mode" != source ]; then
    echo "== update keeps the configuration"
    echo '# edited by the test' >>"$home/.config/veyron/veyron.toml"
    installer --yes --from-dir /dist
    grep -q 'edited by the test' "$home/.config/veyron/veyron.toml"

    echo "== a tampered archive is refused and the installed copy survives"
    bad=$(mktemp -d)
    cp /dist/* "$bad/"
    for archive in "$bad"/*.tar.gz; do echo tampered >>"$archive"; done
    chmod -R a+rX "$bad"
    before=$(sha256sum "$veyron")
    if installer --yes --from-dir "$bad"; then
        echo "a tampered archive was installed" >&2
        exit 1
    fi
    test "$before" = "$(sha256sum "$veyron")"
fi

echo "== uninstall"
installer --yes --uninstall
test ! -e "$veyron"
test -e "$home/.config/veyron/veyron.toml"
