$allCode = ""
Get-ChildItem -Path "F:\Coding\zeff-boy\src" -Recurse -Filter *.rs | 
Sort-Object FullName | 
ForEach-Object {
    $allCode += "`n// ===== $($_.FullName) =====`n"
    $allCode += Get-Content $_.FullName -Raw
}
Set-Clipboard -Value $allCode