# FreeWord TODO

Updated at the end of every work session. Newest session notes at the bottom.

## Next up

- [ ] FreeWord's own pagination vs Word on `tests/corpus_ext` (sample-files.com, not in
  git; `tests/pagination.ps1 -Files ...`): basic-text, formatted-report, image-document
  match. large-document 50 vs 49 pages; lists, table-document, template break pages
  elsewhere; multi-column has the right count but 2 page starts differ (column
  balancing at a continuous break puts one more line in column 1; gap before the
  following heading ~4pt too large)
- [ ] Word compat rule: a Normal-style font size of 11/12 pt or left justification is
  overridden by the table style unless `overrideTableStyleFontSizeAndJustification`
  (LibreOffice `ApplyParagraphPropertiesFromTableStyle`); we apply table rPr only to
  default-style paragraphs
- [ ] Text-box contents rendered (wps:txbx) instead of a placeholder box
- [ ] Repeat table header rows (`w:tblHeader`) on each new page; split long rows across pages (`cantSplit`)
- [ ] Per-change Accept / Reject (next/previous change navigation)
- [ ] New comments: insert, reply, delete, resolve (write `comments.xml`)
- [ ] Headers/footers: create one for a section other than the last, or a first-page /
  even-page variant (currently only the last section's default header can be created)
- [ ] Notes: numbering formats (`w:numFmt`) and restart rules; notes whose reference was
  deleted stay in an *unedited* notes part (only rewritten parts are pruned)
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
- [x] Pagination like Word (multi-column 15 -> 46 pages, as Word): paragraph gaps are
  max(space after, space before), space before kept at the document start; font metrics
  no longer pixel-rounded; multiple line spacing's extra may hang into the bottom
  margin; keep-together blocks skip columns too short for them; zero/negative column
  widths give one character per line; no blank line after a trailing page break;
  empty continuous section-break paragraphs take no room at a page top; table rows
  make room for horizontal borders
- [x] Notes without a reference are dropped when a notes part is rewritten (Word
  rejected a stress result with an orphan footnote holding a picture)
- [x] Table-style paragraph properties rank below the paragraph style (as in Word and
  LibreOffice); cell paragraphs no longer get the table style's spacing written as
  direct formatting (large-document lost 5 pages in Word after a round-trip)
- [x] Comment markers without a comment are dropped on save (Word rejects them)
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
- **2026-10-11** – Added `tests/corpus_ext` (9 sample-files.com documents, git-ignored;
  test scripts take `-Corpus`). Fixed table-style vs paragraph-style precedence and
  dropped dangling comment markers; `roundtrip.ps1` reports a damaged original as SKIP.
  Tests: `cargo test` 9/9; round-trip corpus 10/10 and corpus_ext 8/8 OK (+1 SKIP:
  tracked-changes original is rejected by Word, our output opens); visual compare
  identical on both. FreeWord pagination matches Word on 1 of 8 corpus_ext files.
- **2026-10-11** – Pagination fixes found with Word probes (scripts in the session
  scratchpad: spacing, page-break and section-break placement): see Done. Pagination
  vs Word: corpus 8/10 same starts (04/07 differ only in extracted text, as before),
  corpus_ext 3/8 exact and multi-column 46/46 pages. Orphan notes pruned on save.
  Tests: `cargo test` 12/12; round-trip corpus 10/10, corpus_ext 8/8 (+1 SKIP); visual
  compare 10/10 identical; stress 3x200 corpus + 2x200 corpus_ext, all open in Word.
