# Install oskac, the Oska compiler and runner, on Windows.
#
#   irm https://oskalang.dev/install.ps1 | iex
#
# `iex` cannot pass options. To pass them, run the script as a script block:
#
#   & ([scriptblock]::Create((irm https://oskalang.dev/install.ps1))) --version 0.1.0 --json
#
# Options (`-Version` and `-Json` work too):
#   --version X  install release X (0.1.0 or v0.1.0) instead of the latest
#   --json       print one JSON object on stdout, and nothing else
#   --help       print this help
#
# Environment:
#   OSKA_HOME      install into $env:OSKA_HOME\bin (default: ~\.oska)
#   OSKA_SITE_URL  where to download from (default: https://oskalang.dev);
#                  only https:// and file:// URLs are accepted
#
# Before installing anything it checks that SHA256SUMS carries a valid SSH
# signature by the release key pinned below (with the ssh-keygen of Windows'
# OpenSSH client), and that the binary's SHA-256 is the one SHA256SUMS
# lists. If either check fails, or anything else goes wrong, it installs
# nothing and fails: run as a file it exits non-zero, under `iex` it throws.
# It never prompts. Re-running it updates oskac in place.
#
# Works in Windows PowerShell 5.1 and PowerShell 7.

& {
    Set-StrictMode -Version 2.0
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'

    # The release signing key, pinned when the site is built
    # (scripts/install/pin.sh). Signatures by any other key are refused.
    $ReleaseKey = '@OSKA_RELEASE_KEY@'
    $Namespace = 'oska-release'
    $DefaultSiteUrl = 'https://oskalang.dev'

    # Kept in a table, not in $script: variables, which `iex` would leave in
    # the user's session.
    $state = @{ json = $false; tmp = $null }

    function Stop-Install([string]$Code, [string]$Message) {
        $e = New-Object System.Exception $Message
        $e.Data['oska'] = $Code
        throw $e
    }

    function Write-Say([string]$Message) {
        if (-not $state.json) { Write-Host $Message }
    }

    function Test-Tag([string]$Tag) {
        return $Tag -cmatch '^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$'
    }

    # Downloads over HTTPS only, or copies a local file:// URL.
    function Get-Url([string]$Url, [string]$OutFile) {
        try {
            if ($Url.StartsWith('https://')) {
                Invoke-WebRequest -Uri $Url -OutFile $OutFile -UseBasicParsing
            } elseif ($Url.StartsWith('file://')) {
                Copy-Item -LiteralPath ([Uri]$Url).LocalPath -Destination $OutFile
            } else {
                return $false
            }
            return (Test-Path -LiteralPath $OutFile -PathType Leaf)
        } catch {
            return $false
        }
    }

    # Runs `ssh-keygen -Y verify` with $Data as its stdin, byte for byte.
    # Neither PowerShell's pipe to a native command (it re-encodes text) nor
    # a redirected Process.StandardInput (.NET Framework writes the console
    # encoding's BOM first) would give ssh-keygen the exact bytes; a file
    # handle from Start-Process -RedirectStandardInput does.
    # Returns '' if the signature verifies, otherwise why it does not.
    function Get-SignatureProblem([string]$KeyGen, [string]$Allowed, [string]$Signature, [string]$Data) {
        $out = Join-Path $state.tmp 'verify.out'
        $err = Join-Path $state.tmp 'verify.err'
        try {
            # One argument string, quoted here: Windows PowerShell joins an
            # array of arguments without quoting paths that hold spaces.
            $arguments = "-Y verify -f `"$Allowed`" -I $Namespace -n $Namespace -s `"$Signature`""
            $p = Start-Process -FilePath $KeyGen -ArgumentList $arguments -NoNewWindow -Wait -PassThru `
                -RedirectStandardInput $Data -RedirectStandardOutput $out -RedirectStandardError $err
            # Anything but a clear 0, including no exit code at all, fails.
            if ($p.ExitCode -is [int] -and $p.ExitCode -eq 0) { return '' }
            $said = ''
            if (Test-Path -LiteralPath $err) { $said = ([string](Get-Content -Raw -LiteralPath $err)).Trim() }
            return "$KeyGen exited $($p.ExitCode): $said"
        } catch {
            return "cannot run ${KeyGen}: $($_.Exception.Message)"
        }
    }

    function Write-Utf8([string]$Path, [string]$Text) {
        [IO.File]::WriteAllText($Path, $Text, (New-Object System.Text.UTF8Encoding $false))
    }

    function Install-Oskac([object[]]$Arguments) {
        $requested = ''
        $i = 0
        while ($i -lt $Arguments.Count) {
            $a = [string]$Arguments[$i]
            $lower = $a.ToLowerInvariant()
            if ($lower -eq '--json' -or $lower -eq '-json') {
                $state.json = $true
            } elseif ($lower -eq '--version' -or $lower -eq '-version') {
                if ($i + 1 -ge $Arguments.Count) {
                    Stop-Install 'usage' '--version needs a value, like --version 0.1.0'
                }
                $i++
                $requested = [string]$Arguments[$i]
            } elseif ($lower.StartsWith('--version=')) {
                $requested = $a.Substring(10)
            } elseif ($lower -eq '--help' -or $lower -eq '-help' -or $lower -eq '-h') {
                Write-Host 'Install oskac to ~\.oska\bin, verifying its checksum and release signature.'
                Write-Host ''
                Write-Host '  --version X  install release X (0.1.0 or v0.1.0) instead of the latest'
                Write-Host '  --json       print one JSON object on stdout, and nothing else'
                Write-Host '  --help       print this help'
                Write-Host ''
                Write-Host 'Environment: OSKA_HOME (default ~\.oska), OSKA_SITE_URL (https:// only).'
                return
            } else {
                Stop-Install 'usage' "unknown option $a (try --help)"
            }
            $i++
        }

        if (-not $ReleaseKey.StartsWith('ssh-ed25519 ')) {
            Stop-Install 'no_signing_key' "this installer has no pinned release key; download it from $DefaultSiteUrl/install.ps1"
        }

        $site = $env:OSKA_SITE_URL
        if (-not $site) { $site = $DefaultSiteUrl }
        $site = $site.TrimEnd('/')
        if (-not ($site.StartsWith('https://') -or $site.StartsWith('file://'))) {
            Stop-Install 'insecure_url' "refusing to download from ${site}: only https:// URLs are allowed"
        }

        $tag = ''
        if ($requested) {
            if ($requested.StartsWith('v')) { $tag = $requested } else { $tag = "v$requested" }
            if (-not (Test-Tag $tag)) {
                Stop-Install 'usage' "'$requested' is not a version like 0.1.0"
            }
        }

        if ($env:OS -ne 'Windows_NT') {
            Stop-Install 'unsupported_platform' "on Linux and macOS, use install.sh: curl -fsSL $DefaultSiteUrl/install.sh | sh"
        }
        $machine = $env:PROCESSOR_ARCHITEW6432
        if (-not $machine) { $machine = $env:PROCESSOR_ARCHITECTURE }
        # Windows on Arm runs the x86_64 build under emulation.
        if ($machine -ne 'AMD64' -and $machine -ne 'ARM64') {
            Stop-Install 'unsupported_platform' "there is no oskac build for $machine Windows"
        }
        $platform = 'windows-x86_64'

        $keygen = Get-Command ssh-keygen -CommandType Application -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if (-not $keygen) {
            Stop-Install 'missing_tool' 'ssh-keygen is required to verify the release signature; install the OpenSSH Client (Settings > System > Optional features)'
        }

        if ($PSVersionTable.PSVersion.Major -lt 6) {
            [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        }

        $state.tmp = Join-Path ([IO.Path]::GetTempPath()) ('oska-install-' + [Guid]::NewGuid().ToString('N'))
        [void](New-Item -ItemType Directory -Path $state.tmp)

        if (-not $tag) {
            $latest = Join-Path $state.tmp 'latest.json'
            if (-not (Get-Url "$site/latest.json" $latest)) {
                Stop-Install 'download_failed' "cannot download $site/latest.json"
            }
            try {
                $tag = [string](Get-Content -Raw -LiteralPath $latest | ConvertFrom-Json).tag
            } catch {
                $tag = ''
            }
            # Whatever it names is checked against the signature below.
            if (-not (Test-Tag $tag)) {
                Stop-Install 'download_failed' "$site/latest.json does not name a release"
            }
        }
        $version = $tag.Substring(1)

        $asset = "oskac-$platform.exe"
        $release = "$site/releases/$tag"
        Write-Say "Downloading oskac $version for $platform from $release/"
        foreach ($f in @('SHA256SUMS', 'SHA256SUMS.sig', $asset)) {
            if (-not (Get-Url "$release/$f" (Join-Path $state.tmp $f))) {
                Stop-Install 'download_failed' "cannot download $release/$f (is $version a release?)"
            }
        }

        $allowed = Join-Path $state.tmp 'allowed_signers'
        Write-Utf8 $allowed "$Namespace namespaces=`"$Namespace`" $ReleaseKey`n"
        $sums = Join-Path $state.tmp 'SHA256SUMS'
        $problem = Get-SignatureProblem $keygen.Path $allowed (Join-Path $state.tmp 'SHA256SUMS.sig') $sums
        if ($problem) {
            Stop-Install 'bad_signature' "SHA256SUMS for $tag is not signed by the Oska release key; nothing was installed ($problem)"
        }

        $listed = @([IO.File]::ReadAllLines($sums) | ForEach-Object {
                if ($_ -cmatch '^([0-9a-f]{64})  (\S+)$' -and $Matches[2] -ceq $asset) { $Matches[1] }
            })
        if ($listed.Count -ne 1) {
            Stop-Install 'checksum_mismatch' "SHA256SUMS for $tag does not list $asset exactly once; nothing was installed"
        }
        $downloaded = Join-Path $state.tmp $asset
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $downloaded).Hash.ToLowerInvariant()
        if ($actual -cne $listed[0]) {
            Stop-Install 'checksum_mismatch' "$asset does not match its SHA-256 in SHA256SUMS; nothing was installed"
        }

        $oskaHome = $env:OSKA_HOME
        if (-not $oskaHome) { $oskaHome = Join-Path $HOME '.oska' }
        # An absolute path, so .NET and PowerShell agree on where it is.
        $oskaHome = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($oskaHome)
        $binDir = Join-Path $oskaHome 'bin'
        $target = Join-Path $binDir 'oskac.exe'
        $receipt = Join-Path $oskaHome 'version'
        $previous = $null
        if ((Test-Path -LiteralPath $target -PathType Leaf) -and (Test-Path -LiteralPath $receipt -PathType Leaf)) {
            $previous = ([IO.File]::ReadAllLines($receipt) | Select-Object -First 1)
        }

        try {
            [void](New-Item -ItemType Directory -Force -Path $binDir)
            $staged = Join-Path $binDir '.oskac.new.exe'
            Copy-Item -LiteralPath $downloaded -Destination $staged -Force
            # A running oskac.exe cannot be overwritten, but it can be renamed.
            $old = "$target.old"
            Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
            if (Test-Path -LiteralPath $target) { Move-Item -LiteralPath $target -Destination $old -Force }
            Move-Item -LiteralPath $staged -Destination $target -Force
            Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
            Write-Utf8 $receipt "$version`n"
        } catch {
            Stop-Install 'install_failed' "cannot install to ${target}: $($_.Exception.Message)"
        }

        if (-not $previous) { $action = 'installed' }
        elseif ($previous -eq $version) { $action = 'reinstalled' }
        else { $action = 'updated' }
        $onPath = $false
        foreach ($dir in ($env:Path -split ';')) {
            if ($dir -and $dir.TrimEnd('\') -ieq $binDir.TrimEnd('\')) { $onPath = $true }
        }

        if ($state.json) {
            $result = [ordered]@{
                ok                 = $true
                action             = $action
                version            = $version
                tag                = $tag
                previous_version   = $previous
                platform           = $platform
                path               = $target
                bin_dir            = $binDir
                on_path            = $onPath
                checksum_verified  = $true
                signature_verified = $true
            }
            Write-Output (New-Object PSObject -Property $result | ConvertTo-Json -Compress)
            return
        }

        switch ($action) {
            'installed' { Write-Host "Installed oskac $version to $target." }
            'reinstalled' { Write-Host "Reinstalled oskac $version to $target." }
            'updated' { Write-Host "Updated oskac $previous to $version in $target." }
        }
        Write-Host 'The checksum and the release signature were verified.'
        if (-not $onPath) {
            Write-Host ''
            Write-Host "$binDir is not on your PATH. Add it for your user with:"
            Write-Host ''
            Write-Host "  [Environment]::SetEnvironmentVariable('Path', `"$binDir;`" + [Environment]::GetEnvironmentVariable('Path', 'User'), 'User')"
            Write-Host ''
            Write-Host 'then open a new terminal.'
        }
    }

    try {
        Install-Oskac $args
    } catch {
        $code = $null
        if ($_.Exception.Data.Contains('oska')) { $code = $_.Exception.Data['oska'] }
        if (-not $code) { $code = 'install_failed' }
        $message = $_.Exception.Message
        if ($state.json) {
            $failure = New-Object PSObject -Property ([ordered]@{ ok = $false; error = $code; message = $message })
            Write-Output ($failure | ConvertTo-Json -Compress)
        }
        if ($PSCommandPath) {
            # Run as a file: report and exit with a status, like install.sh.
            if (-not $state.json) { [Console]::Error.WriteLine("install.ps1: $message") }
            if ($code -eq 'usage') { exit 2 }
            exit 1
        }
        # Under `iex` an exit would close the user's window; throw instead.
        throw "install.ps1: $message"
    } finally {
        if ($state.tmp) { Remove-Item -LiteralPath $state.tmp -Recurse -Force -ErrorAction SilentlyContinue }
    }
} @args
