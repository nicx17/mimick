# Development Guide

This guide is for developers who want to contribute to `mimick`.

## Setting Up the Environment

### Prerequisites

- Rust toolchain (`cargo` + `rustc`) via [rustup](https://rustup.rs/)
- GTK4 development files
- Libadwaita development files
- libsecret development files (for system keyring access)

### Installation

1. **Clone the Repository:**

    ```bash
    git clone https://github.com/nicx17/mimick.git
    cd mimick
    ```

2. **Install Dependencies (Ubuntu/Debian):**

    ```bash
    sudo apt install libgtk-4-dev libadwaita-1-dev libglib2.0-dev pkg-config build-essential libsecret-1-dev
    ```

3. **Install Dependencies (Fedora):**

    ```bash
    sudo dnf install gtk4-devel libadwaita-devel libsecret-devel pkg-config
    ```

4. **Install Dependencies (Arch Linux):**

    ```bash
    sudo pacman -S gtk4 libadwaita libsecret pkgconf base-devel
    ```

5. **Build and Run:**

    ```bash
    cargo check             # Check if code compiles without building
    cargo run               # Run in background daemon mode
    cargo run -- --settings # Run and immediately open the settings window
    ```

## Logging and Debugging

The application uses `flexi_logger`. 
- By default, `cargo run` prints `INFO` level logs to the terminal.
- Logs are simultaneously written to disk at `~/.cache/mimick/mimick.log`.
- To increase verbosity, set the `RUST_LOG` environment variable:

```bash
RUST_LOG=debug cargo run
```

## Running Tests

The project uses Rust's built-in testing framework. Most business logic (queue parsing, SHA1 calculators, path handlers) can be tested.

```bash
# Run all unit tests
cargo test
```

## UI Structure and Main Loop

Unlike traditional Python/PySide loops, `mimick` is built on GTK4 and multi-threaded `tokio`.

1. `main.rs`: Initialises the GTK `adw::Application` and spins up the background `tokio` runtime for the file monitor and network queue.
2. `settings_window.rs`: Uses declarative GTK Builder pattern to construct the UI. The UI reads status via a shared `Arc<Mutex<AppState>>` memory lock rather than disk polling.
3. GTK restricts all UI modifications to the main thread. To update the UI from async workers, use generic channels or `glib::timeout_add_local`.

## Packaging

To test the final executable bundle:

1. Run `./build_test_appimage.sh`
2. This script compiles a highly optimised release binary (`target/release/mimick`) stripped of debug symbols and panics, then packages it into a monolithic `.AppImage`.
3. To install locally, run `./install-appimage.sh /path/to/Mimick-2.0.x-x86_64.AppImage`

## Contributing Workflow

1. **Fork** the repository.
2. **Clone** your fork.
3. Create a **feature branch**: `git checkout -b feature/my-new-feature`.
4. Run `cargo clippy` to ensure your code matches Rust idioms.
5. Commit your changes.
6. Submit a **Pull Request**.
