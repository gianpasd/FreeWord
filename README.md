# FreeWord

A Microsoft Word–style word processor written in Rust (egui/eframe) that opens,
edits and saves real `.docx` files without losing their content.

## Features

**Editing**
- WYSIWYG page view with margins, ruler, zoom, formatting marks, dark UI
- Character formatting: any installed font (including Office cloud fonts such as
  Aptos), size, bold, italic, underline, strikethrough, sub/superscript, colour,
  highlight, caps/small caps, change case, clear formatting, format painter
- Paragraphs: alignment, indents (left/right/first line/hanging), line spacing
  (multiple, at least, exactly), spacing before/after, tab stops with leaders,
  borders and shading, keep with next / keep together / widow-orphan control
- Styles gallery backed by the document's own Word styles; multi-level bulleted
  and numbered lists using Word numbering definitions
- Tables: insert, rows/columns, delete, borders, shading; table styles with
  header row, banding and first column; merged cells, cell margins, vertical
  alignment; nested tables are displayed
- Pictures (inline and floating, with wrapping), hyperlinks (Ctrl+K, Ctrl+Click),
  page/line/column breaks, symbols, date & time
- Find & replace, word count, undo/redo, rich copy/paste
- **Spell checker**: red underlines, suggestions in the context menu, Spelling
  dialog (F7), Add to Dictionary / Ignore All, language picker (en_US bundled;
  other Hunspell dictionaries from LibreOffice or `%APPDATA%\FreeWord\dictionaries`)
- **Review**: tracked changes (simple markup with change bars, or full markup),
  Accept All / Reject All, comments shown beside the page

**Layout fidelity**
- Headers and footers with tables and pictures, per section, first-page variants,
  live page numbers; edited in place (double-click the top or bottom of a page)
- Footnotes at the bottom of the page, endnotes after the text; click a note to
  edit it, Ctrl+Alt+F / Ctrl+Alt+D inserts a footnote / endnote
- Sections with page/continuous breaks and multiple (balanced) columns
- Floating pictures and shapes positioned relative to page, margin, column,
  paragraph or line

**Files**
- `.docx` open/save with **lossless round-trips**: every part of the package and
  every element FreeWord does not model (fields, bookmarks, comments, revisions,
  equations, charts, SmartArt, content controls...) is preserved; formatting is
  written as differences from the styles
- Native `.fwd`, plain text; export to **PDF** (embedded, subset fonts), HTML;
  print via PDF

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
freeword.exe [file]                              open a document
freeword.exe --convert input output              convert (.docx/.fwd/.txt -> .docx/.fwd/.pdf/.html/.txt)
freeword.exe --stress input output seed steps    random-edit stress test
```

## Tests

The `tests` folder holds Word-driven checks (they need Microsoft Word for COM
automation and Poppler for rendering):

- `make_corpus.ps1` – builds the test documents in `tests/corpus` with Word
- `roundtrip.ps1` – opens each document in FreeWord, saves it, and compares
  Word's statistics of original and result
- `visual_compare.ps1` – renders original and round-tripped files with Word and
  compares the pages pixel by pixel
- `pagination.ps1` – compares FreeWord's page breaks with Word's
- `stress.ps1` – random edits on every corpus document, then checks that Word
  opens every result
- `cargo test` – unit tests

## Not yet implemented

Editing inside nested tables; text-box contents;
right-to-left and complex scripts; hyphenation; table header rows repeating on
new pages; per-change accept/reject; adding new comments.
