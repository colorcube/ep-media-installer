# Contributing to `EP Media Installer` (`epmi`)

Thank you for your interest in contributing to `ep-media-installer`! Bug reports, hardware reports on additional SureColor models, documentation improvements, and code contributions are welcome.

---

## 1. Building & Testing Locally

### Prerequisites

- Rust `>= 1.85` (Edition 2024)
- Standard Wayland/X11 and OpenGL development headers for `epmi-gui` (`wayland-devel`, `libxkbcommon-devel`, `mesa-libGL-devel` on Fedora; `libwayland-dev`, `libxkbcommon-dev`, `libgl1-mesa-dev` on Debian/Ubuntu)
- Optional validation tools for desktop integration tests: `desktop-file-validate` (`desktop-file-utils`) and `appstreamcli` (`appstream`)

### Build & Run Checks

Before opening a pull request, make sure all checks enforced by CI (`.github/workflows/ci.yml`) pass cleanly:

```bash
# 1. Check formatting across all workspace crates
cargo fmt --all -- --check

# 2. Run strict Clippy lints (zero warnings, no unwrap/expect/panic/todo/unsafe)
cargo clippy --workspace --all-targets -- -D warnings

# 3. Run the full test suite
cargo test --workspace
```

---

## 2. Code Quality & Clean-Room Guidelines

- **Strict safety & lints:** The workspace enforces `unsafe_code = "forbid"` as well as `unwrap_used = "deny"`, `expect_used = "deny"`, `panic = "deny"`, `todo = "deny"`, and `unimplemented = "deny"`. Handle all errors explicitly via `Result` and `thiserror`.
- **Bilingual UI (`epmi-gui`):** User-facing strings in `apps/epmi-gui/src/i18n.rs` support both English (default) and German (`tr("English", "Deutsch")`). CLI and library messages in `epmi` and `epmi-core` are in English.
- **No proprietary files or personal data:** Never commit `.icc` profiles, `.emx` archives, or personal printer serial numbers / MAC addresses.
- **License:** All contributions are licensed under the **GNU General Public License v3.0 or later (`GPL-3.0-or-later`)**.

---

## 3. Pull Request Workflow

1. Fork the repository and create a topic branch from `main`.
2. Make focused, atomic commits with clear commit messages (e.g. `feat(gui): ...`, `fix(snmp): ...`, `docs: ...`).
3. Verify that `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` pass.
4. Open a Pull Request describing the motivation, changes, and how they were tested.
