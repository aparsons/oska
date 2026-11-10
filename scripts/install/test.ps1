# Tests for the PowerShell installer. Run on Windows, in PowerShell 7:
#
#   pwsh scripts/install/test.ps1
#
# Each test builds a local release fixture in its own scratch directory: a
# site tree laid out like https://oskalang.dev (releases/vX.Y.Z/ with fake
# binaries, SHA256SUMS signed by a throwaway key, latest.json), the
# installer pinned to that key the way scripts/install/pin.sh pins it, and
# an OSKA_HOME. Every test runs once under each PowerShell it finds:
# Windows PowerShell 5.1 (`powershell`) and PowerShell 7 (`pwsh`).

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

$here = $PSScriptRoot
$scratch = Join-Path ([IO.Path]::GetTempPath()) ('oska-install-test-' + [Guid]::NewGuid().ToString('N'))
[void](New-Item -ItemType Directory -Path $scratch)

$Assets = @('oskac-linux-aarch64', 'oskac-linux-x86_64', 'oskac-macos-aarch64', 'oskac-macos-x86_64', 'oskac-windows-x86_64.exe')
$Utf8 = New-Object System.Text.UTF8Encoding $false

function Assert([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

# .NET resolves relative paths against the process directory, which
# Set-Location does not move, so resolve them against the location first.
function Get-FullPath([string]$Path) {
    $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
}

function Write-Text([string]$Path, [string]$Text) {
    [IO.File]::WriteAllText((Get-FullPath $Path), $Text, $Utf8)
}

function Get-Sha256([string]$Path) {
    (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

function New-Key([string]$Name) {
    & ssh-keygen -q -t ed25519 -N '' -C $Name -f $Name
    Assert ($LASTEXITCODE -eq 0) "ssh-keygen could not make the key $Name"
}

function Set-Signature([string]$Dir, [string]$Key = 'key', [string]$Namespace = 'oska-release') {
    Remove-Item -LiteralPath "$Dir/SHA256SUMS.sig" -ErrorAction SilentlyContinue
    & ssh-keygen -q -Y sign -f $Key -n $Namespace "$Dir/SHA256SUMS"
    Assert ($LASTEXITCODE -eq 0) "ssh-keygen could not sign $Dir/SHA256SUMS"
}

# Publishes release <version> in site/releases/v<version>/, as
# scripts/release/assemble.sh and sign.sh lay it out.
function New-Release([string]$Version) {
    if (-not (Test-Path key)) { New-Key 'key' }
    $dir = "site/releases/v$Version"
    [void](New-Item -ItemType Directory -Force -Path $dir)
    $sums = ''
    foreach ($asset in $Assets) {
        Write-Text "$dir/$asset" "binary $asset $Version`n"
        $sums += '{0}  {1}' -f (Get-Sha256 "$dir/$asset"), $asset
        $sums += "`n"
    }
    Write-Text "$dir/SHA256SUMS" $sums
    Set-Signature $dir
}

# Points site/latest.json at release <version>, in the shape
# scripts/release/stage-site.sh writes.
function Set-Latest([string]$Version) {
    $url = "https://oskalang.dev/releases/v$Version"
    Write-Text 'site/latest.json' (@"
{
  "docs": "main",
  "version": "$Version",
  "tag": "v$Version",
  "release_url": "$url/",
  "checksums_url": "$url/SHA256SUMS",
  "signature_url": "$url/SHA256SUMS.sig",
  "signing_key_url": "https://oskalang.dev/oska-release.pub",
  "signing_namespace": "oska-release",
  "installers": {"sh": "https://oskalang.dev/install.sh", "ps1": "https://oskalang.dev/install.ps1"},
  "binaries": {"windows-x86_64": "$url/oskac-windows-x86_64.exe"}
}
"@)
}

# Writes pinned/install.ps1, pinned to key.pub as pin.sh pins it.
function Get-Pinned {
    if (-not (Test-Path pinned/install.ps1)) {
        if (-not (Test-Path key)) { New-Key 'key' }
        $parts = (Get-Content -Raw key.pub).Trim() -split ' '
        $source = [IO.File]::ReadAllText("$here/install.ps1")
        [void](New-Item -ItemType Directory -Force -Path pinned)
        Write-Text 'pinned/install.ps1' $source.Replace('@OSKA_RELEASE_KEY@', "$($parts[0]) $($parts[1])")
    }
    (Resolve-Path pinned/install.ps1).Path
}

function Get-SiteUrl {
    ([Uri](Join-Path (Get-Location).Path 'site')).AbsoluteUri
}

# Runs an installer file under the PowerShell being tested. Returns its
# stdout lines; the exit status is in $script:status.
function Invoke-Installer([string[]]$Arguments, [string]$Installer = (Get-Pinned), [string]$Site = (Get-SiteUrl)) {
    $env:OSKA_SITE_URL = $Site
    $env:OSKA_HOME = Join-Path (Get-Location).Path 'home\.oska'
    $errFile = Join-Path (Get-Location).Path 'installer-stderr.txt'
    $out = & $script:shell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $Installer @Arguments 2>$errFile
    $script:status = $LASTEXITCODE
    $script:stdout = @($out) -join "`n"
    $script:stderr = Get-Content -Raw -LiteralPath $errFile -ErrorAction SilentlyContinue
    , @($out)
}

function Read-Json([object[]]$Lines) {
    Assert ($Lines.Count -eq 1) "expected one line of JSON, got $($Lines.Count): $($Lines -join "`n")"
    $Lines[0] | ConvertFrom-Json
}

function Get-Installed { Join-Path (Get-Location).Path 'home\.oska\bin\oskac.exe' }

function Test-InstalledIs([string]$Version) {
    $installed = Get-Installed
    Assert (Test-Path $installed) "nothing installed at $installed"
    Assert ((Get-Sha256 $installed) -eq (Get-Sha256 "site/releases/v$Version/oskac-windows-x86_64.exe")) "the installed oskac is not $Version"
    Assert ((Get-Content home/.oska/version) -eq $Version) "the receipt does not say $Version"
}

function Test-InstallsTheLatestReleaseAndReportsItAsJson {
    New-Release '1.2.3'
    Set-Latest '1.2.3'
    $result = Read-Json (Invoke-Installer @('--json'))
    Assert ($script:status -eq 0) "exited $script:status"
    Test-InstalledIs '1.2.3'
    $expected = [ordered]@{
        ok = $true; action = 'installed'; version = '1.2.3'; tag = 'v1.2.3'; previous_version = $null
        platform = 'windows-x86_64'; path = (Get-Installed)
        bin_dir = (Join-Path (Get-Location).Path 'home\.oska\bin'); on_path = $false
        checksum_verified = $true; signature_verified = $true
    }
    $names = @($result.PSObject.Properties | ForEach-Object { $_.Name })
    Assert (($names -join ',') -eq (@($expected.Keys) -join ',')) "fields are $($names -join ',')"
    foreach ($k in $expected.Keys) {
        $actual = $result.$k
        Assert ($actual -eq $expected[$k] -and ($null -eq $actual) -eq ($null -eq $expected[$k])) "$k is '$actual', not '$($expected[$k])'"
    }
}

function Test-ReRunningUpdatesInPlace {
    New-Release '1.2.3'
    Set-Latest '1.2.3'
    [void](Invoke-Installer @('--json'))
    New-Release '1.3.0'
    Set-Latest '1.3.0'
    $result = Read-Json (Invoke-Installer @('-Json'))
    Assert ($script:status -eq 0) "exited $script:status"
    Test-InstalledIs '1.3.0'
    Assert ($result.action -eq 'updated') "action is $($result.action)"
    Assert ($result.previous_version -eq '1.2.3') "previous_version is $($result.previous_version)"
    $result = Read-Json (Invoke-Installer @('--json'))
    Assert ($result.action -eq 'reinstalled') "same version: action is $($result.action)"
    $left = @(Get-ChildItem -Force home/.oska/bin | ForEach-Object { $_.Name })
    Assert (($left -join ',') -eq 'oskac.exe') "bin holds $($left -join ',')"
}

function Test-VersionInstallsThatReleaseRatherThanTheLatest {
    New-Release '1.2.3'
    New-Release '1.3.0'
    Set-Latest '1.3.0'
    $out = Invoke-Installer @('--version', '1.2.3')
    Assert ($script:status -eq 0) "exited $script:status"
    Test-InstalledIs '1.2.3'
    Assert (($out -join "`n") -like "*Installed oskac 1.2.3 to $(Get-Installed)*") "no human summary in: $out"
    Assert (($out -join "`n") -like '*is not on your PATH*') "no PATH hint in: $out"
    [void](Invoke-Installer @('-Version', 'v1.3.0', '-Json'))
    Test-InstalledIs '1.3.0'
}

function Install-Good {
    New-Release '1.2.3'
    Set-Latest '1.2.3'
    [void](Invoke-Installer @('--json'))
    Test-InstalledIs '1.2.3'
    New-Release '1.3.0'
}

# The tampered 1.3.0 must be refused with <code> and leave 1.2.3 installed.
function Test-Refused([string]$Code, [string]$Site = (Get-SiteUrl)) {
    $result = Read-Json (Invoke-Installer @('--json', '--version', '1.3.0') -Site $Site)
    Assert ($script:status -ne 0) 'installed a tampered release'
    Assert ($result.ok -eq $false) 'JSON does not say ok: false'
    Assert ($result.error -eq $Code) "error is $($result.error), not $Code"
    Test-InstalledIs '1.2.3'
    $left = @(Get-ChildItem -Force home/.oska/bin | ForEach-Object { $_.Name })
    Assert (($left -join ',') -eq 'oskac.exe') "bin holds $($left -join ',')"
}

function Test-RefusesATamperedBinary {
    Install-Good
    Add-Content -LiteralPath site/releases/v1.3.0/oskac-windows-x86_64.exe -Value 'evil'
    Test-Refused 'checksum_mismatch'
}

function Test-RefusesChecksumsRewrittenToMatchATamperedBinary {
    Install-Good
    $dir = 'site/releases/v1.3.0'
    $asset = 'oskac-windows-x86_64.exe'
    Write-Text "$dir/$asset" 'evil'
    $hash = Get-Sha256 "$dir/$asset"
    $lines = [IO.File]::ReadAllLines((Get-FullPath "$dir/SHA256SUMS")) | ForEach-Object {
        if ($_.EndsWith("  $asset")) { "$hash  $asset" } else { $_ }
    }
    Write-Text "$dir/SHA256SUMS" (($lines -join "`n") + "`n")
    Test-Refused 'bad_signature'
}

function Test-RefusesASignatureByAnotherKey {
    Install-Good
    New-Key 'attacker'
    Set-Signature 'site/releases/v1.3.0' -Key 'attacker'
    Test-Refused 'bad_signature'
}

function Test-RefusesASignatureInAnotherNamespace {
    Install-Good
    Set-Signature 'site/releases/v1.3.0' -Namespace 'file'
    Test-Refused 'bad_signature'
}

function Test-RefusesAReleaseWithoutASignature {
    Install-Good
    Remove-Item site/releases/v1.3.0/SHA256SUMS.sig
    Test-Refused 'download_failed'
}

function Test-RefusesToDownloadOverPlainHttp {
    Install-Good
    Test-Refused 'insecure_url' -Site 'http://oskalang.dev'
}

function Test-AnInstallerThatWasNeverPinnedInstallsNothing {
    New-Release '1.2.3'
    Set-Latest '1.2.3'
    $result = Read-Json (Invoke-Installer @('--json') -Installer "$here/install.ps1")
    Assert ($script:status -ne 0) 'the unpinned installer installed'
    Assert ($result.error -eq 'no_signing_key') "error is $($result.error)"
    Assert (-not (Test-Path home)) 'created OSKA_HOME'
}

function Test-RejectsBadArgumentsWithExitStatus2 {
    $result = Read-Json (Invoke-Installer @('--json', '--frobnicate'))
    Assert ($script:status -eq 2) "unknown option exited $script:status"
    Assert ($result.error -eq 'usage') "error is $($result.error)"
    $result = Read-Json (Invoke-Installer @('--json', '--version', '1.2.3; rm'))
    Assert ($script:status -eq 2) "bad version exited $script:status"
}

# `irm https://oskalang.dev/install.ps1 | iex`: no options, no script file.
function Test-RunsUnderInvokeExpression {
    New-Release '1.2.3'
    Set-Latest '1.2.3'
    $env:OSKA_SITE_URL = Get-SiteUrl
    $env:OSKA_HOME = Join-Path (Get-Location).Path 'home\.oska'
    $installer = Get-Pinned
    $out = & $script:shell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Get-Content -Raw -LiteralPath '$installer' | Invoke-Expression"
    Assert ($LASTEXITCODE -eq 0) "exited $LASTEXITCODE"
    Test-InstalledIs '1.2.3'
    Assert (($out -join "`n") -like '*Installed oskac 1.2.3*') "no human summary in: $out"
    Add-Content -LiteralPath site/releases/v1.2.3/oskac-windows-x86_64.exe -Value 'evil'
    Remove-Item home -Recurse -Force
    & $script:shell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "Get-Content -Raw -LiteralPath '$installer' | Invoke-Expression" 2>$null | Out-Null
    Assert ($LASTEXITCODE -ne 0) 'a tampered binary installed under iex'
    Assert (-not (Test-Path (Get-Installed))) 'installed a tampered binary under iex'
}

$shells = @('powershell', 'pwsh') | Where-Object { Get-Command $_ -ErrorAction SilentlyContinue }
Assert ($shells.Count -gt 0) 'no PowerShell to test with'
# Only this file's tests: installed modules define Test-* functions too.
$tests = Get-ChildItem function:Test-* |
    Where-Object { $_.ScriptBlock.File -eq $PSCommandPath -and $_.Name -notin @('Test-InstalledIs', 'Test-Refused') } |
    Sort-Object Name
$passes = 0
$failures = 0
$origin = Get-Location
try {
    foreach ($shell in $shells) {
        $script:shell = $shell
        foreach ($test in $tests) {
            $dir = Join-Path $scratch "$shell-$($test.Name)"
            [void](New-Item -ItemType Directory -Path $dir)
            Set-Location $dir
            $script:stdout = ''
            $script:stderr = ''
            try {
                & $test.Name
                $passes++
                Write-Host "ok   [$shell] $($test.Name)"
            } catch {
                $failures++
                Write-Host "FAIL [$shell] $($test.Name)"
                Write-Host "    $($_.Exception.Message)"
                Write-Host "    $($_.InvocationInfo.PositionMessage)"
                Write-Host "    last installer stdout: $script:stdout"
                Write-Host "    last installer stderr: $script:stderr"
            } finally {
                Set-Location $origin
                Remove-Item Env:OSKA_SITE_URL, Env:OSKA_HOME -ErrorAction SilentlyContinue
            }
        }
    }
} finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "$passes passed, $failures failed"
if ($failures -ne 0) { exit 1 }
