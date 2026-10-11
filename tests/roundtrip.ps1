# Round-trips every .docx in a folder through FreeWord and compares the result
# with the original in Microsoft Word (via COM).
# Usage: powershell -File tests\roundtrip.ps1 [-Corpus tests\corpus] [-Exe target\debug\freeword.exe]
param(
    [string]$Corpus = "$PSScriptRoot\corpus",
    [string]$Exe = "$PSScriptRoot\..\target\debug\freeword.exe",
    [string]$OutDir = "$PSScriptRoot\out"
)

New-Item -ItemType Directory -Force $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
$Exe = (Resolve-Path $Exe).Path

function Get-Stats($word, $path) {
    $d = $word.Documents.Open($path, $false, $true, $false)
    try {
        $text = $d.Content.Text -replace "[\r\a\x07]+", "`n"
        [ordered]@{
            Paragraphs  = $d.Paragraphs.Count
            Tables      = $d.Tables.Count
            Cells       = ($d.Tables | ForEach-Object { $_.Range.Cells.Count } | Measure-Object -Sum).Sum
            Pictures    = $d.InlineShapes.Count
            Shapes      = $d.Shapes.Count
            Footnotes   = $d.Footnotes.Count
            Hyperlinks  = $d.Hyperlinks.Count
            Comments    = $d.Comments.Count
            Revisions   = $d.Revisions.Count
            Fields      = $d.Fields.Count
            Sections    = $d.Sections.Count
            Pages       = $d.ComputeStatistics(2)
            Words       = $d.ComputeStatistics(0)
            Bold        = $d.Content.Font.Bold
            Text        = $text
        }
    }
    finally {
        $d.Close(0)
    }
}

$word = New-Object -ComObject Word.Application
$word.Visible = $false
$word.DisplayAlerts = 0
$failures = 0
try {
    foreach ($f in Get-ChildItem $Corpus -Filter *.docx) {
        $out = Join-Path $OutDir ($f.BaseName + ".rt.docx")
        & $Exe --convert $f.FullName $out 2>&1 | Out-Null
        if (-not (Test-Path $out)) {
            Write-Output "FAIL $($f.Name): FreeWord did not produce output"
            $failures++
            continue
        }
        try {
            $b = Get-Stats $word $out
        }
        catch {
            Write-Output "FAIL $($f.Name): Word could not open the result: $($_.Exception.Message)"
            $failures++
            continue
        }
        try {
            $a = Get-Stats $word $f.FullName
        }
        catch {
            # A damaged source file: only check that FreeWord's output opens.
            Write-Output "SKIP $($f.Name): Word rejects the original ($($_.Exception.Message)); the result opens"
            continue
        }
        $diffs = @()
        foreach ($k in $a.Keys) {
            if ($k -eq "Text") { continue }
            if ("$($a[$k])" -ne "$($b[$k])") { $diffs += "$k $($a[$k]) -> $($b[$k])" }
        }
        if ($a.Text -ne $b.Text) {
            $ta = $a.Text; $tb = $b.Text
            $i = 0
            while ($i -lt [Math]::Min($ta.Length, $tb.Length) -and $ta[$i] -eq $tb[$i]) { $i++ }
            $ctxA = $ta.Substring($i, [Math]::Min(40, $ta.Length - $i)) -replace "`n", "\n"
            $ctxB = $tb.Substring($i, [Math]::Min(40, $tb.Length - $i)) -replace "`n", "\n"
            $diffs += "Text differs at $i : '$ctxA' vs '$ctxB'"
        }
        if ($diffs.Count -eq 0) {
            Write-Output "OK   $($f.Name)"
        }
        else {
            $failures++
            Write-Output "DIFF $($f.Name)"
            $diffs | ForEach-Object { Write-Output "       $_" }
        }
    }
}
finally {
    $word.Quit()
    [System.Runtime.InteropServices.Marshal]::ReleaseComObject($word) | Out-Null
}
Write-Output "$failures file(s) with differences"
