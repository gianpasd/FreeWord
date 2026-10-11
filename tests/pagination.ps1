# Compares pagination of FreeWord's PDF export with Word's for each .docx:
# page count and the first words of every page.
# Usage: powershell -File tests\pagination.ps1 [-Files a.docx,b.docx]
param([string[]]$Files = @(), [string]$Exe = "$PSScriptRoot\..\target\debug\freeword.exe")

if ($Files.Count -eq 1 -and $Files[0].Contains(",")) { $Files = $Files[0].Split(",") }
if ($Files.Count -eq 0) { $Files = (Get-ChildItem "$PSScriptRoot\corpus" -Filter *.docx).FullName }
$out = Join-Path $PSScriptRoot "out\pagination"
New-Item -ItemType Directory -Force $out | Out-Null
$bin = (Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Recurse -Filter pdftotext.exe -ErrorAction SilentlyContinue | Select-Object -First 1).DirectoryName
if (-not $bin) { throw "Poppler (pdftotext) not found" }

function Page-Starts($pdf) {
    $n = [int]((& "$bin\pdfinfo.exe" $pdf | Select-String "^Pages:") -replace "\D", "")
    $starts = @()
    for ($p = 1; $p -le $n; $p++) {
        $t = (& "$bin\pdftotext.exe" -f $p -l $p -layout $pdf - | Out-String) -replace "\s+", " "
        $starts += $t.Trim().Substring(0, [Math]::Min(40, $t.Trim().Length))
    }
    return , $starts
}

$word = New-Object -ComObject Word.Application
$word.Visible = $false
$word.DisplayAlerts = 0
$same = 0
try {
    foreach ($f in $Files) {
        $name = [IO.Path]::GetFileNameWithoutExtension($f)
        $fw = Join-Path $out "$name.fw.pdf"
        $wd = Join-Path $out "$name.word.pdf"
        & $Exe --convert $f $fw 2>&1 | Out-Null
        $d = $word.Documents.Open($f, $false, $true, $false)
        $d.SaveAs2([string]$wd, 17)
        $d.Close(0)
        $a = Page-Starts $wd
        $b = Page-Starts $fw
        $key = { param($s) ($s -replace "[^A-Za-z0-9]", "").Substring(0, [Math]::Min(18, ($s -replace "[^A-Za-z0-9]", "").Length)) }
        $breaks = $a.Count -eq $b.Count -and (0..($a.Count - 1) | Where-Object { (& $key $a[$_]) -ne (& $key $b[$_]) }).Count -eq 0
        if ($breaks) {
            $same++
            Write-Output ("SAME {0}: {1} page(s), same page starts" -f $name, $a.Count)
        } else {
            Write-Output ("DIFF {0}: Word {1} page(s), FreeWord {2}" -f $name, $a.Count, $b.Count)
            for ($i = 0; $i -lt [Math]::Max($a.Count, $b.Count); $i++) {
                Write-Output ("       p{0}  Word: {1}" -f ($i + 1), $(if ($i -lt $a.Count) { $a[$i] } else { "-" }))
                Write-Output ("           FW:   {0}" -f $(if ($i -lt $b.Count) { $b[$i] } else { "-" }))
            }
        }
    }
}
finally {
    $word.Quit()
    [System.Runtime.InteropServices.Marshal]::ReleaseComObject($word) | Out-Null
}
Write-Output "$same of $($Files.Count) with the same page count"
