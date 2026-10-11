# FreeWord TODO

Updated at the end of every work session. Newest session notes at the bottom.

## Next up

- [ ] Text-box contents rendered (wps:txbx) instead of a placeholder box
- [ ] Repeat table header rows (`w:tblHeader`) on each new page; split long rows across pages (`cantSplit`)
- [ ] Per-change Accept / Reject (next/previous change navigation)
- [ ] New comments: insert, reply, delete, resolve (write `comments.xml`)
- [ ] Headers/footers: create one for a section other than the last, or a first-page /
  even-page variant (currently only the last section's default header can be created)
- [ ] Notes: deleting a reference leaves its note in `footnotes.xml` (harmless, but
  should be removed); note numbering formats (`w:numFmt`) and restart rules
- [ ] `.fwd` files: hyperlinks/pictures added to edited notes or new headers lose their
  relationships when reopened (the .docx path is fine)
- [ ] Notes and nested-table UI: clicking into a footnote/endnote/nested table and the
  highlight band were only covered by unit tests, not tried interactively yet
- [ ] Nested tables: tables nested two levels deep, and nested tables inside headers,
  notes or text boxes, are still display-only; a nested table split across pages
  is drawn on one page
- [ ] Pasting objects with relationships (charts, SmartArt) between body, headers and
  notes drops them; copy their target parts instead

## .docx fidelity

- [ ] Text wrapping beside floating objects (square/tight) instead of only above/below
- [ ] Floats inside headers/footers relative to the page when the footer is offset
- [ ] Different odd/even headers (`w:evenAndOddHeaders`)
- [ ] Per-section page size/orientation/margins (only the last section's setup is used)
- [ ] Line numbering, page borders, watermarks, page background colour
- [ ] Underline styles (double, dotted, wavy...) and underline colour display
- [ ] Theme colours with tint/shade applied to text (`w:themeColor`)
- [ ] Character scaling (`w:w`), raised/lowered position (`w:position`), kerning
- [ ] Emphasis marks, outline/shadow/emboss text effects
- [ ] Image cropping (`a:srcRect`), rotation and flips
- [ ] Charts and SmartArt rendered from their fallback images
- [ ] Equations rendered as math (OMML) rather than plain text
- [ ] Field updates: TOC, PAGEREF, REF, SEQ, DATE (currently cached results are shown)
- [ ] Content controls UI (checkboxes, drop-downs, date pickers)
- [ ] Right-to-left and complex scripts (Arabic, Hebrew, Indic) with shaping
- [ ] Hyphenation (`w:autoHyphenation`)
- [ ] Table autofit/percentage widths (`tblW` pct, `tcW` auto) instead of grid only
- [ ] `.dotx` templates, `.docm` (preserve macros), legacy `.doc` import
- [ ] Strict schema validation in tests (Open XML SDK or similar), not just "Word opens it"

## Editing & UI

- [ ] Page Setup per section; insert section breaks from the UI
- [ ] Custom tab stops on the ruler; drag indents on the ruler
- [ ] Style editor (modify/create styles), style pane
- [ ] Navigation pane (headings outline)
- [ ] Insert table of contents
- [ ] Image paste via Ctrl+V (currently only via the Paste button), drag to move pictures
- [ ] Autosave and crash recovery
- [ ] Recent files list
- [ ] Grammar checking
- [ ] Print dialog (currently prints via PDF viewer)

## Engineering

- [ ] Remove `windows_subsystem` console limitation for `--convert` in release builds (output not visible)
- [ ] Incremental layout (only re-lay out changed paragraphs) for long documents
- [ ] More unit tests for doc/numbering/styles; CI on GitHub Actions (MSVC toolchain)
- [ ] Larger real-world corpus (Google Docs / LibreOffice exports, long reports)

## Done

- [x] Edit headers/footers in place (double-click, Esc to leave; Insert > Header/Footer;
  PAGE/NUMPAGES fields); new headers for documents without one
- [x] Edit nested tables in place (click into one; Esc or click outside to leave);
  rewritten on save with their own relationships/media; kept in `.fwd` files
- [x] `.fwd` save failed for any table with cell properties (tuple map keys in JSON)
- [x] Edit footnotes/endnotes in place (click a note); Insert Footnote/Endnote
  (Ctrl+Alt+F / Ctrl+Alt+D); notes parts rewritten or created on save

- [x] Word-style ribbon, paginated WYSIWYG editing, formatting, lists, styles, pictures
- [x] PDF export with embedded subset fonts; HTML/TXT export
- [x] Lossless .docx round-trips (package parts, unknown XML, fields, bookmarks, revisions...)
- [x] Style/theme/numbering resolution; formatting written as differences from styles
- [x] Tables with styles, borders, shading, merges, margins, vertical alignment; nested tables displayed
- [x] Rich headers/footers per section with first-page variants and live page numbers
- [x] Footnotes and endnotes; multi-column sections with balancing
- [x] Floating pictures/shapes positioned like Word, text kept clear above/below
- [x] Keep with next, keep lines together, widow/orphan control (pagination matches Word on the corpus)
- [x] Spell checker (Hunspell dictionaries, suggestions, F7 dialog, user dictionary)
- [x] Tracked changes display, Accept All / Reject All; comments shown beside the page
- [x] Index of all installed fonts incl. Office cloud fonts (Aptos), loaded on demand
- [x] Word-driven tests: round-trip, visual compare, pagination, random-edit stress

## Session log

- **2026-10-11** – Initial app; .docx lossless round-trips; tables; spell checker;
  table styles; footnotes/endnotes; sections & columns; floating objects;
  pagination rules; nested tables; tracked changes & comments; stress testing.
  All 10 corpus documents round-trip pixel-identically; 80 stress runs × 400
  edits open cleanly in Word.
- **2026-10-11** – Header/footer editing in place (story mode: double-click, Esc,
  contextual ribbon group, PAGE/NUMPAGES fields, new headers for docs without
  one); footnote/endnote editing and Insert Footnote/Endnote; edited parts
  rewritten on save, each header/notes part scoped for raw XML so pasted charts
  can't point at the wrong relationships; duplicate note references dropped on
  save. `validate_word.ps1` now uses one Word instance per file with a timeout
  (reports HANG). Tests: `cargo test` 7/7; corpus round-trip and visual compare
  unchanged (10/10 identical); 80 stress runs × 400 edits (now incl. header and
  note edits) + 24 header/note outputs all open in Word.
- **2026-10-11** – Nested tables editable in place as a story (single click opens,
  Esc/click outside closes, one undo step); edited nested tables re-serialized on
  save through the main writer (shared rel ids, separate media names) and stored
  in `.fwd`, which now also rebuilds nested tables on load. Fixed `.fwd` save of
  tables with cell properties. Tests: `cargo test` 8/8; round-trip 10/10 OK;
  visual compare 10/10 identical; all `tests/out/hf` outputs (incl. nested edit)
  open in Word; short stress run (3 × 200).
