# Contributing to Mimick

Thank you for considering contributing to Mimick for Linux!

## Code of Conduct

Be welcoming, kind, and keep feedback constructive.

## How Can I Contribute?

### 1. Reporting Bugs

Open an Issue on GitHub. Include:
* Your OS and desktop environment (e.g., Ubuntu 24.04, GNOME Wayland 50).
* Rust and GTK4/Libadwaita versions (`rustc --version`, `pkg-config --modversion gtk4`).
* A clear description and steps to reproduce.
* Logs from a terminal run (`RUST_LOG=debug mimick 2>&1`).

### 2. Suggesting Enhancements

* Check `roadmap.md` to see if it is already planned.
* Check open issues for duplicates.
* If not, open a feature request Issue.

### 3. Pull Requests

**Workflow:**

1. Fork the repo and create your branch from `main`.
2. Install the system prerequisites (see README).
3. Build with `cargo build`.
4. If you add logic, add unit tests in the same file under `#[cfg(test)]`.
5. Run tests: `cargo test`.
6. Ensure no warnings: `cargo clippy`.
7. Review the [API docs](https://mimick.nicx.dev/docs/) if you need to understand module interfaces.
8. Submit your PR.

### 4. Code Style

* Follow standard Rust formatting: `cargo fmt` before committing.
* Use `cargo clippy` and address all lints — no deprecated APIs.
* Unit tests live in the same source file as the code under `#[cfg(test)] mod tests { ... }`.
* GUI code in `settings_window.rs` is intentionally not unit tested — focus on the pure-logic modules.

### 5. Translations

Mimick uses gettext (#220). Strings without a translation show in English.

**Translating:**

1. Copy `po/mimick.pot` to `po/<lang>.po` (for example `po/fr.po`), or run `msginit --locale=<lang> --input=po/mimick.pot --output-file=po/<lang>.po`.
2. Fill in each `msgstr` with Poedit, GNOME Translation Editor, or a text editor. The file also holds the app menu entry and the software center listing; in the `Keywords` entry, keep the `;` between words.
3. Add the language code to `po/LINGUAS`.
4. Check it: `scripts/i18n.sh install target/share/locale`, then `LANGUAGE=<lang> MIMICK_PROFILE=dev cargo run`. The profile starts a separate instance, so an already running Mimick doesn't take over the launch.
5. Run `scripts/i18n.sh progress` to update the progress chart in the README. CI fails when it is out of date.

**Adding strings in code:** run `scripts/i18n.sh pot` afterwards (needs gettext 0.24 or newer) to refresh `po/mimick.pot` and the `.po` files. CI fails when the template is missing a string.

* Wrap user-facing text in `gettextrs::gettext("…")`. Log messages and the diagnostics bundle stay in English.
* Name placeholders and fill them with `crate::i18n::fill(gettext("Merge into {name}?"), &[("name", &name)])`; `format!` can't take a translated string.
* Counts use `ngettext("{n} photo", "{n} photos", plural_n(count))`. Never build plurals with `if n == 1` or "(s)".
* Strings in a `const` use `gettext_noop("…")` and go through `gettext` where they are shown. Keep comparing and storing the English value; only display the translation.
* Dates use `crate::i18n::format_day` or `format_local` with a translatable strftime pattern, so month names follow the locale.
* Add a `// Translators:` comment above a string that needs context.
* The `.desktop` file and metainfo are translated too. Give each new `<release>` a `<description translate="no">` so release notes stay out of the template.

## Development Environment Setup

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install GTK system dependencies (Ubuntu/Debian)
sudo apt install libgtk-4-dev libadwaita-1-dev libglib2.0-dev pkg-config build-essential

# Build and run
cargo run -- --settings

# Run tests
cargo test
```
