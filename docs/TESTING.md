# Testing Guide: Mimick

This document outlines how to execute and expand the automated testing suite for the Mimick application.

## 1. The Testing Framework
The application uses the standard **`cargo test`** runner built into Rust.

### Prerequisites
Ensure your Rust toolchain is up to date:
```bash
rustup update stable
```

---

## 2. Running Tests

To run the entire test suite simply execute:
```bash
cargo test
```

### Checking Specific Modules
You can target specific modules or functions:
```bash
# Run tests only in monitor.rs
cargo test monitor::

# Run tests with output printed to terminal (normally hidden on success)
cargo test -- --nocapture
```

---

## 3. Test File Structure

Tests in Rust are written inline within identical files to the logic they test, placed inside `#[cfg(test)]` modules at the bottom of the files.

| Source File | Test Location | Description |
| :--- | :--- | :--- |
| `src/monitor.rs` | `mod tests` | Tests chunked SHA-1 generation (`compute_sha1_chunked`) for reliable and memory-safe deduplication. |
| `src/config.rs` | `mod tests` | Tests JSON serde serialization/deserialization and default path resolutions. |
| `src/queue_manager.rs` | N/A | Tests pending refactor for Tokio channel boundaries. |

---

## 4. Current Coverage Gaps

While core data structures and parsers are tested, the following areas currently have **limited coverage** and rely heavily on manual UI testing during development:

1.  **`src/settings_window.rs`**: GTK4 GUI views. Automated testing for GTK widgets requires specialized runners (like `xvfb`) and GTK main-loop integrations.
2.  **`src/api_client.rs`**: Network endpoints. Testing requires mocking `reqwest` clients or firing against a live Immich sandbox instance.
3.  **`src/main.rs` / `src/tray_icon.rs`**: Daemon lifecycle and D-Bus trait integrations. 

## 5. Writing New Tests

When adding a new feature, always consider creating a corresponding inline `#[test]` function.

**Best Practices:**
*   **Never hit the real network:** Use a mock HTTP responder if testing API consumers.
*   **Never modify the real disk:** Use the `tempfile` crate (already in `[dev-dependencies]`) to create temporary, auto-cleaning directories for file I/O tests.
*   **Keep them fast:** Do not inject artificial `tokio::time::sleep()` delays unless absolutely necessary for channel sync tests.
