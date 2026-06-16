param(
    [string]$Title = "",
    [string]$BodyFile = ""
)

$ErrorActionPreference = "Stop"

$AllowedOwner = "hiltonfam"
$AllowedRepo = "Project-Sentinel"
$AllowedBase = "master"
$BlockedOwner = "RCGV1"
$BlockedRepo = "Meshtastic-SAME-EAS-Alerter"
$AllowedBaseRepo = "$AllowedOwner/$AllowedRepo"
$BlockedBaseRepo = "$BlockedOwner/$BlockedRepo"

function Fail($Message) {
    Write-Error $Message
    exit 1
}

function RunStep {
    param(
        [string]$Description,
        [scriptblock]$Command
    )

    Write-Host ""
    Write-Host "==> $Description"
    & $Command
    if ($LASTEXITCODE -ne 0) {
        Fail "$Description failed."
    }
}

function Get-OriginRepo {
    $originUrl = (git remote get-url origin).Trim()

    if ($originUrl -match "github\.com[:/]([^/]+)/([^/\.]+)(\.git)?$") {
        return "$($Matches[1])/$($Matches[2])"
    }

    Fail "Unable to parse origin remote URL: $originUrl"
}

function Get-CompareUrl($Branch) {
    return "https://github.com/$AllowedBaseRepo/compare/$AllowedBase...$Branch`?expand=1"
}

$branch = (git branch --show-current).Trim()
if ([string]::IsNullOrWhiteSpace($branch)) {
    Fail "Unable to determine the current branch."
}

if ($branch -eq $AllowedBase) {
    Fail "Refusing to create a PR from '$AllowedBase'. Create a feature branch first."
}

$originRepo = Get-OriginRepo
if ($originRepo -eq $BlockedBaseRepo) {
    Fail "Refusing to create a PR against upstream $BlockedBaseRepo."
}

if ($originRepo -ne $AllowedBaseRepo) {
    Fail "Refusing to continue. origin must be $AllowedBaseRepo, but found $originRepo."
}

$status = git status --porcelain
if (-not [string]::IsNullOrWhiteSpace($status)) {
    Fail "Working tree is not clean. Commit or stash changes before creating a PR."
}

RunStep "cargo fmt --check" { cargo fmt --check }
RunStep "cargo check" { cargo check }
RunStep "cargo test" { cargo test }
RunStep "Push current branch to origin" { git push -u origin $branch }

$compareUrl = Get-CompareUrl $branch
$gh = Get-Command gh -ErrorAction SilentlyContinue

if ($null -eq $gh) {
    Write-Host ""
    Write-Host "GitHub CLI was not found."
    Write-Host "Open this URL to create the PR manually:"
    Write-Host $compareUrl
    exit 0
}

$ghArgs = @(
    "pr",
    "create",
    "--repo",
    $AllowedBaseRepo,
    "--base",
    $AllowedBase,
    "--head",
    "$AllowedOwner`:$branch"
)

if (-not [string]::IsNullOrWhiteSpace($Title)) {
    $ghArgs += @("--title", $Title)
} else {
    $ghArgs += @("--fill")
}

if (-not [string]::IsNullOrWhiteSpace($BodyFile)) {
    $ghArgs += @("--body-file", $BodyFile)
} elseif (-not [string]::IsNullOrWhiteSpace($Title)) {
    $ghArgs += @("--body", "")
}

Write-Host ""
Write-Host "==> Creating PR against $AllowedBaseRepo`:$AllowedBase"
& gh @ghArgs
if ($LASTEXITCODE -ne 0) {
    Fail "GitHub CLI PR creation failed. Manual fallback URL: $compareUrl"
}
