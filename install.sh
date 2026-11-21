#!/bin/sh
# VEYRON WM installer.
#
# Builds Veyron from this checkout and installs `veyron` and `veyronctl` for the
# current user, then writes a starter configuration if there is none.
#
#   ./install.sh                 install dependencies, build, install
#   ./install.sh --no-deps       skip the system package step
#   ./install.sh --prefix DIR    install binaries to DIR/bin (default ~/.local)
#   ./install.sh --yes           do not ask before running sudo
#   ./install.sh --uninstall     remove the installed binaries
#
# On NixOS it uses the flake instead of cargo. It never touches an existing
# configuration file.

set -eu

PREFIX="${HOME}/.local"
INSTALL_DEPS=1
ASSUME_YES=0
UNINSTALL=0
REPO="$(cd "$(dirname "$0")" && pwd)"

say() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) [ $# -ge 2 ] || die "--prefix needs a directory"; PREFIX="$2"; shift ;;
        --no-deps) INSTALL_DEPS=0 ;;
        --yes|-y) ASSUME_YES=1 ;;
        --uninstall) UNINSTALL=1 ;;
        -h|--help) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) die "unknown option '$1' (see --help)" ;;
    esac
    shift
done

BIN="${PREFIX}/bin"

if [ "$UNINSTALL" -eq 1 ]; then
    rm -f "${BIN}/veyron" "${BIN}/veyronctl"
    say "removed ${BIN}/veyron and ${BIN}/veyronctl (configuration kept)"
    exit 0
fi

run_root() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
        return
    fi
    if [ "$ASSUME_YES" -eq 0 ]; then
        printf 'About to run with sudo: %s\nContinue? [Y/n] ' "$*"
        read -r answer || answer=n
        case "$answer" in n|N|no|NO) die "cancelled" ;; esac
    fi
    sudo "$@"
}

# --- distribution -----------------------------------------------------------

DISTRO=unknown
if [ -e /etc/NIXOS ]; then
    DISTRO=nixos
elif command -v pacman >/dev/null 2>&1; then
    DISTRO=arch
elif command -v apt-get >/dev/null 2>&1; then
    DISTRO=debian
elif command -v dnf >/dev/null 2>&1; then
    DISTRO=fedora
fi
say "detected: ${DISTRO}"

if [ "$DISTRO" = nixos ]; then
    command -v nix >/dev/null 2>&1 || die "nix is not on PATH"
    say "installing with the flake (nix profile install)"
    nix --extra-experimental-features 'nix-command flakes' profile install "${REPO}#default"
    INSTALLED_BY_NIX=1
else
    INSTALLED_BY_NIX=0
    if [ "$INSTALL_DEPS" -eq 1 ]; then
        case "$DISTRO" in
            arch)
                run_root pacman -S --needed --noconfirm base-devel pkgconf git \
                    wayland libxkbcommon libglvnd libffi mesa ;;
            debian)
                run_root apt-get update
                run_root apt-get install -y build-essential pkg-config git curl \
                    libwayland-dev libxkbcommon-dev libegl-dev libffi-dev ;;
            fedora)
                run_root dnf install -y gcc pkgconf-pkg-config git \
                    wayland-devel libxkbcommon-devel mesa-libEGL-devel libffi-devel ;;
            *)
                warn "unknown distribution: install a C toolchain, pkg-config and the"
                warn "wayland, libxkbcommon, libEGL and libffi development packages yourself" ;;
        esac
    fi

    # --- Rust ------------------------------------------------------------------
    # material-colors needs Rust 1.97; distribution packages are often older.
    need_rustup=0
    if ! command -v cargo >/dev/null 2>&1; then
        need_rustup=1
    else
        version="$(rustc --version | cut -d' ' -f2)"
        minor="$(echo "$version" | cut -d. -f2)"
        if [ "${minor:-0}" -lt 97 ]; then
            if command -v rustup >/dev/null 2>&1; then
                say "updating the Rust toolchain (have ${version}, need 1.97+)"
                rustup update stable
            else
                warn "rustc ${version} is older than 1.97"
                need_rustup=1
            fi
        fi
    fi
    if [ "$need_rustup" -eq 1 ]; then
        say "installing Rust with rustup (https://rustup.rs)"
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
            | sh -s -- -y --profile minimal --default-toolchain stable
        # shellcheck disable=SC1091
        . "${HOME}/.cargo/env"
    fi

    # --- build -----------------------------------------------------------------
    # Smithay is heavy to compile: on small machines build one crate at a time
    # so the build does not run out of memory. CARGO_BUILD_JOBS overrides this.
    if [ -z "${CARGO_BUILD_JOBS:-}" ] && [ -r /proc/meminfo ]; then
        mem_kib="$(awk '/MemTotal/ {print $2}' /proc/meminfo)"
        if [ "$mem_kib" -lt 6000000 ]; then
            export CARGO_BUILD_JOBS=1
            say "less than 6 GiB of RAM: building with one job (this takes a while)"
        fi
    fi
    say "building VEYRON WM (release)"
    (cd "$REPO" && cargo build --release --locked --bin veyron --bin veyronctl)

    mkdir -p "$BIN"
    install -m755 "${REPO}/target/release/veyron" "${BIN}/veyron"
    install -m755 "${REPO}/target/release/veyronctl" "${BIN}/veyronctl"
    say "installed ${BIN}/veyron and ${BIN}/veyronctl"
    case ":${PATH}:" in
        *":${BIN}:"*) ;;
        *) warn "${BIN} is not on your PATH; add it to your shell profile" ;;
    esac
fi

# --- configuration -----------------------------------------------------------

CONFIG_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}/veyron"
CONFIG="${CONFIG_DIR}/veyron.toml"
if [ "$INSTALLED_BY_NIX" -eq 1 ]; then
    VEYRON=veyron
else
    VEYRON="${BIN}/veyron"
fi
if [ -e "$CONFIG" ]; then
    say "keeping your configuration at ${CONFIG}"
    "$VEYRON" --check-config --config "$CONFIG" || warn "your configuration has errors (see above)"
else
    mkdir -p "$CONFIG_DIR"
    "$VEYRON" --print-config > "$CONFIG"
    say "wrote a starter configuration to ${CONFIG}"
fi

cat <<EOF

VEYRON WM is installed.

  Start it inside your current desktop:   veyron
  Open a terminal inside it:              super+return
  Change the look while it runs:          super+alt+t (design), super+t (dark/light)
  Edit the configuration:                 ${CONFIG}
                                          (changes apply automatically)
  Control it from a script:               veyronctl state, veyronctl theme, ...

Bars and widgets can use the generated colours in
~/.local/state/veyron/theme/ (waybar.css, colors.css, colors.scss).
EOF
