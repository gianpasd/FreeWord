# Randomized edit stress test: applies random editor commands to every corpus
# document with several seeds, then checks that Word opens every result.
# Usage: powershell -File tests\stress.ps1 [-Seeds 5] [-Steps 300] [-Corpus tests\corpus]
param([int]$Seeds = 5, [int]$Steps = 300, [string]$Exe = "$PSScriptRoot\..\target\debug\freeword.exe", [string]$Corpus = "$PSScriptRoot\corpus")

$out = Join-Path $PSScriptRoot "out\stress"
New-Item -ItemType Directory -Force $out | Out-Null
$fails = 0
foreach ($f in Get-ChildItem $Corpus -Filter *.docx) {
    foreach ($seed in 1..$Seeds) {
        $dst = Join-Path $out "$($f.BaseName).s$seed.docx"
        $r = & $Exe --stress $f.FullName $dst $seed $Steps 2>&1 | Out-String
        if ($LASTEXITCODE -ne 0) {
            $fails++
            Write-Output "FAIL $($f.Name) seed $seed"
            Write-Output $r
        }
    }
}
Write-Output "$fails FreeWord failure(s); validating results in Word..."
& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "validate_word.ps1") -Dir $out
