$allCode = ""
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Get-ChildItem -Path (Join-Path $repoRoot "crates") -Recurse -Filter *.rs |
Sort-Object FullName | 
ForEach-Object {
    $allCode += "`n// ===== $($_.FullName) =====`n"
    $allCode += Get-Content $_.FullName -Raw
}
Set-Clipboard -Value $allCode
