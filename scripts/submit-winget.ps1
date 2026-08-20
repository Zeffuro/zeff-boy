[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $Version
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($env:WINGET_CREATE_GITHUB_TOKEN)) {
    throw "WINGET_CREATE_GITHUB_TOKEN is not configured"
}

$versionNumber = $Version -replace '^v', ''
if ($versionNumber -notmatch '^\d+(?:\.\d+){2}(?:[-+][0-9A-Za-z.-]+)?$') {
    throw "Unsupported WinGet package version: $versionNumber"
}

$manifestUrl = "https://raw.githubusercontent.com/microsoft/winget-pkgs/master/manifests/z/Zeffuro/ZeffBoy/${versionNumber}/Zeffuro.ZeffBoy.yaml"
try {
    $manifestStatus = (Invoke-WebRequest $manifestUrl -Method Head).StatusCode
} catch {
    if ($null -eq $_.Exception.Response) {
        throw
    }
    $manifestStatus = [int]$_.Exception.Response.StatusCode
}
if ($manifestStatus -eq 200) {
    Write-Host "WinGet manifest $versionNumber is already published"
    exit 0
}
if ($manifestStatus -ne 404) {
    throw "Could not check published WinGet manifest: HTTP $manifestStatus"
}

$prTitle = "Update: Zeffuro.ZeffBoy to $versionNumber"
$query = [Uri]::EscapeDataString("repo:microsoft/winget-pkgs is:pr is:open in:title `"$prTitle`"")
$headers = @{ Accept = "application/vnd.github+json"; "User-Agent" = "zeff-boy-winget" }
$openPullRequests = Invoke-RestMethod "https://api.github.com/search/issues?q=$query" -Headers $headers
if ($openPullRequests.total_count -gt 0) {
    Write-Host "An open WinGet pull request already exists for $versionNumber"
    exit 0
}

$toolRoot = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }
$wingetCreate = Join-Path $toolRoot "wingetcreate.exe"
Invoke-WebRequest "https://aka.ms/wingetcreate/latest" -OutFile $wingetCreate

$installerUrl = "https://github.com/Zeffuro/zeff-boy/releases/download/v${versionNumber}/zeff-boy-v${versionNumber}-x86_64-pc-windows-msvc.zip"
$releaseNotes = "https://github.com/Zeffuro/zeff-boy/releases/tag/v${versionNumber}"
& $wingetCreate update Zeffuro.ZeffBoy `
    --version $versionNumber `
    --urls "${installerUrl}|x64" `
    --release-notes-url $releaseNotes `
    --prtitle $prTitle `
    --submit `
    --no-open
if ($LASTEXITCODE -ne 0) {
    throw "WingetCreate failed with exit code $LASTEXITCODE"
}
