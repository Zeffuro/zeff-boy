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
$query = [Uri]::EscapeDataString(
    'repo:microsoft/winget-pkgs is:pr is:open in:title "Zeffuro.ZeffBoy"'
)
$headers = @{
    Accept = "application/vnd.github+json"
    Authorization = "Bearer $env:WINGET_CREATE_GITHUB_TOKEN"
    "User-Agent" = "zeff-boy-winget"
    "X-GitHub-Api-Version" = "2022-11-28"
}
$openPullRequests = Invoke-RestMethod `
    -Uri "https://api.github.com/search/issues?q=$query&per_page=1" `
    -Headers $headers
if ($openPullRequests.total_count -gt 0) {
    $existingPr = $openPullRequests.items[0]
    Write-Host (
        "An open WinGet pull request already exists for Zeffuro.ZeffBoy: " +
        "#$($existingPr.number) $($existingPr.html_url)"
    )
    Write-Host "Skipping WinGet submission."
    exit 0
}

$authenticatedUser = Invoke-RestMethod `
    -Uri "https://api.github.com/user" `
    -Headers $headers
$wingetFork = Invoke-RestMethod `
    -Uri "https://api.github.com/repos/$($authenticatedUser.login)/winget-pkgs" `
    -Headers $headers
$expectedUpstream = "microsoft/winget-pkgs"
if (
    -not $wingetFork.fork -or
    -not $wingetFork.parent -or
    -not [string]::Equals($wingetFork.parent.full_name, $expectedUpstream, [StringComparison]::OrdinalIgnoreCase)
) {
    throw "GitHub account '$($authenticatedUser.login)' does not own a winget-pkgs fork of $expectedUpstream"
}
if (-not [string]::Equals($wingetFork.owner.login, $authenticatedUser.login, [StringComparison]::OrdinalIgnoreCase)) {
    throw "GitHub returned a winget-pkgs fork owned by '$($wingetFork.owner.login)', not authenticated account '$($authenticatedUser.login)'"
}

$upstreamBranch = $wingetFork.parent.default_branch
$forkBranch = $wingetFork.default_branch
$upstreamRef = Invoke-RestMethod `
    -Uri "https://api.github.com/repos/$($wingetFork.parent.full_name)/git/ref/heads/$upstreamBranch" `
    -Headers $headers
$upstreamSha = $upstreamRef.object.sha
if ([string]::IsNullOrWhiteSpace($upstreamSha)) {
    throw "Could not determine the current $expectedUpstream $upstreamBranch commit"
}
$compareUri = "https://api.github.com/repos/$($wingetFork.parent.full_name)/compare/$upstreamSha...$($wingetFork.owner.login):$forkBranch"
$forkComparison = Invoke-RestMethod -Uri $compareUri -Headers $headers
if ($forkComparison.ahead_by -gt 0) {
    throw (
        "WinGet fork '$($wingetFork.full_name)' is $($forkComparison.ahead_by) commit(s) ahead of " +
        "$expectedUpstream. Refusing to sync it automatically; resolve the fork divergence first."
    )
}
if ($forkComparison.behind_by -gt 0) {
    Write-Host "Fast-forwarding WinGet fork '$($wingetFork.full_name)' to $expectedUpstream commit $upstreamSha"
    try {
        $updatedForkRef = Invoke-RestMethod `
            -Uri "https://api.github.com/repos/$($wingetFork.full_name)/git/refs/heads/$forkBranch" `
            -Method Patch `
            -Headers $headers `
            -ContentType "application/json" `
            -Body (@{ sha = $upstreamSha; force = $false } | ConvertTo-Json -Compress)
    } catch {
        $responseProperty = $_.Exception.PSObject.Properties['Response']
        $syncStatus = if ($null -ne $responseProperty -and $null -ne $responseProperty.Value) {
            [int]$responseProperty.Value.StatusCode
        } else {
            $null
        }
        if ($syncStatus -eq 401 -or $syncStatus -eq 403) {
            throw "GitHub denied the non-force WinGet fork update (HTTP $syncStatus). Confirm WINGET_CREATE_GITHUB_TOKEN can write '$($wingetFork.full_name)'."
        }
        if ($syncStatus -eq 409 -or $syncStatus -eq 422) {
            throw "GitHub rejected the non-force WinGet fork update (HTTP $syncStatus). The fork branch may have changed or be protected; inspect it before retrying."
        }
        throw "Could not non-force update WinGet fork '$($wingetFork.full_name)' with ${expectedUpstream}: $($_.Exception.Message)"
    }

    if (-not [string]::Equals($updatedForkRef.object.sha, $upstreamSha, [StringComparison]::OrdinalIgnoreCase)) {
        throw "WinGet fork '$($wingetFork.full_name)' update did not reach pinned $expectedUpstream commit $upstreamSha"
    }
}

$toolRoot = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }
$wingetCreate = Join-Path $toolRoot "wingetcreate.exe"
Invoke-WebRequest "https://aka.ms/wingetcreate/latest" -OutFile $wingetCreate

$manifestRoot = Join-Path $toolRoot "zeff-boy-winget-$([guid]::NewGuid().ToString('N'))"
& python scripts/generate-winget.py $versionNumber $manifestRoot
if ($LASTEXITCODE -ne 0) {
    throw "WinGet manifest generation failed with exit code $LASTEXITCODE"
}

$manifestDirectory = Join-Path `
    $manifestRoot `
    "manifests/z/Zeffuro/ZeffBoy/$versionNumber"
& $wingetCreate submit `
    --prtitle $prTitle `
    --token $env:WINGET_CREATE_GITHUB_TOKEN `
    --no-open `
    $manifestDirectory
if ($LASTEXITCODE -ne 0) {
    throw "WingetCreate manifest submission failed with exit code $LASTEXITCODE"
}
