#!/usr/bin/env bash
# `pot`: extract strings into po/mimick.pot and merge them into each po/<lang>.po (needs gettext 0.24+).
# `check`: fail if po/mimick.pot is missing or has stale strings, or a po/<lang>.po is broken.
# `install <localedir>`: compile every language in po/LINGUAS into <localedir>.
# `merge <template> <output>`: write a translated copy of a .desktop or metainfo file.
set -euo pipefail

cd "$(dirname "$0")/.."

languages() { sed -e 's/#.*//' -e '/^[[:space:]]*$/d' po/LINGUAS; }

extract() {
    git ls-files 'src/*.rs' | xargs xgettext --language=Rust --from-code=UTF-8 \
        --package-name=mimick --msgid-bugs-address=https://github.com/nicx17/mimick/issues \
        --keyword=gettext_noop --add-comments=Translators: --sort-by-file --output="$1"
    xgettext --join-existing --from-code=UTF-8 --output="$1" setup/dev.nicx.mimick.desktop
    # AppStream's rules, pinned: other installed copies extract release notes or skip keywords.
    xgettext --join-existing --from-code=UTF-8 --its=po/its/metainfo.its --output="$1" \
        setup/metainfo/dev.nicx.mimick.metainfo.xml
}

# Only the strings, plurals, and notes matter; line numbers and the creation date change constantly.
strings_of() { msgcat --no-location --no-wrap --sort-output "$1" | grep -v '^"POT-Creation-Date:'; }

case "${1:-}" in
pot)
    extract po/mimick.pot
    for lang in $(languages); do
        msgmerge --quiet --update --backup=none "po/$lang.po" po/mimick.pot
    done
    ;;
check)
    fresh=$(mktemp)
    trap 'rm -f "$fresh"' EXIT
    extract "$fresh"
    if ! diff -u <(strings_of po/mimick.pot) <(strings_of "$fresh"); then
        echo "po/mimick.pot is out of date: run scripts/i18n.sh pot and commit the result." >&2
        exit 1
    fi
    for lang in $(languages); do
        msgfmt --check --output-file=/dev/null "po/$lang.po"
    done
    ;;
install)
    dest=${2:?usage: $0 install <localedir>}
    for lang in $(languages); do
        install -d "$dest/$lang/LC_MESSAGES"
        msgfmt --check --output-file="$dest/$lang/LC_MESSAGES/mimick.mo" "po/$lang.po"
    done
    ;;
merge)
    template=${2:?usage: $0 merge <template> <output>}
    output=${3:?usage: $0 merge <template> <output>}
    case "$template" in
    *.desktop) kind=--desktop ;;
    *.xml) kind=--xml ;;
    *) echo "Can't merge $template: expected a .desktop or .xml file" >&2 && exit 1 ;;
    esac
    install -d "$(dirname "$output")"
    msgfmt "$kind" --template="$template" -d po --output-file="$output"
    ;;
*)
    echo "usage: $0 pot | check | install <localedir> | merge <template> <output>" >&2
    exit 1
    ;;
esac
