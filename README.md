# FreeWord

A Microsoft Word–style word processor written in Rust (egui/eframe).

## Features

- **Pages**: WYSIWYG page view with margins, automatic pagination, ruler, zoom (Ctrl+wheel, slider, One Page / Page Width)
- **Character formatting**: font family (system fonts), size, bold, italic, underline, strikethrough, sub/superscript, font color, highlight, change case, clear formatting, format painter
- **Paragraphs**: left/center/right/justify, line spacing, spacing before/after, indent levels, bulleted and numbered (multi-level) lists, styles (Normal, Title, Subtitle, Heading 1–3, Quote, Code)
- **Insert**: pictures (resizable by dragging the corner handle), page breaks, line breaks, symbols, date & time, header/footer, page numbers
- **Editing**: undo/redo, cut/copy/paste (rich within the app, pictures from the clipboard via the Paste button), find & replace (match case, whole word), word count, formatting marks, right-click menu, AutoFormat (`* ` and `1. ` start lists)
- **Files**: open/save **.docx** (Word), native **.fwd**, **.txt**; export **PDF** (embedded, selectable fonts), HTML; print via PDF; drag & drop files
- Unsaved-changes prompt, dark UI mode, keyboard shortcuts (File → Keyboard Shortcuts)

## Building (Windows)

Uses the Rust GNU toolchain plus MinGW-w64 (for `dlltool`/`as`):

```powershell
winget install Rustlang.Rustup
rustup default stable-x86_64-pc-windows-gnu
winget install BrechtSanders.WinLibs.POSIX.UCRT
.\build.ps1          # release build -> target\release\freeword.exe
.\build.ps1 -Run     # build and run
```

(With the MSVC toolchain and Visual Studio Build Tools, plain `cargo build --release` works too.)

## Command line

```
freeword.exe [file]                         open a document
freeword.exe --convert input output         convert between .fwd/.docx/.txt and .pdf/.docx/.fwd/.html/.txt
```

`samples/Feature Tour.fwd` demonstrates most features.

## Not yet implemented

Tables, spell check, track changes/comments, floating pictures and shapes, columns/sections, footnotes, table of contents, hyperlinks, custom styles, tab stops.
