#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")/.."

step() { printf '\n\033[1;34m>> %s\033[0m\n' "$1"; }
failed=()

step "Checking code formatting..."
if ! cargo fmt --all -- --check; then
    echo "Formatting issues found. Running 'cargo fmt' to fix them automatically..."
    cargo fmt --all
    echo "Formatting applied."
fi

step "Running cargo clippy (with -D warnings)..."
cargo clippy --locked --all-targets --all-features -- -D warnings

step "Checking cyclomatic complexity (lizard, as Codacy does)..."
lizard_bin=$(command -v lizard || true)
if [[ -z "$lizard_bin" && -x flatpak-venv/bin/lizard ]]; then
    lizard_bin=flatpak-venv/bin/lizard
fi
if [[ -z "$lizard_bin" ]]; then
    echo "lizard not found, skipping. Install with: pipx install lizard==1.25.0"
elif "$lizard_bin" -C 15 -w src build.rs; then
    echo "No function is over complexity 15."
else
    echo "The functions above are over complexity 15. Continuing with the other checks."
    failed+=("lizard")
fi

step "Checking the translation template..."
if scripts/i18n.sh check; then
    echo "po/mimick.pot is up to date."
else
    failed+=("translations")
fi

step "Running tests..."
cargo test --locked

step "Running cargo audit..."
if ! command -v cargo-audit &>/dev/null; then
    echo "cargo-audit not found. Install with: cargo install cargo-audit"
    exit 1
fi
cargo audit --deny warnings

step "Validating desktop file..."
desktop-file-validate setup/dev.nicx.mimick.desktop

step "Validating metainfo..."
appstreamcli validate --explain setup/metainfo/dev.nicx.mimick.metainfo.xml

echo ""
if ((${#failed[@]})); then
    echo "Failed: ${failed[*]}"
    exit 1
fi
echo "All checks passed successfully!"
