# FreeWord – notes for Claude

Rust/egui word processor with lossless .docx support. See README.md for features
and the test scripts in `tests/`.

## Session rule: keep TODO.md current

At the end of every work session (before the final summary to the user):

1. Update `TODO.md`: tick off finished items (move them to **Done**), add newly
   discovered work and known bugs, reorder **Next up** by priority.
2. Append a dated line to the **Session log** summarizing what changed and the
   latest test results.
3. Commit and push `TODO.md` together with the session's other changes.

## Building

- GNU toolchain: run `.\build.ps1` (adds WinLibs MinGW to PATH). Debug builds:
  `cargo build` with the MinGW `bin` folder on PATH.
- If `target\release\freeword.exe` is locked because the user has FreeWord open,
  do not close their window; build with `--target-dir target\next` instead.

## Testing before committing .docx changes

- `tests\roundtrip.ps1` and `tests\visual_compare.ps1` (needs Word + Poppler)
- `tests\stress.ps1` for editing changes
- `cargo test`
