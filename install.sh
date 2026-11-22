#!/bin/sh
# VEYRON WM installer.
#
#   curl -fsSL https://raw.githubusercontent.com/pabl0conepc-alt/Veyron-WM/main/install.sh | sh
#   curl -fsSL .../install.sh | sh -s -- [options]
#   ./install.sh [options]       from a checkout: builds that checkout
#
# It detects the system, shows what it is going to do and changes nothing until
# you confirm. NixOS gets the flake; other systems get the release binary for
# their CPU, checked against SHA256SUMS, or a source build when none fits.
#
# The whole script is functions, called on the last line, so a download that
# was cut short cannot run half an installation. Questions are read from
# /dev/tty because stdin is the script itself under `curl | sh`.

# Functions run through `step` and `trap` look unused to shellcheck.
# shellcheck disable=SC2317,SC2329
set -eu

REPO=pabl0conepc-alt/Veyron-WM
MIN_RUST_MINOR=97     # material-colors
MIN_GLIBC_MINOR=35    # release binaries are built on Ubuntu 22.04
LOW_MEMORY_KIB=6000000

usage() {
    cat <<EOF
VEYRON WM installer

  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install.sh | sh -s -- [options]

  -y, --yes           do not ask (required when there is no terminal)
      --dry-run       show what would be done and change nothing
      --from-source   build instead of downloading a binary
      --release TAG   install that release instead of the latest
      --ref REF       build that branch, tag or commit
      --from-dir DIR  take the archive and SHA256SUMS from a local directory
      --prefix DIR    install to DIR/bin (default ~/.local)
      --no-deps       do not install system packages
      --uninstall     remove Veyron (the configuration is kept)
  -h, --help          show this help
EOF
}

# --- terminal ----------------------------------------------------------------

# t ENGLISH PORTUGUESE
t() {
    if [ "$UI_LANG" = pt ]; then printf '%s' "$2"; else printf '%s' "$1"; fi
}

setup_terminal() {
    ESC=$(printf '\033')
    NL='
'
    locale=${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}
    case $locale in pt*) UI_LANG=pt ;; *) UI_LANG=en ;; esac

    HAS_TTY=0
    if (: </dev/tty) 2>/dev/null; then HAS_TTY=1; fi
    FANCY=0
    if [ -t 1 ] && [ "${TERM:-dumb}" != dumb ]; then FANCY=1; fi

    UNICODE=0
    case ${LC_ALL:-${LC_CTYPE:-${LANG:-}}} in
        *UTF-8*|*utf-8*|*UTF8*|*utf8*) UNICODE=1 ;;
    esac

    BOLD='' DIM='' RESET='' ACCENT='' GOOD='' BAD='' WARN=''
    C1='' C2='' C3='' C4='' C5='' C6=''
    if [ "$FANCY" -eq 1 ] && [ -z "${NO_COLOR:-}" ]; then
        BOLD="${ESC}[1m" DIM="${ESC}[2m" RESET="${ESC}[0m"
        ACCENT="${ESC}[1;36m" GOOD="${ESC}[1;32m" BAD="${ESC}[1;31m" WARN="${ESC}[1;33m"
        case ${COLORTERM:-} in
            truecolor|24bit)
                C1="${ESC}[38;2;0;229;255m"   C2="${ESC}[38;2;45;184;255m"
                C3="${ESC}[38;2;98;135;255m"  C4="${ESC}[38;2;151;92;255m"
                C5="${ESC}[38;2;206;66;236m"  C6="${ESC}[38;2;255;61;154m" ;;
            *)
                C1="${ESC}[38;5;51m"  C2="${ESC}[38;5;39m"  C3="${ESC}[38;5;63m"
                C4="${ESC}[38;5;99m"  C5="${ESC}[38;5;165m" C6="${ESC}[38;5;198m" ;;
        esac
    fi

    if [ "$UNICODE" -eq 1 ]; then
        OK='✔' NO='✘' POINT='❯' RAIL='│' TOP='╭─' END='╰─'
    else
        OK='+' NO='x' POINT='>' RAIL='|' TOP='+-' END='+-'
    fi

    COLS=80
    if [ "$HAS_TTY" -eq 1 ]; then
        COLS=$(stty size </dev/tty 2>/dev/null | cut -d' ' -f2) || COLS=80
        case $COLS in ''|0|*[!0-9]*) COLS=80 ;; esac
    fi
}

banner() {
    if [ "$COLS" -lt 58 ]; then
        set -- '' '' \
            '__   _______   _____  ___  _  _ ' \
            '\ \ / / __\ \ / / _ \/ _ \| \| |' \
            ' \ V /| _| \ V /|   / (_) | .` |' \
            '  \_/ |___| |_| |_|_\\___/|_|\_|'
    elif [ "$UNICODE" -eq 1 ]; then
        set -- \
            '██╗   ██╗███████╗██╗   ██╗██████╗  ██████╗ ███╗   ██╗' \
            '██║   ██║██╔════╝╚██╗ ██╔╝██╔══██╗██╔═══██╗████╗  ██║' \
            '██║   ██║█████╗   ╚████╔╝ ██████╔╝██║   ██║██╔██╗ ██║' \
            '╚██╗ ██╔╝██╔══╝    ╚██╔╝  ██╔══██╗██║   ██║██║╚██╗██║' \
            ' ╚████╔╝ ███████╗   ██║   ██║  ██║╚██████╔╝██║ ╚████║' \
            '  ╚═══╝  ╚══════╝   ╚═╝   ╚═╝  ╚═╝ ╚═════╝ ╚═╝  ╚═══╝'
    else
        set -- \
            '__     __ _____ __   __ ____    ___   _   _ ' \
            '\ \   / /| ____|\ \ / /|  _ \  / _ \ | \ | |' \
            ' \ \ / / |  _|   \ V / | |_) || | | ||  \| |' \
            '  \ V /  | |___   | |  |  _ < | |_| || |\  |' \
            '   \_/   |_____|  |_|  |_| \_\ \___/ |_| \_|' \
            ''
    fi

    printf '\n'
    row=0
    for line in "$@"; do
        row=$((row + 1))
        [ -n "$line" ] || continue
        case $row in 1) c=$C1 ;; 2) c=$C2 ;; 3) c=$C3 ;; 4) c=$C4 ;; 5) c=$C5 ;; *) c=$C6 ;; esac
        printf '  %s%s%s\n' "$c" "$line" "$RESET"
        if [ "$FANCY" -eq 1 ]; then sleep 0.06; fi
    done
    printf '\n  %s%s%s\n\n' "$BOLD" "$(t 'scrollable tiling Wayland compositor' 'compositor Wayland em Rust')" "$RESET"
}

section() { printf '  %s%s %s%s\n' "$ACCENT" "$TOP" "$1" "$RESET"; }
section_end() { printf '  %s%s%s\n\n' "$ACCENT" "$END" "$RESET"; }
field() {
    # printf pads by bytes: count characters, not UTF-8 continuation bytes.
    chars=$(printf %s "$1" | LC_ALL=C tr -d '\200-\277' | wc -c)
    pad=$((11 - chars))
    [ "$pad" -ge 0 ] || pad=0
    printf '  %s%s%s %s%*s %s\n' "$ACCENT" "$RAIL" "$RESET" "$1" "$pad" '' "$2"
}
done_step() { printf '  %s%s%s %s\n' "$GOOD" "$OK" "$RESET" "$1"; }
warn() { printf '  %s!%s %s\n' "$WARN" "$RESET" "$1" >&2; }
die() {
    printf '\n  %s%s %s%s\n' "$BAD" "$NO" "$1" "$RESET" >&2
    exit 1
}

kill_tree() {
    if command -v pgrep >/dev/null 2>&1; then
        for child in $(pgrep -P "$1" || true); do kill_tree "$child"; done
    fi
    kill "$1" 2>/dev/null || true
}

cleanup() {
    status=$?
    trap - EXIT INT TERM HUP
    if [ -n "$JOB" ]; then kill_tree "$JOB"; fi
    if [ -n "$SAVED_STTY" ]; then stty "$SAVED_STTY" </dev/tty 2>/dev/null || true; fi
    if [ "$FANCY" -eq 1 ]; then printf '%s[?25h%s' "$ESC" "$RESET"; fi
    if [ -n "$WORK" ]; then rm -rf "$WORK"; fi
    exit "$status"
}

# read_key: sets KEY to up, down, enter, quit, a digit or nothing.
read_key() {
    stty -echo -icanon min 1 time 0 </dev/tty
    # The trailing dot keeps Enter (a newline) distinct from end of input.
    byte=$(dd bs=1 count=1 </dev/tty 2>/dev/null; printf .)
    byte=${byte%.}
    case $byte in
        "$NL") KEY=enter ;;
        ''|q|Q) KEY=quit ;;
        k) KEY=up ;;
        j) KEY=down ;;
        [0-9]) KEY=$byte ;;
        "$ESC")
            stty min 0 time 1 </dev/tty
            rest=$(dd bs=1 count=1 </dev/tty 2>/dev/null || true)
            rest=$rest$(dd bs=1 count=1 </dev/tty 2>/dev/null || true)
            case $rest in '[A'|OA) KEY=up ;; '[B'|OB) KEY=down ;; '') KEY=quit ;; *) KEY='' ;; esac ;;
        *) KEY='' ;;
    esac
}

# choose ITEM...: sets CHOICE to the chosen position, 0 to cancel.
choose() {
    count=$# selected=1 drawn=0 CHOICE=''
    if [ "$FANCY" -eq 0 ]; then
        n=0
        for item in "$@"; do n=$((n + 1)); printf '    %s) %s\n' "$n" "$item"; done
        printf '  [1-%s]: ' "$count"
        IFS= read -r answer </dev/tty || answer=''
        case $answer in
            [1-9]) if [ "$answer" -le "$count" ]; then CHOICE=$answer; else CHOICE=0; fi ;;
            *) CHOICE=0 ;;
        esac
        return 0
    fi

    SAVED_STTY=$(stty -g </dev/tty)
    printf '%s[?25l' "$ESC"
    while [ -z "$CHOICE" ]; do
        if [ "$drawn" -eq 1 ]; then printf '%s[%sA' "$ESC" "$((count + 1))"; fi
        n=0
        for item in "$@"; do
            n=$((n + 1))
            if [ "$n" -eq "$selected" ]; then
                printf '\r%s[2K  %s%s %s%s\n' "$ESC" "$ACCENT" "$POINT" "$item" "$RESET"
            else
                printf '\r%s[2K    %s\n' "$ESC" "$item"
            fi
        done
        printf '\r%s[2K  %s%s%s\n' "$ESC" "$DIM" "$(t 'arrows move, Enter selects, q quits' 'setas movem, Enter escolhe, q sai')" "$RESET"
        drawn=1
        read_key
        case $KEY in
            up) selected=$((selected - 1)); [ "$selected" -ge 1 ] || selected=$count ;;
            down) selected=$((selected + 1)); [ "$selected" -le "$count" ] || selected=1 ;;
            enter) CHOICE=$selected ;;
            quit) CHOICE=0 ;;
            [1-9]) [ "$KEY" -gt "$count" ] || CHOICE=$KEY ;;
        esac
    done
    stty "$SAVED_STTY" </dev/tty
    SAVED_STTY=''
    printf '%s[?25h\n' "$ESC"
}

spin_frame() {
    if [ "$UNICODE" -eq 1 ]; then
        case $(($1 % 10)) in
            0) FRAME='⠋' ;; 1) FRAME='⠙' ;; 2) FRAME='⠹' ;; 3) FRAME='⠸' ;; 4) FRAME='⠼' ;;
            5) FRAME='⠴' ;; 6) FRAME='⠦' ;; 7) FRAME='⠧' ;; 8) FRAME='⠇' ;; *) FRAME='⠏' ;;
        esac
    else
        case $(($1 % 4)) in 0) FRAME='|' ;; 1) FRAME='/' ;; 2) FRAME='-' ;; *) FRAME="\\" ;; esac
    fi
}

# step LABEL COMMAND...: run it with a spinner, show its log only on failure.
step() {
    label=$1
    shift
    log="$WORK/step.log"
    : >"$log"
    began=$(date +%s) tick=0
    "$@" </dev/null >"$log" 2>&1 &
    JOB=$!
    if [ "$FANCY" -eq 1 ]; then
        printf '%s[?25l' "$ESC"
        while kill -0 "$JOB" 2>/dev/null; do
            spin_frame "$tick"
            detail=''
            if [ "$BUILD_TOTAL" -gt 0 ]; then
                built=$(grep -c '^ *Compiling ' "$log" || true)
                # The lockfile also lists crates for other systems: an estimate.
                percent=$((built * 100 / BUILD_TOTAL))
                [ "$percent" -lt 99 ] || percent=99
                bar='' cell=0
                while [ "$cell" -lt 12 ]; do
                    if [ $((cell * 100 / 12)) -lt "$percent" ]; then bar="$bar#"; else bar="$bar."; fi
                    cell=$((cell + 1))
                done
                detail="~${percent}%"
                # A wrapped line cannot be redrawn in place.
                [ "$COLS" -lt 60 ] || detail="[$bar] ~${percent}%"
            fi
            printf '\r%s[2K  %s%s%s %s %s %s%ss%s' "$ESC" "$ACCENT" "$FRAME" "$RESET" \
                "$label" "$detail" "$DIM" "$(($(date +%s) - began))" "$RESET"
            tick=$((tick + 1))
            sleep 0.12
        done
        printf '\r%s[2K%s[?25h' "$ESC" "$ESC"
    else
        printf '  ... %s\n' "$label"
    fi
    result=0
    wait "$JOB" || result=$?
    JOB=''
    BUILD_TOTAL=0
    if [ "$result" -ne 0 ]; then
        printf '  %s%s%s %s\n\n' "$BAD" "$NO" "$RESET" "$label" >&2
        tail -n 25 "$log" >&2
        if mkdir -p "$CACHE" 2>/dev/null && cp "$log" "$CACHE/install.log" 2>/dev/null; then
            printf '\n  log: %s\n' "$CACHE/install.log" >&2
        fi
        die "$(t 'installation stopped' 'instalação interrompida')"
    fi
    done_step "$label ($(($(date +%s) - began))s)"
}

# --- detection ---------------------------------------------------------------

os_value() {
    [ -r /etc/os-release ] || return 0
    sed -n "s/^$1=//p" /etc/os-release | head -n 1 | tr -d "\"'" | tr -d '[:cntrl:]'
}

detect_system() {
    [ "$(uname -s)" = Linux ] || die "$(t 'Veyron only runs on Linux' 'o Veyron só roda em Linux')"

    OS_NAME=$(os_value PRETTY_NAME)
    OS_NAME=${OS_NAME:-Linux}
    FAMILY=unknown
    # The first match wins, so ID takes priority over ID_LIKE.
    for id in $(os_value ID) $(os_value ID_LIKE); do
        case $id in
            nixos) FAMILY=nixos ;;
            arch|archarm|manjaro|endeavouros|cachyos) FAMILY=arch ;;
            debian|ubuntu) FAMILY=debian ;;
            fedora) FAMILY=fedora ;;
            opensuse*|suse) FAMILY=suse ;;
            alpine) FAMILY=musl ;;
            *) continue ;;
        esac
        break
    done
    # An image-based system has no usable package manager for this.
    if [ -e /run/ostree-booted ]; then FAMILY=unknown; fi

    CPU=$(uname -m)
    case $CPU in arm64) CPU=aarch64 ;; amd64) CPU=x86_64 ;; esac

    GLIBC=$(getconf GNU_LIBC_VERSION 2>/dev/null | cut -d' ' -f2) || GLIBC=''
    MEM_KIB=$(awk '/^MemTotal:/ {print $2}' /proc/meminfo 2>/dev/null) || MEM_KIB=''
    MEM_KIB=${MEM_KIB:-0}

    if [ -n "${WAYLAND_DISPLAY:-}" ]; then
        SESSION=Wayland
    elif [ -n "${DISPLAY:-}" ]; then
        SESSION=X11
    else
        SESSION=$(t 'no graphical session' 'sem sessão gráfica')
    fi

    # Under curl | sh, $0 is the shell, so this is empty.
    CHECKOUT=''
    case $0 in
        *install.sh)
            if [ -f "$0" ]; then
                here=$(cd "$(dirname "$0")" && pwd)
                if [ -f "$here/Cargo.toml" ] && [ -d "$here/crates/veyron" ]; then CHECKOUT=$here; fi
            fi ;;
    esac
}

fetch() {
    curl --proto '=https' --tlsv1.2 -fsSL --retry 3 --connect-timeout 15 -o "$2" "$1"
}

choose_method() {
    ASSET="veyron-wm-linux-${CPU}.tar.gz"
    METHOD=source WHY=''
    if [ "$FAMILY" = nixos ]; then
        [ "$CPU" = x86_64 ] || die "$(t 'the Nix flake only supports x86_64' 'o flake Nix só suporta x86_64')"
        command -v nix >/dev/null 2>&1 || die "$(t 'nix is not on PATH' 'o nix não está no PATH')"
        METHOD=nix
        return 0
    fi
    if [ "$FAMILY" = musl ]; then
        die "$(t 'musl systems such as Alpine are not supported yet' 'sistemas musl, como Alpine, ainda não são suportados')"
    fi

    if [ "$FORCE_SOURCE" -eq 1 ]; then
        WHY=$(t 'you asked for it' 'você pediu')
        return 0
    fi
    if [ -n "$CHECKOUT" ] && [ -z "$FROM_DIR" ] && [ -z "$RELEASE" ]; then
        WHY=$(t 'running from a checkout' 'rodando de um clone')
        return 0
    fi
    case $CPU in
        x86_64|aarch64) ;;
        *) WHY=$(t 'no binary for this CPU' 'não há binário para esta CPU'); return 0 ;;
    esac
    glibc_major=${GLIBC%%.*}
    glibc_minor=${GLIBC#*.}
    glibc_minor=${glibc_minor%%.*}
    if [ "$glibc_major" != 2 ] || [ "${glibc_minor:-0}" -lt "$MIN_GLIBC_MINOR" ]; then
        WHY=$(t "binaries need glibc 2.$MIN_GLIBC_MINOR" "os binários exigem glibc 2.$MIN_GLIBC_MINOR")
        return 0
    fi

    if [ -n "$FROM_DIR" ]; then
        [ -f "$FROM_DIR/$ASSET" ] && [ -f "$FROM_DIR/SHA256SUMS" ] ||
            die "$(t "$FROM_DIR needs $ASSET and SHA256SUMS" "$FROM_DIR precisa de $ASSET e SHA256SUMS")"
        cp "$FROM_DIR/SHA256SUMS" "$WORK/SHA256SUMS"
        METHOD=binary
        return 0
    fi

    command -v curl >/dev/null 2>&1 || die "$(t 'curl is required' 'o curl é necessário')"
    if [ -n "$RELEASE" ]; then
        BASE="https://github.com/$REPO/releases/download/$RELEASE"
    else
        BASE="https://github.com/$REPO/releases/latest/download"
    fi
    code=$(curl --proto '=https' --tlsv1.2 -sSL --connect-timeout 15 \
        -o "$WORK/SHA256SUMS" -w '%{http_code}' "$BASE/SHA256SUMS") ||
        die "$(t 'could not reach GitHub' 'não consegui acessar o GitHub')"
    case $code in
        200) ;;
        404)
            [ -z "$RELEASE" ] || die "$(t "release $RELEASE has no binaries" "a versão $RELEASE não tem binários")"
            WHY=$(t 'no release published yet' 'nenhuma versão publicada ainda')
            return 0 ;;
        *) die "GitHub: HTTP $code" ;;
    esac
    if grep -q " [* ]$ASSET\$" "$WORK/SHA256SUMS"; then
        METHOD=binary
    else
        WHY=$(t 'the release has no binary for this CPU' 'a versão não tem binário para esta CPU')
    fi
}

package_installed() {
    case $FAMILY in
        debian) dpkg-query -W -f='${Status}' "$1" 2>/dev/null | grep -q 'ok installed' ;;
        arch) pacman -T "$1" >/dev/null 2>&1 ;;
        fedora|suse) rpm -q --whatprovides "$1" >/dev/null 2>&1 ;;
    esac
}

# Several libraries are opened with dlopen, so ldd does not show them:
# wayland-egl for Wayland hosts, the X11 ones and xkbcommon-x11 for X11 hosts.
plan_packages() {
    run='' dev='' MISSING=''
    case $FAMILY in
        debian)
            run='libwayland-client0 libwayland-server0 libwayland-egl1 libxkbcommon0 libxkbcommon-x11-0 xkb-data libegl1 libegl-mesa0 libgl1-mesa-dri libx11-6 libx11-xcb1 libxcursor1 libxi6'
            dev='build-essential pkg-config ca-certificates libwayland-dev libxkbcommon-dev libegl-dev libffi-dev' ;;
        arch)
            run='wayland libxkbcommon libxkbcommon-x11 xkeyboard-config libglvnd mesa libx11 libxcursor libxi'
            dev='gcc make pkgconf libffi' ;;
        fedora)
            run='libwayland-client libwayland-server libwayland-egl libxkbcommon libxkbcommon-x11 xkeyboard-config libglvnd-egl mesa-libEGL mesa-dri-drivers libX11 libX11-xcb libXcursor libXi'
            dev='gcc make pkgconf-pkg-config wayland-devel libxkbcommon-devel libffi-devel' ;;
        suse)
            run='libwayland-client0 libwayland-server0 libwayland-egl1 libxkbcommon0 libxkbcommon-x11-0 xkeyboard-config libglvnd Mesa-libEGL1 Mesa-dri libX11-6 libX11-xcb1 libXcursor1 libXi6'
            dev='gcc make pkg-config wayland-devel libxkbcommon-devel libffi-devel' ;;
    esac
    [ "$INSTALL_DEPS" -eq 1 ] || return 0
    [ "$METHOD" = source ] || dev=''
    for package in $run $dev; do
        package_installed "$package" || MISSING="$MISSING $package"
    done
    MISSING=${MISSING# }
}

rust_is_recent() {
    command -v cargo >/dev/null 2>&1 || return 1
    version=$(RUSTUP_TOOLCHAIN=stable rustc --version 2>/dev/null | cut -d' ' -f2) || return 1
    [ -n "$version" ] || return 1
    major=${version%%.*}
    minor=${version#*.}
    minor=${minor%%.*}
    [ "$major" -gt 1 ] || [ "$minor" -ge "$MIN_RUST_MINOR" ]
}

plan_rust() {
    RUST_SETUP=''
    [ "$METHOD" = source ] || return 0
    if [ -d "$HOME/.cargo/bin" ]; then PATH="$HOME/.cargo/bin:$PATH"; fi
    rust_is_recent && return 0
    if command -v rustup >/dev/null 2>&1; then RUST_SETUP=update; else RUST_SETUP=install; fi
}

make_plan() {
    plan_packages
    plan_rust
    JOBS=${CARGO_BUILD_JOBS:-}
    if [ -z "$JOBS" ] && [ "$MEM_KIB" -gt 0 ] && [ "$MEM_KIB" -lt "$LOW_MEMORY_KIB" ]; then JOBS=1; fi
}

# Paths under the home directory read better, and fit a phone, with ~.
tilde() {
    case $1 in
        "$HOME"/*) printf '~%s' "${1#"$HOME"}" ;;
        *) printf '%s' "$1" ;;
    esac
}

show_plan() {
    section "$(t 'Your system' 'Seu sistema')"
    field "$(t System Sistema)" "$OS_NAME"
    field CPU "$CPU${GLIBC:+, glibc $GLIBC}"
    field RAM "$((MEM_KIB / 1024)) MiB"
    field "$(t Session 'Sessão')" "$SESSION"
    section_end

    section "$(t 'What will happen' 'O que vai acontecer')"
    case $METHOD in
        nix)
            field "$(t Method 'Método')" "Nix flake (nix profile)"
            field "$(t Source Origem)" "$FLAKE" ;;
        binary)
            field "$(t Method 'Método')" "$(t 'download the ready binary' 'baixar o binário pronto')"
            field "$(t File Arquivo)" "$ASSET" ;;
        source)
            field "$(t Method 'Método')" "$(t 'build from source' 'compilar do código')"
            field "$(t Reason Motivo)" "$WHY"
            field "$(t Source Origem)" "${CHECKOUT:-github.com/$REPO @ $REF}" ;;
    esac
    if [ "$METHOD" != nix ]; then
        field "$(t Into 'Destino')" "$(tilde "$BIN")"
        if [ -n "$MISSING" ]; then
            n=0
            for package in $MISSING; do n=$((n + 1)); done
            field "$(t Packages Pacotes)" "$n $(t '(asks for your password)' '(vai pedir sua senha)')"
        fi
    fi
    case $RUST_SETUP in
        install) field Rust "$(t 'install with rustup' 'instalar com rustup') (rustup.rs)" ;;
        update) field Rust "$(t 'update stable' 'atualizar o stable')" ;;
    esac
    if [ "$METHOD" != binary ]; then
        field "$(t Time Tempo)" "$(t 'several minutes, longer on weak CPUs' 'vários minutos, mais em CPU fraca')"
    fi
    if [ -e "$CONFIG" ]; then
        field Config "$(t 'kept as it is' 'mantida como está')"
    else
        field Config "$(t 'create' 'criar') $(tilde "$CONFIG")"
    fi
    section_end

    if [ "$FAMILY" = unknown ]; then
        warn "$(t 'unknown system: install the Wayland, xkbcommon, EGL and X11 libraries yourself' 'sistema desconhecido: instale você as bibliotecas Wayland, xkbcommon, EGL e X11')"
    fi
    warn "$(t 'Veyron opens as a window inside your desktop; it is not a login session yet' 'o Veyron abre numa janela dentro do seu desktop; ainda não é uma sessão de login')"
    printf '\n'
}

show_details() {
    section "$(t Details Detalhes)"
    case $METHOD in
        nix) field nix "nix profile install $FLAKE" ;;
        binary)
            field "$(t From De)" "${FROM_DIR:-$BASE}"
            field sha256 "$(t 'checked before installing' 'conferido antes de instalar')" ;;
        source)
            field cargo "build --release --locked${JOBS:+ -j$JOBS}"
            [ -z "$RUST_SETUP" ] || field rustup "https://sh.rustup.rs" ;;
    esac
    for package in $MISSING; do field "$(t package pacote)" "$package"; done
    section_end
}

confirm() {
    if [ "$ASSUME_YES" -eq 1 ]; then return 0; fi
    [ "$HAS_TTY" -eq 1 ] || die "$(t 'no terminal to ask: run again with --yes' 'sem terminal para perguntar: rode de novo com --yes')"
    while :; do
        install=$(t 'Install now' 'Instalar agora')
        details=$(t 'Show every detail' 'Ver todos os detalhes')
        cancel=$(t 'Cancel' 'Cancelar')
        if [ "$METHOD" = binary ]; then
            choose "$install" "$details" "$(t 'Build from source instead' 'Compilar do código')" "$cancel"
            case $CHOICE in
                1) return 0 ;;
                2) show_details ;;
                3)
                    METHOD=source WHY=$(t 'you asked for it' 'você pediu')
                    make_plan
                    show_plan ;;
                *) cancelled ;;
            esac
        else
            choose "$install" "$details" "$cancel"
            case $CHOICE in 1) return 0 ;; 2) show_details ;; *) cancelled ;; esac
        fi
    done
}

cancelled() {
    printf '  %s\n' "$(t 'Cancelled, nothing was changed.' 'Cancelado, nada foi alterado.')"
    exit 0
}

# --- installation ------------------------------------------------------------

get_root() {
    ROOT='' QUIET_ROOT=1
    [ "$(id -u)" -ne 0 ] || return 0
    if command -v sudo >/dev/null 2>&1; then
        ROOT=sudo
        if sudo -n true 2>/dev/null; then return 0; fi
        [ "$HAS_TTY" -eq 1 ] || die "$(t 'sudo needs a terminal for the password' 'o sudo precisa de um terminal para a senha')"
        # shellcheck disable=SC2024 # the tty is for the password prompt
        sudo -v </dev/tty || die "$(t 'sudo was refused' 'o sudo foi recusado')"
        # Without cached credentials the prompt would hide behind the spinner.
        sudo -n true 2>/dev/null || QUIET_ROOT=0
    elif command -v doas >/dev/null 2>&1; then
        ROOT=doas QUIET_ROOT=0
    else
        die "$(t 'sudo or doas is needed, or use --no-deps' 'precisa de sudo ou doas, ou use --no-deps')"
    fi
}

install_packages() {
    # shellcheck disable=SC2086 # ROOT may be empty and MISSING is a list
    case $FAMILY in
        debian) $ROOT apt-get update && $ROOT env DEBIAN_FRONTEND=noninteractive apt-get install -y $MISSING ;;
        # No -y: refreshing without upgrading is a partial upgrade on Arch.
        arch)
            $ROOT pacman -S --needed --noconfirm $MISSING || {
                echo "If packages were not found, update first: sudo pacman -Syu"
                return 1
            } ;;
        fedora) $ROOT dnf install -y $MISSING ;;
        suse) $ROOT zypper --non-interactive install --no-recommends $MISSING ;;
    esac
}

dependencies() {
    [ -n "$MISSING" ] || return 0
    get_root
    label=$(t 'System packages' 'Pacotes do sistema')
    if [ "$QUIET_ROOT" -eq 1 ]; then
        step "$label" install_packages
    else
        install_packages </dev/tty || die "$label"
        done_step "$label"
    fi
}

install_rust() {
    fetch https://sh.rustup.rs "$WORK/rustup.sh" || return 1
    RUSTUP_INIT_SKIP_PATH_CHECK=yes sh "$WORK/rustup.sh" -y --no-modify-path \
        --profile minimal --default-toolchain stable
}

setup_rust() {
    case $RUST_SETUP in
        install)
            step "$(t 'Installing Rust' 'Instalando o Rust')" install_rust
            PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH" ;;
        update) step "$(t 'Updating Rust' 'Atualizando o Rust')" rustup update stable ;;
    esac
    rust_is_recent || die "$(t "Rust 1.$MIN_RUST_MINOR or newer is needed" "precisa do Rust 1.$MIN_RUST_MINOR ou mais novo")"
}

download_source() {
    fetch "https://github.com/$REPO/archive/$REF.tar.gz" "$WORK/source.tar.gz" || return 1
    mkdir "$WORK/source" || return 1
    tar -xzf "$WORK/source.tar.gz" --no-same-owner --strip-components=1 -C "$WORK/source"
}

build_source() {
    cd "$SRC" || return 1
    # Not rust-toolchain.toml's toolchain: that also pulls clippy and rustfmt.
    RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR="$TARGET" \
        cargo build --release --locked ${JOBS:+-j"$JOBS"} --bin veyron --bin veyronctl
}

prepare_source() {
    setup_rust
    if [ -n "$CHECKOUT" ]; then
        SRC=$CHECKOUT TARGET="$CHECKOUT/target"
    else
        SRC="$WORK/source" TARGET="$CACHE/target"
        mkdir -p "$CACHE"
        free=$(df -Pk "$CACHE" | awk 'NR == 2 {print $4}')
        [ "${free:-0}" -ge 3000000 ] || warn "$(t 'less than 3 GB free, the build may fail' 'menos de 3 GB livres, a compilação pode falhar')"
        step "$(t 'Downloading the source' 'Baixando o código')" download_source
    fi
    BUILD_TOTAL=$(grep -c '^name = ' "$SRC/Cargo.lock" || true)
    step "$(t 'Building' 'Compilando')" build_source
    cp "$TARGET/release/veyron" "$TARGET/release/veyronctl" "$STAGE/"
}

download_binary() {
    if [ -n "$FROM_DIR" ]; then
        cp "$FROM_DIR/$ASSET" "$WORK/$ASSET"
    else
        fetch "$BASE/$ASSET" "$WORK/$ASSET"
    fi
}

verify_binary() {
    cd "$WORK" || return 1
    grep " [* ]$ASSET\$" SHA256SUMS >wanted.sha256 || return 1
    sha256sum -c wanted.sha256 || return 1
    tar -xzf "$ASSET" --no-same-owner -C "$STAGE" veyron veyronctl
}

prepare_binary() {
    step "$(t 'Downloading Veyron' 'Baixando o Veyron')" download_binary
    step "$(t 'Checking the file' 'Conferindo o arquivo')" verify_binary
}

# Replace by rename, so updating works while Veyron is running.
place_binaries() {
    mkdir -p "$BIN" || return 1
    for name in veyron veyronctl; do
        cp "$STAGE/$name" "$BIN/.$name.new" || return 1
        chmod 755 "$BIN/.$name.new" || return 1
        mv -f "$BIN/.$name.new" "$BIN/$name" || return 1
    done
}

run_nix() { nix --extra-experimental-features 'nix-command flakes' "$@"; }

nix_profile() {
    op=$1
    shift
    if [ -n "${VEYRON_NIX_PROFILE:-}" ]; then
        run_nix profile "$op" --profile "$VEYRON_NIX_PROFILE" "$@"
    else
        run_nix profile "$op" "$@"
    fi
}

# Names of the profile entries that hold Veyron.
nix_entries() {
    nix_profile list 2>/dev/null | sed "s/${ESC}\[[0-9;]*m//g" |
        awk '$1 == "Name:" {name = $2} /^Store paths:/ && /-veyron-wm-/ {print name}'
}

nix_build() {
    set --
    if [ -n "$JOBS" ]; then set -- --max-jobs "$JOBS" --cores "$JOBS"; fi
    # --refresh: branches are otherwise cached for an hour.
    run_nix build --refresh --no-link --print-out-paths "$@" "$FLAKE" >"$WORK/nix-path"
}

nix_install() {
    # Built already, so the old version is removed only once the new one works.
    for entry in $(nix_entries); do nix_profile remove "$entry" || return 1; done
    nix_profile install "$FLAKE"
}

install_veyron() {
    STAGE="$WORK/stage"
    mkdir "$STAGE"
    if [ "$METHOD" = nix ]; then
        step "$(t 'Building with Nix' 'Compilando com o Nix')" nix_build
        STAGE="$(cat "$WORK/nix-path")/bin"
    else
        dependencies
        if [ "$METHOD" = binary ]; then prepare_binary; else prepare_source; fi
    fi
    step "$(t 'Testing the binary' 'Testando o binário')" "$STAGE/veyron" --version
    if [ "$METHOD" = nix ]; then
        step "$(t 'Installing in your profile' 'Instalando no seu perfil')" nix_install
        VEYRON="$STAGE/veyron"
    else
        step "$(t 'Installing' 'Instalando')" place_binaries
        VEYRON="$BIN/veyron"
        if [ "$METHOD" = source ] && [ -z "$CHECKOUT" ]; then rm -rf "$TARGET"; fi
    fi
}

setup_config() {
    if [ -e "$CONFIG" ]; then
        done_step "$(t 'Configuration kept' 'Configuração mantida')"
        "$VEYRON" --check-config --config "$CONFIG" >"$WORK/config.log" 2>&1 || {
            warn "$(t 'your configuration has errors:' 'sua configuração tem erros:')"
            cat "$WORK/config.log" >&2
        }
        return 0
    fi
    mkdir -p "${CONFIG%/*}"
    "$VEYRON" --print-config >"$WORK/veyron.toml"
    mv "$WORK/veyron.toml" "$CONFIG"
    done_step "$(t 'Configuration created' 'Configuração criada')"
}

finish() {
    printf '\n  %s%s%s\n\n' "$GOOD" "$(t 'VEYRON IS INSTALLED' 'VEYRON INSTALADO')" "$RESET"
    section "$(t 'Next' 'Agora')"
    field "$(t Start Abrir)" "veyron"
    field Terminal "super+return"
    field Design "super+alt+t"
    field "$(t Quit Sair)" "super+shift+e"
    field Config "$(tilde "$CONFIG")"
    section_end

    found=$(command -v veyron || true)
    if [ "$METHOD" != nix ]; then
        case ":$START_PATH:" in
            *":$BIN:"*) ;;
            *)
                warn "$(t "$BIN is not on your PATH. Add this to your shell profile:" "$BIN não está no seu PATH. Ponha isto no seu perfil do shell:")"
                # shellcheck disable=SC2016 # printed for the user's shell to expand
                printf '      export PATH="%s:$PATH"\n' "$BIN" >&2 ;;
        esac
        if [ -n "$found" ] && [ "$found" != "$BIN/veyron" ]; then
            warn "$(t "another veyron comes first on PATH: $found" "outro veyron vem antes no PATH: $found")"
        fi
    elif [ -e "$BIN/veyron" ] && [ "$found" = "$BIN/veyron" ]; then
        warn "$(t "an older copy at $found hides the Nix one; delete it" "uma cópia antiga em $found esconde a do Nix; apague-a")"
    fi
}

uninstall() {
    entries=''
    if command -v nix >/dev/null 2>&1; then entries=$(nix_entries || true); fi
    section "$(t 'Remove Veyron' 'Remover o Veyron')"
    anything=0
    for name in veyron veyronctl; do
        if [ -e "$BIN/$name" ]; then field "$(t file arquivo)" "$BIN/$name"; anything=1; fi
    done
    for entry in $entries; do field "nix" "$entry"; anything=1; done
    section_end
    if [ "$anything" -eq 0 ]; then
        printf '  %s\n' "$(t 'Nothing to remove.' 'Nada para remover.')"
        return 0
    fi
    [ "$DRY_RUN" -eq 0 ] || return 0
    if [ "$ASSUME_YES" -eq 0 ]; then
        [ "$HAS_TTY" -eq 1 ] || die "$(t 'run again with --yes' 'rode de novo com --yes')"
        # Cancel comes first, so a stray Enter removes nothing.
        choose "$(t Cancel Cancelar)" "$(t Remove Remover)"
        [ "$CHOICE" = 2 ] || cancelled
    fi
    rm -f "$BIN/veyron" "$BIN/veyronctl"
    for entry in $entries; do nix_profile remove "$entry"; done
    done_step "$(t 'Removed, configuration kept' 'Removido, configuração mantida')"
}

# --- main --------------------------------------------------------------------

need_value() { [ "$2" -ge 2 ] || die "$1 needs a value"; }

main() {
    JOB='' SAVED_STTY='' WORK='' BUILD_TOTAL=0 BASE='' RUST_SETUP='' MISSING='' JOBS=''
    setup_terminal

    ASSUME_YES=0 DRY_RUN=0 FORCE_SOURCE=0 INSTALL_DEPS=1 UNINSTALL=0
    RELEASE='' FROM_DIR='' REF=main REF_GIVEN=0
    PREFIX="${HOME}/.local"
    while [ $# -gt 0 ]; do
        case $1 in
            -y|--yes) ASSUME_YES=1 ;;
            --dry-run) DRY_RUN=1 ;;
            --from-source) FORCE_SOURCE=1 ;;
            --no-deps) INSTALL_DEPS=0 ;;
            --uninstall) UNINSTALL=1 ;;
            --release) need_value "$1" $#; RELEASE=$2 REF=$2 REF_GIVEN=1; shift ;;
            --ref) need_value "$1" $#; REF=$2 REF_GIVEN=1 FORCE_SOURCE=1; shift ;;
            --prefix) need_value "$1" $#; PREFIX=$2; shift ;;
            --from-dir) need_value "$1" $#; FROM_DIR=$2; shift ;;
            -h|--help) usage; exit 0 ;;
            *) die "unknown option '$1' (see --help)" ;;
        esac
        shift
    done
    case $REF in *[!A-Za-z0-9._/-]*|*..*|'') die "invalid ref '$REF'" ;; esac
    case $PREFIX in /*) ;; *) PREFIX="$PWD/$PREFIX" ;; esac

    if [ "$(id -u)" -eq 0 ] && [ -n "${SUDO_USER:-}" ] && [ "$SUDO_USER" != root ]; then
        die "$(t 'run it without sudo: it asks for the password when needed' 'rode sem sudo: ele pede a senha quando precisar')"
    fi

    BIN="$PREFIX/bin"
    CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/veyron/install"
    CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/veyron/veyron.toml"
    START_PATH=$PATH

    trap cleanup EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM HUP
    WORK=$(mktemp -d "${TMPDIR:-/tmp}/veyron-install.XXXXXX")

    banner
    if [ "$UNINSTALL" -eq 1 ]; then
        uninstall
        return 0
    fi

    detect_system
    # A ref or release asked for by name beats the checkout the script is in.
    if [ "$REF_GIVEN" -eq 1 ]; then CHECKOUT=''; fi
    if [ -n "$CHECKOUT" ]; then
        FLAKE="$CHECKOUT#default"
    else
        FLAKE="github:$REPO?ref=$REF#default"
    fi
    choose_method
    make_plan
    show_plan
    if [ "$DRY_RUN" -eq 1 ]; then
        show_details
        printf '  %s\n' "$(t 'Dry run, nothing was changed.' 'Simulação, nada foi alterado.')"
        return 0
    fi
    confirm
    install_veyron
    setup_config
    finish
}

{ main "$@"; exit; }
