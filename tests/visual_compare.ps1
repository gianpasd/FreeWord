# Renders original and round-tripped documents with Word, rasterizes them with
# pdftoppm and reports how many pixels differ per page.
# Usage: powershell -File tests\visual_compare.ps1 [-Corpus tests\corpus] [-OutDir tests\out]
param(
    [string]$Corpus = "$PSScriptRoot\corpus",
    [string]$OutDir = "$PSScriptRoot\out",
    [string]$Pdftoppm = ""
)

if (-not $Pdftoppm) {
    $Pdftoppm = (Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Recurse -Filter pdftoppm.exe -ErrorAction SilentlyContinue | Select-Object -First 1).FullName
}
if (-not $Pdftoppm) { throw "pdftoppm not found (install Poppler)" }
Add-Type -AssemblyName System.Drawing
New-Item -ItemType Directory -Force $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path

function Diff-Images($a, $b) {
    $ia = [System.Drawing.Bitmap]::FromFile($a)
    $ib = [System.Drawing.Bitmap]::FromFile($b)
    try {
        if ($ia.Width -ne $ib.Width -or $ia.Height -ne $ib.Height) { return 100.0 }
        $rect = New-Object System.Drawing.Rectangle 0, 0, $ia.Width, $ia.Height
        $da = $ia.LockBits($rect, 'ReadOnly', 'Format24bppRgb')
        $db = $ib.LockBits($rect, 'ReadOnly', 'Format24bppRgb')
        $n = $da.Stride * $ia.Height
        $ba = New-Object byte[] $n; $bb = New-Object byte[] $n
        [Runtime.InteropServices.Marshal]::Copy($da.Scan0, $ba, 0, $n)
        [Runtime.InteropServices.Marshal]::Copy($db.Scan0, $bb, 0, $n)
        $ia.UnlockBits($da); $ib.UnlockBits($db)
        $diff = 0
        for ($i = 0; $i -lt $n; $i += 3) {
            if ([Math]::Abs($ba[$i] - $bb[$i]) -gt 40 -or [Math]::Abs($ba[$i + 1] - $bb[$i + 1]) -gt 40 -or [Math]::Abs($ba[$i + 2] - $bb[$i + 2]) -gt 40) { $diff++ }
        }
        return [Math]::Round(100.0 * $diff / ($n / 3), 3)
    }
    finally { $ia.Dispose(); $ib.Dispose() }
}

$word = New-Object -ComObject Word.Application
$word.Visible = $false
$word.DisplayAlerts = 0
try {
    foreach ($f in Get-ChildItem $Corpus -Filter *.docx) {
        $rt = Join-Path $OutDir ($f.BaseName + ".rt.docx")
        if (-not (Test-Path $rt)) { continue }
        $results = @()
        foreach ($pair in @(@($f.FullName, "a"), @($rt, "b"))) {
            $pdf = Join-Path $OutDir ($f.BaseName + "." + $pair[1] + ".pdf")
            $d = $word.Documents.Open($pair[0], $false, $true, $false)
            $d.SaveAs2([string]$pdf, 17)
            $d.Close(0)
            & $Pdftoppm -r 40 -png $pdf (Join-Path $OutDir ($f.BaseName + "." + $pair[1]))
        }
        $pagesA = Get-ChildItem $OutDir -Filter "$($f.BaseName).a-*.png" | Sort-Object Name
        $pagesB = Get-ChildItem $OutDir -Filter "$($f.BaseName).b-*.png" | Sort-Object Name
        if ($pagesA.Count -ne $pagesB.Count) {
            Write-Output "DIFF $($f.Name): page count $($pagesA.Count) -> $($pagesB.Count)"
        }
        for ($i = 0; $i -lt [Math]::Min($pagesA.Count, $pagesB.Count); $i++) {
            $results += Diff-Images $pagesA[$i].FullName $pagesB[$i].FullName
        }
        $worst = ($results | Measure-Object -Maximum).Maximum
        $status = if ($worst -lt 0.05) { "SAME" } else { "DIFF" }
        Write-Output ("{0} {1}: worst page differs by {2}% ({3} pages)" -f $status, $f.Name, $worst, $results.Count)
    }
}
finally {
    $word.Quit()
    [System.Runtime.InteropServices.Marshal]::ReleaseComObject($word) | Out-Null
}
