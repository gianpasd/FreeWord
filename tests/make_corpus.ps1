# Generates .docx test documents with Microsoft Word (COM automation).
# Usage: powershell -File tests\make_corpus.ps1 -OutDir tests\corpus [-Image path.png]
param([string]$OutDir = "$PSScriptRoot\corpus", [string]$Image = "")

New-Item -ItemType Directory -Force $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
$word = New-Object -ComObject Word.Application
$word.Visible = $false
$word.DisplayAlerts = 0

function New-Doc { $d = $word.Documents.Add(); return $d }
function Save-Doc($d, $name) {
    $path = Join-Path $OutDir $name
    $d.SaveAs2([string]$path, 16)
    $d.Close(0)
    Write-Output "wrote $name"
}
function Para($sel, $text, $style = $null) {
    if ($style) { $sel.Style = $style }
    $sel.TypeText($text)
    $sel.TypeParagraph()
}

try {
    # 1. Styles, headings, character formatting
    $d = New-Doc; $s = $word.Selection
    Para $s "Document Title" "Title"
    Para $s "Introduction" "Heading 1"
    $s.Style = "Normal"
    $s.TypeText("Plain text, ")
    $s.Font.Bold = 1; $s.TypeText("bold"); $s.Font.Bold = 0
    $s.TypeText(", ")
    $s.Font.Italic = 1; $s.TypeText("italic"); $s.Font.Italic = 0
    $s.TypeText(", ")
    $s.Font.Underline = 3; $s.TypeText("double underline"); $s.Font.Underline = 0
    $s.TypeText(", ")
    $s.Font.SmallCaps = 1; $s.TypeText("small caps"); $s.Font.SmallCaps = 0
    $s.TypeText(", ")
    $s.Font.AllCaps = 1; $s.TypeText("all caps"); $s.Font.AllCaps = 0
    $s.TypeText(", ")
    $s.Font.Color = 255; $s.TypeText("red"); $s.Font.Color = -16777216
    $s.TypeText(", ")
    $s.Font.Name = "Georgia"; $s.Font.Size = 14; $s.TypeText("Georgia 14"); $s.Font.Reset()
    $s.TypeText(", H"); $s.Font.Subscript = 1; $s.TypeText("2"); $s.Font.Subscript = 0; $s.TypeText("O.")
    $s.TypeParagraph()
    Para $s "Details" "Heading 2"
    $s.Style = "Normal"
    $s.ParagraphFormat.Alignment = 3
    Para $s ("Justified paragraph. " * 12)
    $s.ParagraphFormat.Alignment = 1
    Para $s "Centered paragraph."
    $s.ParagraphFormat.Alignment = 0
    $s.ParagraphFormat.LeftIndent = 36; $s.ParagraphFormat.FirstLineIndent = 18
    Para $s ("Indented with a first-line indent. " * 6)
    $s.ParagraphFormat.LeftIndent = 0; $s.ParagraphFormat.FirstLineIndent = 0
    Para $s "A quotation in the Quote style." "Quote"
    Save-Doc $d "01_styles.docx"

    # 2. Lists
    $d = New-Doc; $s = $word.Selection
    Para $s "Lists" "Heading 1"
    $s.Style = "List Bullet"; Para $s "Bullet one"; Para $s "Bullet two"
    $s.Style = "List Bullet 2"; Para $s "Nested bullet"
    $s.Style = "List Number"; Para $s "Number one"; Para $s "Number two"; Para $s "Number three"
    $s.Style = "Normal"; Para $s "Between lists."
    $s.Range.ListFormat.ApplyNumberDefault(); Para $s "Default numbering A"; Para $s "Default numbering B"
    $s.Range.ListFormat.RemoveNumbers()
    Para $s "End."
    Save-Doc $d "02_lists.docx"

    # 3. Tables
    $d = New-Doc; $s = $word.Selection
    Para $s "Tables" "Heading 1"
    $s.Style = "Normal"
    $t = $d.Tables.Add($s.Range, 4, 3)
    $t.Style = "Table Grid"
    for ($r = 1; $r -le 4; $r++) { for ($c = 1; $c -le 3; $c++) { $t.Cell($r, $c).Range.Text = "R$r C$c" } }
    $t.Rows.Item(1).Range.Font.Bold = 1
    $t.Cell(1, 1).Shading.BackgroundPatternColor = 15773696
    $t.Cell(3, 1).Merge($t.Cell(3, 2))
    $t.Cell(4, 3).Range.Text = "Multi-line cell with enough text to wrap onto several lines inside the table cell."
    $s.EndKey(6) | Out-Null
    $s.TypeParagraph()
    Para $s "Text after the table."
    Save-Doc $d "03_tables.docx"

    # 4. Hyperlinks, footnotes, fields, TOC
    $d = New-Doc; $s = $word.Selection
    Para $s "Contents" "Heading 1"
    $s.Style = "Normal"
    $tocRange = $s.Range
    $d.TablesOfContents.Add($tocRange, $true, 1, 3) | Out-Null
    $s.EndKey(6) | Out-Null
    $s.TypeParagraph()
    Para $s "Chapter One" "Heading 1"
    $s.Style = "Normal"
    $s.TypeText("Visit ")
    $d.Hyperlinks.Add($s.Range, "https://www.rust-lang.org/", "", "", "the Rust website") | Out-Null
    $s.TypeText(" for more. A footnote follows")
    $d.Footnotes.Add($s.Range, [Type]::Missing, "This is the footnote text.") | Out-Null
    $s.EndKey(6) | Out-Null
    $s.TypeText(" and the date field: ")
    $s.Fields.Add($s.Range, 31, "", $false) | Out-Null
    $s.TypeParagraph()
    Para $s "Chapter Two" "Heading 1"
    $s.Style = "Normal"
    Para $s "More text."
    $d.TablesOfContents.Item(1).Update()
    Save-Doc $d "04_fields.docx"

    # 5. Sections, headers/footers, page numbers, landscape
    $d = New-Doc; $s = $word.Selection
    $d.Sections.Item(1).Headers.Item(1).Range.Text = "Header text"
    $d.Sections.Item(1).Footers.Item(1).PageNumbers.Add(1, $true) | Out-Null
    Para $s "First section" "Heading 1"
    $s.Style = "Normal"
    Para $s ("Section one text. " * 30)
    $s.InsertBreak(2)
    Para $s "Second section" "Heading 1"
    $s.Style = "Normal"
    Para $s ("Section two text. " * 30)
    $s.InsertBreak(7)
    Para $s "After a page break."
    Save-Doc $d "05_sections.docx"

    # 6. Pictures, tabs, comments, tracked changes
    $d = New-Doc; $s = $word.Selection
    Para $s "Pictures and tabs" "Heading 1"
    $s.Style = "Normal"
    if ($Image -and (Test-Path $Image)) {
        $s.InlineShapes.AddPicture((Resolve-Path $Image).Path) | Out-Null
        $s.TypeParagraph()
    }
    $s.ParagraphFormat.TabStops.Add(216, 1, 1) | Out-Null
    $s.ParagraphFormat.TabStops.Add(432, 2, 1) | Out-Null
    Para $s "Left`tCentered`tRight"
    $s.ParagraphFormat.TabStops.ClearAll()
    $s.TypeText("Text with a comment.")
    $d.Comments.Add($d.Range($s.Start - 8, $s.Start - 1), "A reviewer comment") | Out-Null
    $s.EndKey(6) | Out-Null
    $s.TypeParagraph()
    $d.TrackRevisions = $true
    $s.TypeText("Tracked insertion.")
    $d.TrackRevisions = $false
    $s.TypeParagraph()
    Para $s "Final paragraph."
    Save-Doc $d "06_objects.docx"

    # 7. Equations, text box, shape, chart, SmartArt, content control
    $d = New-Doc; $s = $word.Selection
    Para $s "Rich objects" "Heading 1"
    $s.Style = "Normal"
    $s.TypeText("Equation: ")
    $r = $s.Range
    $r.Text = "x=(-b+sqrt(b^2-4ac))/2a"
    $d.OMaths.Add($r) | Out-Null
    $d.OMaths.Item(1).BuildUp()
    $s.EndKey(6) | Out-Null
    $s.TypeParagraph()
    $tb = $d.Shapes.AddTextbox(1, 300, 150, 150, 60)
    $tb.TextFrame.TextRange.Text = "Text inside a text box"
    $d.Shapes.AddShape(9, 100, 250, 80, 50) | Out-Null
    $s.EndKey(6) | Out-Null
    try { $d.InlineShapes.AddChart2(-1, 51) | Out-Null } catch { }
    $s.EndKey(6) | Out-Null
    $s.TypeParagraph()
    try { $d.InlineShapes.AddSmartArt($word.Application.SmartArtLayouts.Item(1)) | Out-Null } catch { }
    $s.EndKey(6) | Out-Null
    $s.TypeParagraph()
    $cc = $d.ContentControls.Add(1)
    $cc.Range.Text = "Content control text"
    $d.Content.InsertParagraphAfter()
    $s.EndKey(6) | Out-Null
    Para $s "After the objects."
    Save-Doc $d "07_rich_objects.docx"

    # 8. Nested table, columns, endnotes, first-page header, bookmark + cross reference
    $d = New-Doc; $s = $word.Selection
    $d.PageSetup.DifferentFirstPageHeaderFooter = $true
    $d.Sections.Item(1).Headers.Item(2).Range.Text = "First page header"
    $d.Sections.Item(1).Headers.Item(1).Range.Text = "Other pages header"
    Para $s "Advanced layout" "Heading 1"
    $s.Style = "Normal"
    $t = $d.Tables.Add($s.Range, 2, 2)
    $t.Borders.Enable = $true
    $t.Cell(1, 1).Range.Text = "Outer"
    $inner = $t.Cell(2, 2).Tables.Add($t.Cell(2, 2).Range, 2, 2)
    $inner.Borders.Enable = $true
    $inner.Cell(1, 1).Range.Text = "Inner"
    $s.EndKey(6) | Out-Null
    $s.TypeParagraph()
    $bm = $s.Range
    $s.TypeText("Bookmarked sentence.")
    $d.Bookmarks.Add("Target", $d.Range($bm.Start, $s.Start)) | Out-Null
    $s.TypeText(" See endnote")
    $d.Endnotes.Add($s.Range, [Type]::Missing, "Endnote text.") | Out-Null
    $s.EndKey(6) | Out-Null
    $s.TypeText(". Cross reference: ")
    $s.InsertCrossReference("Bookmark", 1, "Target", $true, $false)
    $s.TypeParagraph()
    $s.InsertBreak(3)
    $d.Sections.Item($d.Sections.Count).PageSetup.TextColumns.SetCount(2)
    Para $s ("Two column text. " * 60)
    $s.ParagraphFormat.LineSpacingRule = 4
    $s.ParagraphFormat.LineSpacing = 18
    Para $s "Exact line spacing paragraph."
    $s.Font.Hidden = 1; $s.TypeText("hidden text"); $s.Font.Hidden = 0
    $s.TypeParagraph()
    Save-Doc $d "08_advanced.docx"

    # 9. Table styles, banding, cell borders/alignment, paragraph borders
    $d = New-Doc; $s = $word.Selection
    Para $s "Styled tables" "Heading 1"
    $s.Style = "Normal"
    $t = $d.Tables.Add($s.Range, 5, 4)
    try { $t.Style = "Grid Table 4 - Accent 1" } catch { $t.Style = "Table Grid" }
    $t.ApplyStyleHeadingRows = $true
    $t.ApplyStyleRowBands = $true
    $t.ApplyStyleFirstColumn = $true
    for ($r = 1; $r -le 5; $r++) { for ($c = 1; $c -le 4; $c++) { $t.Cell($r, $c).Range.Text = if ($r -eq 1) { "Head $c" } else { "R$r C$c" } } }
    $s.EndKey(6) | Out-Null
    $s.TypeParagraph()
    $t2 = $d.Tables.Add($s.Range, 2, 3)
    $t2.Borders.Enable = $false
    $t2.Rows.Item(1).Height = 40
    $t2.Cell(1, 1).Range.Text = "Top"
    $t2.Cell(1, 2).Range.Text = "Middle"
    $t2.Cell(1, 2).VerticalAlignment = 1
    $t2.Cell(1, 3).Range.Text = "Bottom"
    $t2.Cell(1, 3).VerticalAlignment = 3
    $b = $t2.Cell(2, 2).Borders.Item(-3)
    $b.LineStyle = 7; $b.LineWidth = 12; $b.Color = 255
    $t2.Cell(2, 2).Range.Text = "Double red bottom border"
    $s.EndKey(6) | Out-Null
    $s.TypeParagraph()
    $s.TypeText("A boxed paragraph with a shaded background.")
    $s.ParagraphFormat.Borders.Enable = $true
    $s.ParagraphFormat.Shading.BackgroundPatternColor = 14348258
    $s.TypeParagraph()
    $s.ParagraphFormat.Borders.Enable = $false
    $s.ParagraphFormat.Shading.BackgroundPatternColor = -16777216
    Para $s "Plain paragraph after the box."
    Save-Doc $d "09_table_styles.docx"
}
finally {
    $word.Quit()
    [System.Runtime.InteropServices.Marshal]::ReleaseComObject($word) | Out-Null
}



