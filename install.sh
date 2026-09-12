#!/bin/sh
set -eu
umask 022

if [ -z "${HOME:-}" ]; then
    echo "Error: HOME is not set. Please set HOME and try again." >&2
    exit 1
fi

# Detect OS and Architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

if [ "$OS" = "Linux" ]; then
    if [ "$ARCH" = "x86_64" ]; then
        PLATFORM="linux-x86_64"
    else
        echo "Error: Unsupported architecture $ARCH on Linux"
        exit 1
    fi
    INSTALL_DIR="$HOME/.local/share/taurine/bin"
elif [ "$OS" = "Darwin" ]; then
    if [ "$ARCH" = "x86_64" ]; then
        PLATFORM="macos-x86_64"
    elif [ "$ARCH" = "arm64" ] || [ "$ARCH" = "aarch64" ]; then
        PLATFORM="macos-aarch64"
    else
        echo "Error: Unsupported architecture $ARCH on macOS"
        exit 1
    fi
    INSTALL_DIR="$HOME/Library/Application Support/Taurine/bin"
else
    echo "Error: Unsupported OS $OS"
    exit 1
fi

# Verify required commands
for cmd in curl tar; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "Error: $cmd is required but was not found. Please install it and try again." >&2
        exit 1
    fi
done

# Cleanup handler — always remove temp dir on exit
TMP_DIR=$(mktemp -d)
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

run_with_spinner() {
    local label=$1
    local cmd=$2
    local success_label=${3:-$label}
    local err_file="${TMP_DIR}/spinner_err.$$"

    eval "$cmd" >/dev/null 2>"$err_file" &
    local pid=$!
    local delay=0.08
    local spinstr='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏'

    # Disable set -e temporarily to safely manage spinner loop and wait
    set +e
    while kill -0 $pid 2>/dev/null; do
        local temp=${spinstr#?}
        printf "\r%c %s" "$spinstr" "$label"
        spinstr=$temp${spinstr%"$temp"}
        sleep $delay
    done

    wait $pid
    local exit_code=$?
    set -e

    if [ $exit_code -eq 0 ]; then
        printf "\r\x1b[32m✓\x1b[0m %s\x1b[K\n" "$success_label"
    else
        printf "\r\x1b[31m✗\x1b[0m %s\x1b[K\n" "$label"
        if [ -s "$err_file" ]; then
            sed 's/^/  /' "$err_file" >&2
        fi
    fi

    rm -f "$err_file"
    return $exit_code
}

verify_checksum() {
    file=$1
    expected=$2
    if command -v sha256sum >/dev/null 2>&1; then
        echo "$expected  $file" | sha256sum -c - > /dev/null 2>&1
    elif command -v shasum >/dev/null 2>&1; then
        echo "$expected  $file" | shasum -a 256 -c - > /dev/null 2>&1
    elif command -v openssl >/dev/null 2>&1; then
        actual=$(openssl dgst -sha256 "$file" | cut -d' ' -f2)
        [ "$actual" = "$expected" ]
    else
        echo "Error: No sha256 checksum tool available (tried sha256sum, shasum, openssl)"
        exit 1
    fi
}

# Human-readable byte size (1024-based, 1 decimal, rounded): "12.4 MB", "512 B"
format_bytes() {
    local b=${1:-0}
    case $b in ''|*[!0-9]*) b=0 ;; esac
    if [ "$b" -lt 1024 ]; then
        printf "%s B" "$b"
    elif [ "$b" -lt 1048576 ]; then
        scaled_tenth $(( (b * 10 + 512) / 1024 )) "KB"
    elif [ "$b" -lt 1073741824 ]; then
        scaled_tenth $(( (b * 10 + 524288) / 1048576 )) "MB"
    else
        scaled_tenth $(( (b * 10 + 536870912) / 1073741824 )) "GB"
    fi
}

# Print "<whole>.<tenth> <unit>" from a value already scaled x10, carrying
# overflow (e.g. 9.95 KB rounds to 10.0 KB, not 9.10 KB)
scaled_tenth() {
    local t=$1
    local unit=$2
    printf "%s.%s %s" "$((t / 10))" "$((t % 10))" "$unit"
}

# downloaded/total sharing one unit picked from total: "12.4/28.1 MB"
format_pair() {
    local cur=${1:-0}
    local total=${2:-0}
    case $cur in ''|*[!0-9]*) cur=0 ;; esac
    case $total in ''|*[!0-9]*) total=0 ;; esac
    local div
    local unit
    if [ "$total" -ge 1073741824 ]; then
        div=1073741824; unit="GB"
    elif [ "$total" -ge 1048576 ]; then
        div=1048576; unit="MB"
    elif [ "$total" -ge 1024 ]; then
        div=1024; unit="KB"
    else
        printf "%s/%s B" "$cur" "$total"
        return 0
    fi
    printf "%s/%s %s" "$(scaled_pair "$cur" "$div")" "$(scaled_pair "$total" "$div")" "$unit"
}

# One side of a pair, rounded to 1 decimal in the shared unit
scaled_pair() {
    local v=$1
    local div=$2
    local t=$(( (v * 10 + div / 2) / div ))
    printf "%s.%s" "$((t / 10))" "$((t % 10))"
}

format_speed() {
    local s=${1:-0}
    case $s in ''|*[!0-9]*) s=0 ;; esac
    printf "%s/s" "$(format_bytes "$s")"
}

format_duration() {
    local secs=${1:-0}
    case $secs in ''|*[!0-9]*) secs=0 ;; esac
    if [ "$secs" -lt 60 ]; then
        printf "%ss" "$secs"
    else
        printf "%sm %ss" "$((secs / 60))" "$((secs % 60))"
    fi
}

# Line-2 content for the open download block
progress_line2() {
    local cur=$1
    local total=$2
    local speed=$3
    case $total in ''|*[!0-9]*|0)
        printf "  %s downloaded @ %s" "$(format_bytes "$cur")" "$(format_speed "$speed")"
        ;;
        *)
        local pct=$((cur * 100 / total))
        if [ "$pct" -gt 100 ]; then pct=100; fi
        printf "  %s (%s%%) @ %s" "$(format_pair "$cur" "$total")" "$pct" "$(format_speed "$speed")"
        ;;
    esac
}

file_size() {
    if [ "$OS" = "Darwin" ]; then
        stat -f%z "$1" 2>/dev/null || echo 0
    else
        stat -c%s "$1" 2>/dev/null || echo 0
    fi
}

# Best-effort total via HEAD (follows GitHub/S3 redirects); empty = unknown
fetch_content_length() {
    local len
    len=$(curl -fsSLI --max-time 10 "$1" 2>/dev/null | grep -i '^content-length:' | tail -n 1 | tr -d '\r' | awk '{print $2}' || true)
    case $len in ''|*[!0-9]*) printf "" ;; *) printf "%s" "$len" ;; esac
}

# Two-line download: line 1 spins the label, line 2 shows live progress.
# Appends " (28.1 MB in 8s)" totals to the success label on completion.
download_with_progress() {
    local url=$1
    local out=$2
    local label=$3
    local success_label=${4:-$label}
    local err_file="${TMP_DIR}/dl_err.$$"
    local total=""
    local start
    start=$(date +%s)

    if [ ! -t 1 ]; then
        printf "%s...\n" "$label"
        if curl -fsSL --max-time 300 "$url" -o "$out" 2>"$err_file"; then
            local end
            end=$(date +%s)
            printf "%s (%s in %s)\n" "$success_label" \
                "$(format_bytes "$(file_size "$out")")" "$(format_duration $((end - start)))"
            rm -f "$err_file"
            return 0
        fi
        if [ -s "$err_file" ]; then sed 's/^/  /' "$err_file" >&2; fi
        rm -f "$err_file"
        return 1
    fi

    rm -f "$out"
    # Total is only needed for the live percentage; skip the HEAD round-trip
    # entirely when non-interactive.
    total=$(fetch_content_length "$url")
    curl -fsSL --max-time 300 "$url" -o "$out" 2>"$err_file" &
    local cpid=$!
    local spinstr='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏'
    printf "%s\n" "$label"

    set +e
    local tick=0
    local size_1s_ago=0
    local speed=0
    while kill -0 $cpid 2>/dev/null; do
        local temp=${spinstr#?}
        local frame="$spinstr"
        spinstr=$temp${spinstr%"$temp"}
        local cur
        cur=$(file_size "$out")
        tick=$((tick + 1))
        if [ $((tick % 5)) -eq 0 ]; then
            speed=$((cur - size_1s_ago))
            size_1s_ago=$cur
        fi
        printf "\x1b[1A\r%c %s\x1b[K\n\r%s\x1b[K" "$frame" "$label" "$(progress_line2 "$cur" "$total" "$speed")"
        sleep 0.2
    done

    wait $cpid
    local exit_code=$?
    set -e

    local end
    end=$(date +%s)
    local elapsed=$((end - start))
    if [ $exit_code -eq 0 ]; then
        printf "\r\x1b[K\x1b[1A\r\x1b[32m✓\x1b[0m %s (%s in %s)\x1b[K\n" "$success_label" \
            "$(format_bytes "$(file_size "$out")")" "$(format_duration "$elapsed")"
    else
        printf "\r\x1b[K\x1b[1A\r\x1b[31m✗\x1b[0m %s\x1b[K\n" "$label"
        if [ -s "$err_file" ]; then
            sed 's/^/  /' "$err_file" >&2
        fi
    fi

    rm -f "$err_file"
    return $exit_code
}

# Retry wrapper for downloads (mirrors invoke_with_retry messaging)
download_with_retry() {
    local url=$1
    local out=$2
    local label=$3
    local success_label=${4:-$label}
    local max_attempts=3
    local attempt=1
    local delay=2

    while [ $attempt -le $max_attempts ]; do
        local current_label="$label"
        if [ $attempt -gt 1 ]; then
            current_label="$label (attempt $attempt/$max_attempts)"
        fi

        if download_with_progress "$url" "$out" "$current_label" "$success_label"; then
            return 0
        fi

        if [ $attempt -lt $max_attempts ]; then
            echo "  Retrying in ${delay}s... ($attempt/$max_attempts)" >&2
            sleep $delay
            delay=$((delay * 2))
        fi
        attempt=$((attempt + 1))
    done

    echo "Error: Failed after $max_attempts attempts: $label" >&2
    return 1
}

# Invoke a command with retry and spinner
invoke_with_retry() {
    local label=$1
    local cmd=$2
    local success_label=${3:-$label}
    local max_attempts=3
    local attempt=1
    local delay=2

    while [ $attempt -le $max_attempts ]; do
        local current_label="$label"
        if [ $attempt -gt 1 ]; then
            current_label="$label (attempt $attempt/$max_attempts)"
        fi

        if run_with_spinner "$current_label" "$cmd" "$success_label"; then
            return 0
        fi

        if [ $attempt -lt $max_attempts ]; then
            echo "  Retrying in ${delay}s..." >&2
            sleep $delay
            delay=$((delay * 2))
        fi
        attempt=$((attempt + 1))
    done

    echo "Error: Failed after $max_attempts attempts: $label" >&2
    return 1
}

version_gt() {
    # Returns 0 if $1 > $2, 1 otherwise
    v1="${1%%-*}"
    v2="${2%%-*}"

    i=1
    while [ $i -le 4 ]; do
        a=$(echo "$v1" | cut -d. -f$i)
        b=$(echo "$v2" | cut -d. -f$i)
        a="${a%%[!0-9]*}"
        b="${b%%[!0-9]*}"
        a="${a:-0}"
        b="${b:-0}"
        if [ "$a" -gt "$b" ] 2>/dev/null; then return 0; fi
        if [ "$a" -lt "$b" ] 2>/dev/null; then return 1; fi
        i=$((i + 1))
    done
    return 1
}

trim() {
    echo "$1" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//'
}

configure_profile() {
    profile="$1"
    shell_type="$2"
    modified=false

    # Ensure profile directory exists
    mkdir -p "$(dirname "$profile")"

    # 1. Handle PATH (idempotent)
    if [ "$shell_type" = "fish" ]; then
        path_line="fish_add_path \"$INSTALL_DIR\""
        if [ -f "$profile" ]; then
            if ! grep -Fq "$path_line" "$profile" 2>/dev/null; then
                printf "\n%s\n" "$path_line" >> "$profile"
                modified=true
            fi
        else
            printf "\n%s\n" "$path_line" >> "$profile"
            modified=true
        fi
    elif [ "$shell_type" = "csh" ]; then
        path_line="set path = ( \$path \"$INSTALL_DIR\" )"
        if [ -f "$profile" ]; then
            if ! grep -Fq "$path_line" "$profile" 2>/dev/null; then
                printf "\n%s\n" "$path_line" >> "$profile"
                modified=true
            fi
        else
            printf "\n%s\n" "$path_line" >> "$profile"
            modified=true
        fi
    else
        path_line="export PATH=\"$INSTALL_DIR:\$PATH\""
        if [ -f "$profile" ]; then
            if ! grep -Fxq "$path_line" "$profile" 2>/dev/null; then
                printf "\n%s\n" "$path_line" >> "$profile"
                modified=true
            fi
        else
            printf "\n%s\n" "$path_line" >> "$profile"
            modified=true
        fi
    fi

    # 2. Handle Alias (Ensure only ONE entry of the alias exists, and it's correct)
    alias_line="alias tau='taurine'"
    alias_prefix="alias tau="
    if [ "$shell_type" = "csh" ]; then
        alias_line="alias tau taurine"
        alias_prefix="alias tau "
    fi

    if [ -f "$profile" ]; then
        matches=$(grep -F "$alias_prefix" "$profile" || true)
        count=$(echo "$matches" | grep -c . || true)
        trimmed_match=$(trim "$matches")

        if [ "$count" -eq 1 ] && [ "$trimmed_match" = "$alias_line" ]; then
            : # Already correct
        else
            tmp_profile=$(mktemp)
            grep -Fv "$alias_prefix" "$profile" > "$tmp_profile" || true
            cat "$tmp_profile" > "$profile"
            rm -f "$tmp_profile"
            printf "\n%s\n" "$alias_line" >> "$profile"
            modified=true
        fi
    else
        printf "\n%s\n" "$alias_line" >> "$profile"
        modified=true
    fi

    if [ "$modified" = true ]; then
        return 0 # Modified
    else
        return 1 # Not modified
    fi
}

IS_INSTALLED=false
IS_FRESH_INSTALL=false
VERSION=""
URL=""
SHA256=""

# 1. Local Check First
if [ -x "$INSTALL_DIR/taurine" ]; then
    IS_INSTALLED=true
    LOCAL_VERSION=$("$INSTALL_DIR/taurine" --version 2>/dev/null | awk '{print $2}') || true
    if [ -n "$LOCAL_VERSION" ]; then
        # Try fetching manifest silently to check if up to date
        if curl -fsSL -H 'Accept: application/vnd.github+json' --max-time 10 https://api.github.com/repos/ereinaimer/taurine/releases -o "$TMP_DIR/releases.json" >/dev/null 2>&1; then
            RELEASE_URL=$(grep -o '"url"[[:space:]]*:[[:space:]]*"https://api.github.com/repos/ereinaimer/taurine/releases/[0-9]*"' "$TMP_DIR/releases.json" | head -n 1 | cut -d'"' -f4)
            if [ -n "$RELEASE_URL" ]; then
                if curl -fsSL -H 'Accept: application/vnd.github+json' --max-time 10 "$RELEASE_URL" -o "$TMP_DIR/release.json" >/dev/null 2>&1; then
                    MANIFEST_ASSET_URL=$(grep -o '"browser_download_url"[[:space:]]*:[[:space:]]*"[^"]*manifest\.json"' "$TMP_DIR/release.json" | head -n 1 | cut -d'"' -f4)
                    if [ -n "$MANIFEST_ASSET_URL" ]; then
                        if curl -fsSL --max-time 10 "$MANIFEST_ASSET_URL" -o "$TMP_DIR/manifest.json" >/dev/null 2>&1; then
                            MANIFEST=$(tr -d '\n\r\t ' < "$TMP_DIR/manifest.json")
                            VERSION=$(echo "$MANIFEST" | grep -o '"version":"[^"]*"' | head -n 1 | cut -d'"' -f4 || true)
                            URL=$(echo "$MANIFEST" | grep -o "\"$PLATFORM\":{[^}]*}" | grep -o '"url":"[^"]*"' | cut -d'"' -f4 || true)
                            SHA256=$(echo "$MANIFEST" | grep -o "\"$PLATFORM\":{[^}]*}" | grep -o '"sha256":"[^"]*"' | cut -d'"' -f4 || true)
                            # Handle malformed sha256 with filename prefix (e.g. "checksums/file.sha256:hash")
                            SHA256="${SHA256##*:}"
                        fi
                    fi
                fi
            fi
        fi

        if [ -n "$VERSION" ] && [ -n "$URL" ]; then
            if [ "$LOCAL_VERSION" = "$VERSION" ] || version_gt "$LOCAL_VERSION" "$VERSION"; then
                printf "\x1b[32m✓\x1b[0m Taurine is up to date (v%s)\n" "$LOCAL_VERSION"
            fi
        fi
    fi
fi

# 2. Manifest fetch if not already populated (e.g. fresh install or silent check failed)
if [ -z "$VERSION" ] || [ -z "$URL" ]; then
    invoke_with_retry "Fetching release manifest" "
        set -e
        curl -fsSL -H 'Accept: application/vnd.github+json' --max-time 10 \
            https://api.github.com/repos/ereinaimer/taurine/releases \
            -o \"$TMP_DIR/releases.json\"
        RELEASE_URL=\$(grep -o '\"url\"[[:space:]]*:[[:space:]]*\"https://api.github.com/repos/ereinaimer/taurine/releases/[0-9]*\"' \"$TMP_DIR/releases.json\" | head -n 1 | cut -d'\"' -f4)
        [ -n \"\$RELEASE_URL\" ]
        curl -fsSL -H 'Accept: application/vnd.github+json' --max-time 10 \"\$RELEASE_URL\" \
            -o \"$TMP_DIR/release.json\"
        MANIFEST_ASSET_URL=\$(grep -o '\"browser_download_url\"[[:space:]]*:[[:space:]]*\"[^\"]*manifest\\.json\"' \"$TMP_DIR/release.json\" | head -n 1 | cut -d'\"' -f4)
        [ -n \"\$MANIFEST_ASSET_URL\" ]
        curl -fsSL --max-time 10 \"\$MANIFEST_ASSET_URL\" -o \"$TMP_DIR/manifest.json\"
    " "Fetched release manifest" || exit 1

    MANIFEST=$(tr -d '\n\r\t ' < "$TMP_DIR/manifest.json")
    VERSION=$(echo "$MANIFEST" | grep -o '"version":"[^"]*"' | head -n 1 | cut -d'"' -f4 || true)
    URL=$(echo "$MANIFEST" | grep -o "\"$PLATFORM\":{[^}]*}" | grep -o '"url":"[^"]*"' | cut -d'"' -f4 || true)
    SHA256=$(echo "$MANIFEST" | grep -o "\"$PLATFORM\":{[^}]*}" | grep -o '"sha256":"[^"]*"' | cut -d'"' -f4 || true)
    # Handle malformed sha256 with filename prefix (e.g. "checksums/file.sha256:hash")
    SHA256="${SHA256##*:}"

    # Validate manifest structure
    if [ -z "$VERSION" ] || [ -z "$URL" ]; then
        echo "Error: Invalid manifest - missing version or URL for $PLATFORM"
        exit 1
    fi
fi

# 3. Handle already installed but outdated/failed checks
if [ "$IS_INSTALLED" = true ]; then
    if [ -n "${VERSION:-}" ] && [ -n "${LOCAL_VERSION:-}" ]; then
        if [ "$LOCAL_VERSION" != "$VERSION" ] && ! version_gt "$LOCAL_VERSION" "$VERSION"; then
            printf "Taurine is already installed but a newer version (v%s) is available. Please run 'tau update' to update.\n" "$VERSION"
        fi
    elif [ -z "${LOCAL_VERSION:-}" ]; then
        printf "Taurine is already installed. If you want to update to the latest version (v%s), please run 'tau update'.\n" "$VERSION"
    fi
fi

# 4. Perform fresh install if not installed
if [ "$IS_INSTALLED" = false ]; then
    ARCHIVE="$TMP_DIR/taurine.tar.xz"

    # Download archive with retry and live two-line progress
    download_with_retry "$URL" "$ARCHIVE" "Downloading taurine v$VERSION" "Downloaded taurine v$VERSION" || exit 1

    # Verify checksum if available
    if [ -n "$SHA256" ]; then
        verify_checksum "$ARCHIVE" "$SHA256" || {
            echo "Error: Checksum verification failed for downloaded archive."
            exit 1
        }
    fi

    # Extract
    mkdir -p "$INSTALL_DIR"
    run_with_spinner "Extracting" "tar -xf \"$ARCHIVE\" -C \"$TMP_DIR\"" "Extracted" || {
        echo "Error: Extraction failed." >&2
        exit 1
    }

    # Copy binary
    cp "$TMP_DIR/taurine" "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/taurine"

    # Download the uninstaller before reporting a successful installation.
    UNINSTALL_SCRIPT="$INSTALL_DIR/uninstall.sh"
    if ! curl -fsSL --max-time 30 "https://raw.githubusercontent.com/ereinaimer/taurine/main/uninstall.sh" -o "$UNINSTALL_SCRIPT" >/dev/null 2>&1; then
        rm -f "$UNINSTALL_SCRIPT"
        echo "Error: Failed to download the Taurine uninstaller script." >&2
        exit 1
    fi
    chmod +x "$UNINSTALL_SCRIPT"

    IS_INSTALLED=true
    IS_FRESH_INSTALL=true

    printf "\x1b[32m✓\x1b[0m taurine v%s installed\n" "$VERSION"

    # Start the service after installation (detached)
    if [ -x "$INSTALL_DIR/taurine" ]; then
        "$INSTALL_DIR/taurine" up > /dev/null 2>&1 &
    fi
fi

# Configure shell profiles if installation succeeded (either fresh or pre-existing)
if [ "$IS_INSTALLED" = true ]; then
    # Determine active shell to ensure we have at least one profile file
    ACTIVE_SHELL=$(basename "${SHELL:-bash}")
    case "$ACTIVE_SHELL" in
        zsh)   touch "$HOME/.zshrc" ;;
        fish)  mkdir -p "$HOME/.config/fish" && touch "$HOME/.config/fish/config.fish" ;;
        csh|tcsh) touch "$HOME/.cshrc" ;;
        bash)  touch "$HOME/.bashrc" ;;
        *)     touch "$HOME/.profile" ;;
    esac

    ADDED_PATH=false
    MODIFIED_PROFILES=""

    # POSIX profiles
    for profile in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.bash_profile" "$HOME/.zprofile" "$HOME/.profile"; do
        if [ -f "$profile" ]; then
            if configure_profile "$profile" "posix"; then
                ADDED_PATH=true
                MODIFIED_PROFILES="$MODIFIED_PROFILES $profile"
            fi
        fi
    done

    # Fish profile
    FISH_PROFILE="$HOME/.config/fish/config.fish"
    if [ -f "$FISH_PROFILE" ]; then
        if configure_profile "$FISH_PROFILE" "fish"; then
            ADDED_PATH=true
            MODIFIED_PROFILES="$MODIFIED_PROFILES $FISH_PROFILE"
        fi
    fi

    # Csh/Tcsh profiles
    for profile in "$HOME/.tcshrc" "$HOME/.cshrc"; do
        if [ -f "$profile" ]; then
            if configure_profile "$profile" "csh"; then
                ADDED_PATH=true
                MODIFIED_PROFILES="$MODIFIED_PROFILES $profile"
            fi
        fi
    done

    if [ "$IS_FRESH_INSTALL" = true ]; then
        if [ "$ADDED_PATH" = true ]; then
            printf "Added to PATH and set up alias tau in your shell profiles:\n"
            for p in $MODIFIED_PROFILES; do
                printf "  %s\n" "$p"
            done
            printf "Please restart your shell or source your profile to apply the changes.\n"
        else
            ON_PATH=false
            if command -v taurine >/dev/null 2>&1; then
                ON_PATH=true
            fi
            case ":$PATH:" in
                *:"$INSTALL_DIR":*) ON_PATH=true ;;
            esac

            if [ "$ON_PATH" = true ]; then
                printf "\x1b[32m✓\x1b[0m Taurine binary is already on your PATH.\n"
            else
                printf "Taurine binary is configured in your profile, but not in your current shell session.\n"
                printf "Please restart your shell or source your profile.\n"
            fi
            printf "\x1b[32m✓\x1b[0m alias 'tau' is already set up.\n"
        fi
    fi
fi