# FreeWord TODO

Updated at the end of every work session. Newest session notes at the bottom.

## Next up

- [ ] Editing inside headers/footers (double-click to edit, like Word)
- [ ] Editing inside footnotes/endnotes; Insert Footnote / Insert Endnote
- [ ] Editing inside nested tables
- [ ] Text-box contents rendered (wps:txbx) instead of a placeholder box
- [ ] Repeat table header rows (`w:tblHeader`) on each new page; split long rows across pages (`cantSplit`)
- [ ] Per-change Accept / Reject (next/previous change navigation)
- [ ] New comments: insert, reply, delete, resolve (write `comments.xml`)

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
