#!/usr/bin/env bash
# `pot`: extract strings into po/mimick.pot and merge them into each po/<lang>.po (needs gettext 0.24+).
# `check`: fail if po/mimick.pot is missing or has stale strings, or a po/<lang>.po is broken.
# `install <localedir>`: compile every language in po/LINGUAS into <localedir>.
# `merge <template> <output>`: write a translated copy of a .desktop or metainfo file.
# `progress`: redraw the translation progress chart in README.md (`pot` does too).
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

# The README's translation progress chart sits between these markers; `progress` rewrites it.
README=README.md
PROGRESS_START='<!-- translations:start -->'
PROGRESS_END='<!-- translations:end -->'

# English names, since the README is in English; unknown codes show as the code.
declare -A NAMES=(
    [ar]=Arabic [bg]=Bulgarian [bn]=Bengali [ca]=Catalan [cs]=Czech [da]=Danish
    [de]=German [el]=Greek [eo]=Esperanto [es]=Spanish [et]=Estonian [eu]=Basque
    [fa]=Persian [fi]=Finnish [fr]=French [gl]=Galician [he]=Hebrew [hi]=Hindi
    [hr]=Croatian [hu]=Hungarian [id]=Indonesian [it]=Italian [ja]=Japanese
    [ko]=Korean [lt]=Lithuanian [lv]=Latvian [nb]="Norwegian Bokmål" [nl]=Dutch
    [oc]=Occitan [pl]=Polish [pt]=Portuguese [pt_BR]="Portuguese (Brazil)"
    [ro]=Romanian [ru]=Russian [sk]=Slovak [sl]=Slovenian [sr]=Serbian
    [sv]=Swedish [ta]=Tamil [th]=Thai [tr]=Turkish [uk]=Ukrainian [vi]=Vietnamese
    [zh_CN]="Chinese (Simplified)" [zh_TW]="Chinese (Traditional)"
)

# "N translated messages, M fuzzy translations, K untranslated messages." → N and the total.
counts() {
    local stats translated=0 total=0 n
    stats=$(LC_ALL=C msgfmt --statistics --output-file=/dev/null "$1" 2>&1)
    for n in $(grep -oE '[0-9]+ (translated|fuzzy|untranslated)' <<<"$stats" | tr ' ' ':'); do
        total=$((total + ${n%%:*}))
        [[ ${n#*:} == translated ]] && translated=${n%%:*}
    done
    echo "$translated $total"
}

# Rows of "percent<TAB>name<TAB>code", fullest first.
rows() {
    local total=$1 lang translated pct
    for lang in $(languages); do
        read -r translated _ < <(counts "po/$lang.po")
        pct=$((total > 0 ? translated * 100 / total : 0))
        printf '%03d\t%s\t%s\n' "$pct" "${NAMES[$lang]:-$lang}" "$lang"
    done | sort -t$'\t' -k1,1r -k2,2
}

# A Mermaid bar chart GitHub draws in the README, or a note when there are no translations yet.
progress_block() {
    local total listing pct name code
    local -a labels=() percents=()
    read -r _ total < <(counts po/mimick.pot)
    listing=$(rows "$total")
    echo "$PROGRESS_START"
    if [[ -z $listing ]]; then
        echo "> **No translations yet.** See [Translations](CONTRIBUTING.md#5-translations) to add your language."
    else
        while IFS=$'\t' read -r pct name code; do
            labels+=("\"$name\"")
            percents+=("$((10#$pct))")
        done <<<"$listing"
        cat <<CHART
\`\`\`mermaid
---
config:
  xyChart:
    chartOrientation: horizontal
    height: $((${#labels[@]} * 44 + 96))
  themeVariables:
    xyChart:
      plotColorPalette: "#FF90C3"
---
xychart-beta
    title "Translated, of $total strings (%)"
    x-axis [$(IFS=,; echo "${labels[*]}")]
    y-axis 0 --> 100
    bar [$(IFS=,; echo "${percents[*]}")]
\`\`\`
CHART
    fi
    echo "$PROGRESS_END"
}

# The README with a fresh progress block between the markers.
readme_with_progress() {
    if ! grep -qxF "$PROGRESS_START" "$README"; then
        echo "$README has no $PROGRESS_START marker" >&2
        return 1
    fi
    BLOCK=$(progress_block) awk -v start="$PROGRESS_START" -v end="$PROGRESS_END" '
        $0 == start { print ENVIRON["BLOCK"]; skipping = 1; next }
        $0 == end { skipping = 0; next }
        !skipping { print }
    ' "$README"
}

write_progress() {
    local updated
    updated=$(readme_with_progress)
    printf '%s\n' "$updated" >"$README"
}

# Only the strings, plurals, and notes matter; line numbers and the creation date change constantly.
strings_of() { msgcat --no-location --no-wrap --sort-output "$1" | grep -v '^"POT-Creation-Date:'; }

case "${1:-}" in
pot)
    extract po/mimick.pot
    for lang in $(languages); do
        msgmerge --quiet --update --backup=none "po/$lang.po" po/mimick.pot
    done
    write_progress
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
    if ! diff -q "$README" <(readme_with_progress) >/dev/null; then
        echo "The translation chart in $README is out of date: run scripts/i18n.sh progress and commit the result." >&2
        exit 1
    fi
    ;;
progress)
    write_progress
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
    echo "usage: $0 pot | check | progress | install <localedir> | merge <template> <output>" >&2
    exit 1
    ;;
esac
