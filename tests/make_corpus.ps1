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
}
finally {
    $word.Quit()
    [System.Runtime.InteropServices.Marshal]::ReleaseComObject($word) | Out-Null
}


