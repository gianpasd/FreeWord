# Opens every .docx in a folder with Microsoft Word and reports files Word
# rejects, has to repair, crashes on or hangs on (each file gets its own Word
# instance and a time limit).
# Usage: powershell -File tests\validate_word.ps1 -Dir tests\out\stress [-Timeout 60]
param([string]$Dir = "$PSScriptRoot\out\stress", [int]$Timeout = 60)

$check = {
    param($path)
    $word = New-Object -ComObject Word.Application
    $word.Visible = $false
    $word.DisplayAlerts = 0
    try {
        # OpenAndRepair = false: unreadable content makes Open fail instead of silently fixing it.
        $d = $word.Documents.Open($path, $false, $true, $false, "", "", $false, "", "", 0, [Type]::Missing, $false, $false)
        $n = $d.Paragraphs.Count
        $d.Close(0)
        "OK|$n"
    }
    catch {
        "BAD|$($_.Exception.Message)"
    }
    finally {
        try { $word.Quit() } catch {}
    }
}

$bad = 0
foreach ($f in Get-ChildItem $Dir -Filter *.docx) {
    $before = @(Get-Process WINWORD -ErrorAction SilentlyContinue | ForEach-Object Id)
    $job = Start-Job -ScriptBlock $check -ArgumentList $f.FullName
    $result = $null
    if (Wait-Job $job -Timeout $Timeout) {
        $result = Receive-Job $job | Select-Object -Last 1
    }
    Remove-Job $job -Force
    # Word instances started for this file must not outlive it.
    Get-Process WINWORD -ErrorAction SilentlyContinue | Where-Object { $before -notcontains $_.Id } | Stop-Process -Force
    if (-not $result) {
        $bad++
        Write-Output "HANG $($f.Name): no answer from Word within $Timeout s"
    }
    elseif ($result.StartsWith("OK|")) {
        Write-Output "OK   $($f.Name) ($($result.Substring(3)) paragraphs)"
    }
    else {
        $bad++
        Write-Output "BAD  $($f.Name): $($result.Substring(4))"
    }
}
Write-Output "$bad file(s) rejected by Word"
