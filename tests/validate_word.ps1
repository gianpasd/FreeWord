# Opens every .docx in a folder with Microsoft Word and reports files Word
# rejects or has to repair. Usage: powershell -File tests\validate_word.ps1 -Dir tests\out\stress
param([string]$Dir = "$PSScriptRoot\out\stress")

$word = New-Object -ComObject Word.Application
$word.Visible = $false
$word.DisplayAlerts = 0
$bad = 0
try {
    foreach ($f in Get-ChildItem $Dir -Filter *.docx) {
        try {
            # OpenAndRepair = false: unreadable content makes Open fail instead of silently fixing it.
            $d = $word.Documents.Open($f.FullName, $false, $true, $false, "", "", $false, "", "", 0, [Type]::Missing, $false, $false)
            $n = $d.Paragraphs.Count
            $d.Close(0)
            Write-Output "OK   $($f.Name) ($n paragraphs)"
        }
        catch {
            $bad++
            Write-Output "BAD  $($f.Name): $($_.Exception.Message)"
        }
    }
}
finally {
    $word.Quit()
    [System.Runtime.InteropServices.Marshal]::ReleaseComObject($word) | Out-Null
}
Write-Output "$bad file(s) rejected by Word"
