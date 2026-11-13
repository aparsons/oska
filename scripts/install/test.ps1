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
# The certificate Start-RedirectServer makes, once.
$script:certificate = $null

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

# Returns the path of a console program that prints "oskac <version>", like
# the real `oskac --version`, compiled once per version with the C#
# compiler of the .NET Framework, which every Windows has.
function Get-FakeOskac([string]$Version) {
    $exe = Join-Path $scratch "fake-oskac-$Version.exe"
    if (-not (Test-Path -LiteralPath $exe)) {
        $csc = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
        Assert (Test-Path -LiteralPath $csc) "no C# compiler at $csc"
        $source = Join-Path $scratch "fake-oskac-$Version.cs"
        [IO.File]::WriteAllText($source, "class P { static void Main() { System.Console.WriteLine(`"oskac $Version`"); } }", $Utf8)
        $out = & $csc /nologo /target:exe "/out:$exe" $source
        Assert ($LASTEXITCODE -eq 0) "csc could not build $exe`: $out"
    }
    $exe
}

# Publishes release <version> in site/releases/v<version>/, as
# scripts/release/assemble.sh and sign.sh lay it out: SHA256SUMS starts
# with the signed tag line. The Windows binary prints "oskac <reports>"
# (default: <version>).
function New-Release([string]$Version, [string]$Reports = $Version) {
    if (-not (Test-Path key)) { New-Key 'key' }
    $dir = "site/releases/v$Version"
    [void](New-Item -ItemType Directory -Force -Path $dir)
    $sums = "# oska-release v$Version`n"
    foreach ($asset in $Assets) {
        if ($asset -eq 'oskac-windows-x86_64.exe') {
            Copy-Item -LiteralPath (Get-FakeOskac $Reports) -Destination "$dir/$asset"
        } else {
            Write-Text "$dir/$asset" "binary $asset $Version`n"
        }
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

# Writes pinned/install.ps1, pinned to key.pub as pin.sh pins it, and to
# require the release index (yes) or not (no).
function Write-Pinned([string]$RequireIndex) {
    if (-not (Test-Path key)) { New-Key 'key' }
    $parts = (Get-Content -Raw key.pub).Trim() -split ' '
    $source = [IO.File]::ReadAllText("$here/install.ps1")
    [void](New-Item -ItemType Directory -Force -Path pinned)
    Write-Text 'pinned/install.ps1' $source.Replace('@OSKA_RELEASE_KEY@', "$($parts[0]) $($parts[1])").Replace('@OSKA_INDEX_REQUIRED@', $RequireIndex)
}

function Get-Pinned {
    if (-not (Test-Path pinned/install.ps1)) { Write-Pinned 'no' }
    (Resolve-Path pinned/install.ps1).Path
}

# Pins the installer to require the index, as stage-site.sh does once it
# publishes one.
function Set-RequireIndex {
    Write-Pinned 'yes'
}

# An RFC 3339 UTC time <Days> days from now (negative: in the past).
function Get-DaysFromNow([int]$Days) {
    [DateTime]::UtcNow.AddDays($Days).ToString("yyyy-MM-dd'T'HH:mm:ss'Z'", [Globalization.CultureInfo]::InvariantCulture)
}

# Publishes site/release-index.json naming release <Version>, with serial
# <Serial>, expiring in <Days> days, signed in <Namespace> by <Key>, as
# scripts/release/sign-index.sh writes it.
function New-Index([long]$Serial, [string]$Version, [int]$Days = 60, [string]$Namespace = 'oska-index', [string]$Key = 'key') {
    if (-not (Test-Path key)) { New-Key 'key' }
    [void](New-Item -ItemType Directory -Force -Path site)
    Write-Text 'site/release-index.json' (@"
{
  "schema": 1,
  "serial": $Serial,
  "issued": "$(Get-DaysFromNow -30)",
  "expires": "$(Get-DaysFromNow $Days)",
  "latest": {
    "tag": "v$Version",
    "version": "$Version"
  }
}
"@)
    Remove-Item -LiteralPath 'site/release-index.json.sig' -ErrorAction SilentlyContinue
    & ssh-keygen -q -Y sign -f $Key -n $Namespace site/release-index.json
    Assert ($LASTEXITCODE -eq 0) 'ssh-keygen could not sign the index'
}

# Releases 1.2.3 and 1.3.0, with latest.json still naming 1.2.3 and a
# signed index (serial 7) naming 1.3.0.
function Use-IndexNaming130 {
    New-Release '1.2.3'
    New-Release '1.3.0'
    Set-Latest '1.2.3'
    New-Index 7 '1.3.0'
    Set-RequireIndex
}

function Get-ReceiptSerial {
    (Get-Content -Raw home/.oska/receipt.json | ConvertFrom-Json).index_serial
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
    Assert ((Get-Content home/.oska/version) -eq $Version) "the version file does not say $Version"
    $receipt = Get-Content -Raw home/.oska/receipt.json | ConvertFrom-Json
    Assert ($receipt.version -eq $Version) "the receipt does not say $Version"
}

# Every path under home, with each file's SHA-256: compare two of these to
# show a run changed nothing.
function Get-Snapshot {
    if (-not (Test-Path home)) { return '' }
    $root = (Resolve-Path home).Path
    $lines = Get-ChildItem -Recurse -Force -LiteralPath $root | Sort-Object FullName | ForEach-Object {
        if ($_.PSIsContainer) { $_.FullName } else { "$($_.FullName) $(Get-Sha256 $_.FullName)" }
    }
    @($lines) -join "`n"
}

# Runs the installer with <Arguments>, which must refuse with <Code> and
# exit status <Status>, as one line of JSON, and leave home exactly as it
# was. Returns the JSON.
function Test-RefusedUnchanged([string]$Code, [int]$Status, [string[]]$Arguments) {
    $before = Get-Snapshot
    $all = @('--json')
    if ($Arguments) { $all += $Arguments }
    $result = Read-Json (Invoke-Installer $all)
    Assert ($script:status -eq $Status) "$($Arguments -join ' '): exited $script:status, not $Status"
    Assert ($result.ok -eq $false) 'JSON does not say ok: false'
    Assert ($result.error -eq $Code) "$($Arguments -join ' '): error is $($result.error), not $Code"
    Assert ((Get-Snapshot) -eq $before) "$($Arguments -join ' '): the home changed"
    $result
}

function Test-WritesAReceiptListingWhatItInstalled {
    New-Release '1.2.3'
    Set-Latest '1.2.3'
    $result = Read-Json (Invoke-Installer @('--json'))
    Assert ($script:status -eq 0) "exited $script:status"
    $oskaHome = Join-Path (Get-Location).Path 'home\.oska'
    $path = Join-Path $oskaHome 'receipt.json'
    Assert ($result.receipt -eq $path) "JSON receipt is $($result.receipt)"
    $receipt = Get-Content -Raw -LiteralPath $path | ConvertFrom-Json
    $fingerprint = ((& ssh-keygen -l -f key.pub) -split ' ')[1]
    $installed = Get-Installed
    $expected = [ordered]@{
        schema = 1; version = '1.2.3'; tag = 'v1.2.3'; platform = 'windows-x86_64'
        site = (Get-SiteUrl).TrimEnd('/'); key_fingerprint = $fingerprint
        sha256 = (Get-Sha256 $installed); installer = 'ps1'; index_serial = 0
    }
    foreach ($k in $expected.Keys) {
        Assert ($receipt.$k -eq $expected[$k]) "receipt $k is '$($receipt.$k)', not '$($expected[$k])'"
    }
    # Read from the text: PowerShell 7's ConvertFrom-Json turns dates into
    # DateTime values.
    $text = [IO.File]::ReadAllText($path)
    Assert ($text -cmatch '"installed_at":\s*"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z"') "installed_at is not UTC RFC 3339 in: $text"
    $files = @($receipt.files)
    Assert ($files.Count -eq 3) "the receipt lists $($files.Count) files"
    Assert ($files[0].path -eq $installed -and $files[0].kind -eq 'binary' -and $files[0].sha256 -eq (Get-Sha256 $installed)) 'files[0] is not the binary'
    $versionFile = Join-Path $oskaHome 'version'
    Assert ($files[1].path -eq $versionFile -and $files[1].kind -eq 'record' -and $files[1].sha256 -eq (Get-Sha256 $versionFile)) 'files[1] is not the version file'
    Assert ($files[2].path -eq $path -and $files[2].kind -eq 'record') 'files[2] is not the receipt'
    Assert ([IO.File]::ReadAllText($versionFile) -ceq "1.2.3`n") 'the version file is not 1.2.3'
    $held = @(Get-ChildItem -Force home/.oska | Sort-Object Name | ForEach-Object { $_.Name })
    Assert (($held -join ',') -eq 'bin,receipt.json,version') "the home holds $($held -join ',')"
}

# An install from before receipts has only the version file. The next run
# takes the previous version from it and writes a receipt.
function Test-UpdatesAnInstallThatHasOnlyAVersionFile {
    New-Release '1.2.3'
    New-Release '1.3.0'
    Set-Latest '1.2.3'
    [void](Invoke-Installer @('--json'))
    Remove-Item home/.oska/receipt.json
    Set-Latest '1.3.0'
    $result = Read-Json (Invoke-Installer @('--json'))
    Assert ($result.action -eq 'updated') "action is $($result.action)"
    Assert ($result.previous_version -eq '1.2.3') "previous_version is $($result.previous_version)"
    Test-InstalledIs '1.3.0'
}

function Test-TheReceiptWinsOverTheVersionFile {
    New-Release '1.2.3'
    New-Release '1.3.0'
    Set-Latest '1.2.3'
    [void](Invoke-Installer @('--json'))
    Write-Text 'home/.oska/version' "9.9.9`n"
    Set-Latest '1.3.0'
    $result = Read-Json (Invoke-Installer @('--json'))
    Assert ($result.action -eq 'updated') "action is $($result.action)"
    Assert ($result.previous_version -eq '1.2.3') "previous_version is $($result.previous_version)"
}

# The receipt is advisory: one that vouches for a tampered binary changes
# nothing about what is verified.
function Test-ATamperedReceiptCannotVouchForATamperedBinary {
    Install-Good
    $asset = 'site/releases/v1.3.0/oskac-windows-x86_64.exe'
    Add-Content -LiteralPath $asset -Value 'evil'
    $evil = Get-Sha256 $asset
    $text = [IO.File]::ReadAllText((Get-FullPath 'home/.oska/receipt.json'))
    $text = $text.Replace('1.2.3', '1.3.0').Replace((Get-Sha256 (Get-Installed)), $evil)
    Write-Text 'home/.oska/receipt.json' $text
    [void](Test-RefusedUnchanged 'checksum_mismatch' 1 @('--version', '1.3.0'))
    Assert ((Get-Sha256 (Get-Installed)) -eq (Get-Sha256 'site/releases/v1.2.3/oskac-windows-x86_64.exe')) 'the installed oskac changed'
}

function Test-RefusesToDowngradeToAnOlderLatestRelease {
    New-Release '1.2.3'
    New-Release '1.3.0'
    Set-Latest '1.3.0'
    [void](Invoke-Installer @('--json'))
    Set-Latest '1.2.3'
    $result = Test-RefusedUnchanged 'downgrade_refused' 1 @()
    Assert ($result.message -like '*1.3.0*' -and $result.message -like '*1.2.3*') "message is $($result.message)"
    Test-InstalledIs '1.3.0'
}

function Test-RefusesADowngradeNamedWithVersionAlone {
    New-Release '1.2.3'
    New-Release '1.3.0'
    Set-Latest '1.3.0'
    [void](Invoke-Installer @('--json'))
    $result = Test-RefusedUnchanged 'downgrade_refused' 1 @('--version', '1.2.3')
    Assert ($result.message -like '*--allow-downgrade*') "message is $($result.message)"
}

function Test-DowngradesWithVersionAndAllowDowngrade {
    New-Release '1.2.3'
    New-Release '1.3.0'
    Set-Latest '1.3.0'
    [void](Invoke-Installer @('--json'))
    $result = Read-Json (Invoke-Installer @('--json', '--allow-downgrade', '--version', '1.2.3'))
    Assert ($script:status -eq 0) "exited $script:status"
    Test-InstalledIs '1.2.3'
    Assert ($result.action -eq 'downgraded') "action is $($result.action)"
    Assert ($result.previous_version -eq '1.3.0') "previous_version is $($result.previous_version)"
    [void](Invoke-Installer @('-Version', '1.3.0'))
    $out = Invoke-Installer @('-Version', '1.2.3', '-AllowDowngrade')
    Assert (($out -join "`n") -like "*Downgraded oskac 1.3.0 to 1.2.3 in $(Get-Installed)*") "no human summary in: $out"
}

function Test-AllowDowngradeWithoutVersionIsAUsageError {
    New-Release '1.2.3'
    Set-Latest '1.2.3'
    [void](Invoke-Installer @('--json'))
    [void](Test-RefusedUnchanged 'usage' 2 @('--allow-downgrade'))
}

# A pre-release installed with --version is newer than the latest release,
# so a plain re-run keeps it rather than move back.
function Test-APlainReRunKeepsANewerPreRelease {
    New-Release '1.3.0'
    New-Release '1.4.0-pre.1'
    Set-Latest '1.3.0'
    [void](Invoke-Installer @('--json', '--version', '1.4.0-pre.1'))
    $result = Test-RefusedUnchanged 'downgrade_refused' 1 @()
    Assert ($result.message -like '*1.4.0-pre.1 is installed, which is newer than the latest release, 1.3.0*') "message is $($result.message)"
    New-Release '1.4.0'
    Set-Latest '1.4.0'
    $result = Read-Json (Invoke-Installer @('--json'))
    Assert ($result.action -eq 'updated') "1.4.0-pre.1 to 1.4.0 is $($result.action)"
}

# Versions compare by semver precedence: numerically, part by part, with a
# pre-release below its release.
function Test-ComparesVersionsBySemverPrecedence {
    $cases = @(
        @('1.9.0', '1.10.0', 'updated'),
        @('1.10.0', '1.9.0', 'downgraded'),
        @('1.2.10', '1.2.9', 'downgraded'),
        @('2.0.0', '10.0.0', 'updated'),
        @('1.0.0-alpha', '1.0.0', 'updated'),
        @('1.0.0', '1.0.0-rc.1', 'downgraded'),
        @('1.0.0-alpha', '1.0.0-alpha.1', 'updated'),
        @('1.0.0-alpha.1', '1.0.0-alpha.beta', 'updated'),
        @('1.0.0-beta.11', '1.0.0-beta.2', 'downgraded'),
        @('1.0.0-beta', '1.0.0-beta.2', 'updated'),
        @('1.0.0-rc.1', '1.0.0-rc.1', 'reinstalled')
    )
    foreach ($case in $cases) {
        if (Test-Path home) { Remove-Item -Recurse -Force home }
        New-Release $case[0]
        New-Release $case[1]
        [void](Invoke-Installer @('--json', '--version', $case[0]))
        $result = Read-Json (Invoke-Installer @('--json', '--allow-downgrade', '--version', $case[1]))
        Assert ($script:status -eq 0) "$($case[0]) to $($case[1]) exited $script:status"
        Assert ($result.action -eq $case[2]) "$($case[0]) to $($case[1]) is $($result.action), not $($case[2])"
    }
}

# Install (the default), --check and --uninstall are modes, and one run is
# in one of them. --version and --allow-downgrade belong to install.
function Test-ModesAreMutuallyExclusive {
    New-Release '1.2.3'
    Set-Latest '1.2.3'
    [void](Invoke-Installer @('--json'))
    $cases = @(
        @('--check', '--uninstall'),
        @('--uninstall', '--check'),
        @('--check', '--version', '1.2.3'),
        @('--uninstall', '--version', '1.2.3'),
        @('--check', '--allow-downgrade'),
        @('--uninstall', '--version', '1.2.3', '--allow-downgrade')
    )
    foreach ($case in $cases) {
        $result = Test-RefusedUnchanged 'usage' 2 $case
        Assert ($result.message -like '*cannot be used*') "$($case -join ' '): message is $($result.message)"
    }
}

# The staged binary goes into a new directory with a name no one can
# predict: something planted at the old fixed name, .oskac.new.exe, is
# neither used nor touched.
function Test-StagesUnderANameNoOneCanPredict {
    New-Release '1.2.3'
    Set-Latest '1.2.3'
    [void](New-Item -ItemType Directory -Force -Path 'home/.oska/bin/.oskac.new.exe')
    Write-Text 'home/.oska/bin/.oskac.new.exe/planted' "planted`n"
    [void](Invoke-Installer @('--json'))
    Assert ($script:status -eq 0) "exited $script:status"
    Test-InstalledIs '1.2.3'
    Assert ((Get-Content -Raw 'home/.oska/bin/.oskac.new.exe/planted') -eq "planted`n") 'the planted file changed'
    $left = @(Get-ChildItem -Force home/.oska/bin | ForEach-Object { $_.Name })
    Assert ($left.Count -eq 2 -and $left -contains '.oskac.new.exe' -and $left -contains 'oskac.exe') "bin holds $($left -join ',')"
}

# A run removes the oskac.exe.old an earlier run could not.
function Test-RemovesALeftoverOldBinary {
    New-Release '1.2.3'
    Set-Latest '1.2.3'
    [void](Invoke-Installer @('--json'))
    Write-Text 'home/.oska/bin/oskac.exe.old' "old`n"
    [void](Invoke-Installer @('--json'))
    Assert ($script:status -eq 0) "exited $script:status"
    $left = @(Get-ChildItem -Force home/.oska/bin | ForEach-Object { $_.Name })
    Assert (($left -join ',') -eq 'oskac.exe') "bin holds $($left -join ',')"
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
        receipt = (Join-Path (Get-Location).Path 'home\.oska\receipt.json')
        index_verified = $false; index_serial = $null
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
    $before = Get-Snapshot
    $result = Read-Json (Invoke-Installer @('--json', '--version', '1.3.0') -Site $Site)
    Assert ($script:status -ne 0) 'installed a tampered release'
    Assert ($result.ok -eq $false) 'JSON does not say ok: false'
    Assert ($result.error -eq $Code) "error is $($result.error), not $Code"
    Test-InstalledIs '1.2.3'
    Assert ((Get-Snapshot) -eq $before) 'the home changed'
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

# The rollback attack: genuine, signed files of an old release served as a
# newer one. The signature is valid; only the signed tag line refuses it.
function Test-RefusesAnOlderSignedReleasePublishedAsTheLatest {
    Install-Good
    Remove-Item -Recurse -Force site/releases/v1.3.0
    Copy-Item -Recurse site/releases/v1.2.3 site/releases/v1.3.0
    Set-Latest '1.3.0'
    $result = Read-Json (Invoke-Installer @('--json'))
    Assert ($script:status -ne 0) "installed 1.2.3's files as 1.3.0"
    Assert ($result.error -eq 'version_mismatch') "error is $($result.error)"
    Assert ($result.message -like '*v1.2.3*') "message does not name the signed tag: $($result.message)"
    Test-InstalledIs '1.2.3'
}

function Test-RefusesAnOlderSignedReleaseFromAMirrorForANewerVersion {
    Install-Good
    [void](New-Item -ItemType Directory -Force -Path mirror/releases)
    Copy-Item -Recurse site/releases/v1.2.3 mirror/releases/v1.3.0
    $mirror = ([Uri](Join-Path (Get-Location).Path 'mirror')).AbsoluteUri
    Test-Refused 'version_mismatch' -Site $mirror
}

function Test-RefusesSignedChecksumsThatNameNoTag {
    Install-Good
    $dir = 'site/releases/v1.3.0'
    $lines = [IO.File]::ReadAllLines((Get-FullPath "$dir/SHA256SUMS")) | Where-Object { -not $_.StartsWith('#') }
    Write-Text "$dir/SHA256SUMS" (($lines -join "`n") + "`n")
    Set-Signature $dir
    Test-Refused 'version_mismatch'
}

function Test-RefusesABinaryThatDoesNotReportTheSignedVersion {
    New-Release '1.2.3'
    Set-Latest '1.2.3'
    [void](Invoke-Installer @('--json'))
    Test-InstalledIs '1.2.3'
    New-Release '1.3.0' -Reports '1.2.9'
    Test-Refused 'version_mismatch'
}

# A local HTTPS server that answers every request with a redirect to
# <Location>. Its certificate, for localhost, is trusted machine-wide while
# the tests run, which needs administrator rights (CI runners have them).
function Start-RedirectServer([string]$Location) {
    if (-not $script:certificate) {
        $script:certificate = New-SelfSignedCertificate -DnsName 'localhost' -CertStoreLocation 'Cert:\CurrentUser\My' -NotAfter (Get-Date).AddDays(1)
        $root = New-Object Security.Cryptography.X509Certificates.X509Store -ArgumentList 'Root', 'LocalMachine'
        $root.Open('ReadWrite')
        $root.Add($script:certificate)
        $root.Close()
    }
    $listener = [Net.Sockets.TcpListener]::Create(0)
    $listener.Start()
    $serve = {
        param($Listener, $Certificate, $Location)
        while ($true) {
            try { $client = $Listener.AcceptTcpClient() } catch { return }
            try {
                $ssl = New-Object Net.Security.SslStream -ArgumentList $client.GetStream(), $false
                $ssl.AuthenticateAsServer($Certificate, $false, [Security.Authentication.SslProtocols]::Tls12, $false)
                $reader = New-Object IO.StreamReader -ArgumentList $ssl, ([Text.Encoding]::ASCII), $false, 1024, $true
                while ($true) {
                    $line = $reader.ReadLine()
                    if ($null -eq $line -or $line -eq '') { break }
                }
                $reply = [Text.Encoding]::ASCII.GetBytes("HTTP/1.1 302 Found`r`nLocation: $Location`r`nContent-Length: 0`r`nConnection: close`r`n`r`n")
                $ssl.Write($reply, 0, $reply.Length)
                $ssl.Flush()
            } catch {
            } finally {
                $client.Close()
            }
        }
    }
    $shell = [PowerShell]::Create().AddScript($serve).AddArgument($listener).AddArgument($script:certificate).AddArgument($Location)
    $handle = $shell.BeginInvoke()
    @{ Listener = $listener; Shell = $shell; Handle = $handle; Port = $listener.LocalEndpoint.Port }
}

function Stop-RedirectServer($Server) {
    $Server.Listener.Stop()
    [void]$Server.Shell.EndInvoke($Server.Handle)
    $Server.Shell.Dispose()
}

function Remove-TestCertificate {
    if ($script:certificate) {
        $root = New-Object Security.Cryptography.X509Certificates.X509Store -ArgumentList 'Root', 'LocalMachine'
        $root.Open('ReadWrite')
        $root.Remove($script:certificate)
        $root.Close()
        Remove-Item -LiteralPath "Cert:\CurrentUser\My\$($script:certificate.Thumbprint)" -ErrorAction SilentlyContinue
    }
}

# Windows PowerShell 5.1's Invoke-WebRequest follows a redirect from https
# to http. The installer must refuse it, and with insecure_url: a
# download_failed would mean the https hop itself did not work.
function Test-RefusesARedirectFromHttpsToHttp {
    New-Release '1.2.3'
    $server = Start-RedirectServer 'http://127.0.0.1:1/latest.json'
    try {
        $result = Read-Json (Invoke-Installer @('--json') -Site "https://localhost:$($server.Port)")
        Assert ($script:status -ne 0) 'followed a redirect to http'
        Assert ($result.error -eq 'insecure_url') "error is $($result.error): $($result.message)"
        Assert (-not (Test-Path home)) 'created OSKA_HOME'
    } finally {
        Stop-RedirectServer $server
    }
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

# ---- The signed release index and --check ------------------------------------

function Test-InstallsTheLatestReleaseTheSignedIndexNames {
    Use-IndexNaming130
    $result = Read-Json (Invoke-Installer @('--json'))
    Assert ($script:status -eq 0) "exited $script:status"
    Test-InstalledIs '1.3.0'
    Assert ($result.index_verified -eq $true) "index_verified is $($result.index_verified)"
    Assert ($result.index_serial -eq 7) "index_serial is $($result.index_serial)"
    Assert ((Get-ReceiptSerial) -eq 7) "the receipt records serial $(Get-ReceiptSerial)"
}

function Test-AnInstallerNotPinnedToRequireTheIndexReadsLatestJson {
    Use-IndexNaming130
    Write-Pinned 'no'
    $result = Read-Json (Invoke-Installer @('--json'))
    Assert ($script:status -eq 0) "exited $script:status"
    Test-InstalledIs '1.2.3'
    Assert ($result.index_verified -eq $false) "index_verified is $($result.index_verified)"
    Assert ($null -eq $result.index_serial) "index_serial is $($result.index_serial)"
}

function Test-RefusesAnIndexThatIsNotSignedByTheReleaseKeyAsAnIndex {
    Use-IndexNaming130
    [void](Invoke-Installer @('--json', '--version', '1.2.3'))
    New-Key 'attacker'
    foreach ($case in @('unsigned', 'another_key', 'release_namespace', 'not_an_index')) {
        New-Index 7 '1.3.0'
        switch ($case) {
            'unsigned' { Remove-Item site/release-index.json.sig }
            'another_key' { New-Index 7 '1.3.0' -Key 'attacker' }
            'release_namespace' { New-Index 7 '1.3.0' -Namespace 'oska-release' }
            'not_an_index' {
                Write-Text 'site/release-index.json' "{`"schema`": 1, `"serial`": `"seven`", `"latest`": {`"tag`": `"v1.3.0`"}}`n"
                Remove-Item site/release-index.json.sig
                & ssh-keygen -q -Y sign -f key -n oska-index site/release-index.json
            }
        }
        [void](Test-RefusedUnchanged 'bad_index' 1 @())
        Test-InstalledIs '1.2.3'
    }
}

function Test-RefusesAnExpiredIndex {
    Use-IndexNaming130
    [void](Invoke-Installer @('--json', '--version', '1.2.3'))
    New-Index 7 '1.3.0' -Days -1
    $result = Test-RefusedUnchanged 'stale_index' 1 @()
    Assert ($result.message -like '*--version*') "message is $($result.message)"
}

function Test-RefusesAnIndexWithALowerSerialThanOneAlreadyAccepted {
    Use-IndexNaming130
    [void](Invoke-Installer @('--json'))
    # A genuine, unexpired, older index: the rollback (and freeze) attack.
    New-Index 6 '1.2.3'
    $result = Test-RefusedUnchanged 'rollback_refused' 1 @()
    Assert ($result.message -like '*6*7*') "message is $($result.message)"
    New-Index 7 '1.3.0'
    $result = Read-Json (Invoke-Installer @('--json'))
    Assert ($result.action -eq 'reinstalled') "serial 7 again is $($result.action)"
}

function Test-RefusesAMissingIndexWhenItIsRequired {
    Use-IndexNaming130
    Remove-Item site/release-index.json
    $result = Read-Json (Invoke-Installer @('--json'))
    Assert ($script:status -eq 1) "exited $script:status"
    Assert ($result.error -eq 'download_failed') "error is $($result.error)"
    Assert (-not (Test-Path home)) 'created OSKA_HOME'
}

function Test-WarnsWhenFewerThan30DaysRemainOnTheIndex {
    Use-IndexNaming130
    New-Index 7 '1.3.0' -Days 29
    $result = Read-Json (Invoke-Installer @('--json'))
    Assert ($result.version -eq '1.3.0') "version is $($result.version)"
    Assert ($script:stderr -like '*warning: *release index expires*') "no warning in: $script:stderr"
    New-Index 7 '1.3.0' -Days 31
    [void](Invoke-Installer @('--json'))
    Assert (-not ($script:stderr -like '*warning*')) "warned with 31 days left: $script:stderr"
}

# --version installs from the signed SHA256SUMS alone: an expired index
# does not stop it, and the serial the receipt holds is kept.
function Test-VersionIgnoresTheIndexAndKeepsTheSerialSeen {
    Use-IndexNaming130
    [void](Invoke-Installer @('--json'))
    New-Index 7 '1.3.0' -Days -1
    $result = Read-Json (Invoke-Installer @('--json', '--allow-downgrade', '--version', '1.2.3'))
    Assert ($script:status -eq 0) "exited $script:status"
    Assert ($result.action -eq 'downgraded') "action is $($result.action)"
    Assert ($result.index_verified -eq $false) "index_verified is $($result.index_verified)"
    Assert ((Get-ReceiptSerial) -eq 7) "the receipt records serial $(Get-ReceiptSerial)"
}

function Test-CheckReportsAnUpdateAndWritesNothing {
    Use-IndexNaming130
    [void](Invoke-Installer @('--json', '--version', '1.2.3'))
    $before = Get-Snapshot
    $result = Read-Json (Invoke-Installer @('--json', '--check'))
    Assert ($script:status -eq 0) "exited $script:status"
    Assert ((Get-Snapshot) -eq $before) '--check changed the home'
    $expected = [ordered]@{
        ok = $true; action = 'checked'; installed = '1.2.3'; latest = '1.3.0'; tag = 'v1.3.0'
        update_available = $true; index_verified = $true; index_serial = 7
    }
    $names = @($result.PSObject.Properties | ForEach-Object { $_.Name })
    Assert (($names -join ',') -eq ((@($expected.Keys) + 'index_expires') -join ',')) "fields are $($names -join ',')"
    foreach ($k in $expected.Keys) {
        Assert ($result.$k -eq $expected[$k]) "$k is '$($result.$k)', not '$($expected[$k])'"
    }
    # Read from the text: PowerShell 7's ConvertFrom-Json turns dates into
    # DateTime values.
    $expires = [regex]::Match([IO.File]::ReadAllText((Get-FullPath 'site/release-index.json')), '"expires": "([^"]+)"').Groups[1].Value
    Assert ($script:stdout -clike "*`"index_expires`":`"$expires`"*") "index_expires is not $expires in: $script:stdout"
    $out = Invoke-Installer @('--check')
    Assert ($script:status -eq 0) "exited $script:status"
    Assert (($out -join "`n") -like '*oskac 1.2.3 is installed*1.3.0*') "no human summary in: $out"
}

function Test-CheckReportsWhenUpToDateOrNotInstalled {
    Use-IndexNaming130
    $result = Read-Json (Invoke-Installer @('--json', '--check'))
    Assert ($null -eq $result.installed) "installed is $($result.installed)"
    Assert ($result.update_available -eq $true) 'nothing installed: update_available is not true'
    Assert (-not (Test-Path home)) '--check created OSKA_HOME'
    [void](Invoke-Installer @('--json'))
    $result = Read-Json (Invoke-Installer @('--json', '--check'))
    Assert ($result.installed -eq '1.3.0') "installed is $($result.installed)"
    Assert ($result.update_available -eq $false) 'up to date: update_available is not false'
    New-Release '1.4.0-pre.1'
    [void](Invoke-Installer @('--json', '--version', '1.4.0-pre.1'))
    $result = Read-Json (Invoke-Installer @('--json', '--check'))
    Assert ($result.update_available -eq $false) 'a newer pre-release: update_available is not false'
}

function Test-CheckRefusesAnIndexItCannotTrust {
    Use-IndexNaming130
    [void](Invoke-Installer @('--json'))
    New-Index 7 '1.3.0' -Days -1
    [void](Test-RefusedUnchanged 'stale_index' 1 @('--check'))
    New-Index 6 '1.3.0'
    [void](Test-RefusedUnchanged 'rollback_refused' 1 @('--check'))
    New-Index 7 '1.3.0' -Namespace 'oska-release'
    [void](Test-RefusedUnchanged 'bad_index' 1 @('--check'))
}

function Test-CheckWithoutARequiredIndexReadsLatestJson {
    New-Release '1.2.3'
    Set-Latest '1.2.3'
    $result = Read-Json (Invoke-Installer @('--json', '--check'))
    Assert ($script:status -eq 0) "exited $script:status"
    Assert ($result.latest -eq '1.2.3') "latest is $($result.latest)"
    Assert ($result.index_verified -eq $false) "index_verified is $($result.index_verified)"
    Assert ($null -eq $result.index_serial) "index_serial is $($result.index_serial)"
    Assert ($null -eq $result.index_expires) "index_expires is $($result.index_expires)"
}

$shells = @('powershell', 'pwsh') | Where-Object { Get-Command $_ -ErrorAction SilentlyContinue }
Assert ($shells.Count -gt 0) 'no PowerShell to test with'
# Only this file's tests: installed modules define Test-* functions too.
$tests = Get-ChildItem function:Test-* |
    Where-Object { $_.ScriptBlock.File -eq $PSCommandPath -and $_.Name -notin @('Test-InstalledIs', 'Test-Refused', 'Test-RefusedUnchanged') } |
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
    Remove-TestCertificate
    Remove-Item -LiteralPath $scratch -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "$passes passed, $failures failed"
if ($failures -ne 0) { exit 1 }
