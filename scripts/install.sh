#!/bin/sh
set -eu

usage() {
    cat <<'USAGE'
usage: install.sh --archive FILE [--install-dir DIR] [--checksum-file FILE] [--checksum SHA256] [--no-checksum] [--dry-run]

Installs ferry from a local FileFerry release archive.

Options:
  --archive FILE        Local fileferry-<version>-<target>.tar.gz archive
  --install-dir DIR     Directory for the ferry binary (default: $FILEFERRY_INSTALL_DIR or $HOME/.local/bin)
  --checksum-file FILE  SHA256SUMS file to verify the archive
  --checksum SHA256     Expected archive SHA-256 digest
  --no-checksum         Skip checksum lookup when no checksum is available
  --dry-run             Verify and report without writing the binary
  -h, --help            Show this help
USAGE
}

fail() {
    echo "install.sh: $*" >&2
    exit 1
}

info() {
    echo "install.sh: $*" >&2
}

archive=
install_dir=${FILEFERRY_INSTALL_DIR:-"${HOME:-}/.local/bin"}
checksum_file=
expected_checksum=
no_checksum=0
dry_run=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --archive)
            [ "$#" -ge 2 ] || fail "--archive requires a value"
            archive=$2
            shift 2
            ;;
        --install-dir)
            [ "$#" -ge 2 ] || fail "--install-dir requires a value"
            install_dir=$2
            shift 2
            ;;
        --checksum-file)
            [ "$#" -ge 2 ] || fail "--checksum-file requires a value"
            checksum_file=$2
            shift 2
            ;;
        --checksum)
            [ "$#" -ge 2 ] || fail "--checksum requires a value"
            expected_checksum=$2
            shift 2
            ;;
        --no-checksum)
            no_checksum=1
            shift
            ;;
        --dry-run)
            dry_run=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            fail "unknown option: $1"
            ;;
    esac
done

[ -n "$archive" ] || fail "--archive is required"
[ -n "$install_dir" ] || fail "--install-dir could not be determined"
[ -f "$archive" ] || fail "archive not found: $archive"

archive_dir=$(dirname "$archive")
archive_name=$(basename "$archive")

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        fail "sha256sum or shasum is required for checksum verification"
    fi
}

lookup_checksum() {
    awk -v wanted="$archive_name" '
        $1 ~ /^[0-9A-Fa-f]{64}$/ {
            name = $2
            sub(/^\*/, "", name)
            if (name == wanted) {
                print tolower($1)
                exit
            }
        }
    ' "$1"
}

verify_checksum() {
    if [ "$no_checksum" -eq 1 ] && { [ -n "$expected_checksum" ] || [ -n "$checksum_file" ]; }; then
        fail "--no-checksum cannot be combined with --checksum or --checksum-file"
    fi

    if [ -n "$expected_checksum" ]; then
        expected_checksum=$(printf '%s' "$expected_checksum" | tr 'A-F' 'a-f')
    else
        if [ -z "$checksum_file" ] && [ -f "$archive_dir/SHA256SUMS" ]; then
            checksum_file=$archive_dir/SHA256SUMS
        fi

        if [ -n "$checksum_file" ]; then
            [ -f "$checksum_file" ] || fail "checksum file not found: $checksum_file"
            expected_checksum=$(lookup_checksum "$checksum_file")
            [ -n "$expected_checksum" ] || fail "no checksum entry for $archive_name in $checksum_file"
        elif [ "$no_checksum" -eq 0 ]; then
            info "warning: no checksum supplied; use --checksum-file, --checksum, or --no-checksum"
            return
        else
            info "checksum verification skipped"
            return
        fi
    fi

    actual_checksum=$(sha256 "$archive")
    [ "$actual_checksum" = "$expected_checksum" ] || fail "checksum mismatch for $archive_name"
    info "verified SHA-256 for $archive_name"
}

tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/fileferry-install.XXXXXX") || fail "create temporary directory"
cleanup() {
    rm -rf "$tmpdir"
}
trap cleanup EXIT INT TERM

verify_checksum

tar -xzf "$archive" -C "$tmpdir" || fail "extract archive"

binary=
for candidate in "$tmpdir"/*/ferry; do
    if [ -f "$candidate" ]; then
        binary=$candidate
        break
    fi
done

[ -n "$binary" ] || fail "archive does not contain a ferry binary"
destination=$install_dir/ferry

if [ "$dry_run" -eq 1 ]; then
    info "dry run: would install $binary to $destination"
    exit 0
fi

mkdir -p "$install_dir" || fail "create install directory: $install_dir"
cp "$binary" "$destination" || fail "copy ferry to $destination"
chmod 755 "$destination" || fail "mark ferry executable"

info "installed ferry to $destination"
