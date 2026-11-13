# Install oskac, the Oska compiler and runner, on Windows.
#
#   irm https://oskalang.dev/install.ps1 | iex
#
# `iex` cannot pass options. To pass them, run the script as a script block:
#
#   & ([scriptblock]::Create((irm https://oskalang.dev/install.ps1))) --version 0.1.0 --json
#
# Options (`-Version`, `-AllowDowngrade` and `-Json` work too):
#   --version X        install release X (0.1.0 or v0.1.0) instead of the latest
#   --allow-downgrade  with --version, install X even if it is older than
#                      the installed oskac
#   --json             print one JSON object on stdout, and nothing else
#   --help             print this help
#
# Environment:
#   OSKA_HOME      install into $env:OSKA_HOME\bin (default: ~\.oska)
#   OSKA_SITE_URL  where to download from (default: https://oskalang.dev);
#                  only https:// and file:// URLs are accepted
#
# Before installing anything it checks that SHA256SUMS carries a valid SSH
# signature by the release key pinned below (with the ssh-keygen of Windows'
# OpenSSH client), that its signed first line names the release asked for
# (`# oska-release vX.Y.Z`, so an old release cannot be passed off as a
# newer one), that the binary's SHA-256 is the one SHA256SUMS lists, and
# that the binary reports that version. If any check fails, or anything
# else goes wrong, it installs nothing and fails: run as a file it exits
# non-zero, under `iex` it throws. It never prompts.
#
# Re-running it updates oskac in place. It never installs an older release
# than the installed one unless asked with --version and --allow-downgrade.
# It records what it installed in $env:OSKA_HOME\receipt.json, which is
# advisory: no check above reads it.
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
    $state = @{ json = $false; tmp = $null; stage = $null; receiptTmp = $null }

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

    # Downloads over HTTPS only, or copies a local file:// URL. Returns
    # $false if the download fails. Redirects are followed here, one hop at a
    # time, and only to https:// URLs: Windows PowerShell 5.1's
    # Invoke-WebRequest would follow one to http://.
    function Get-Url([string]$Url, [string]$OutFile) {
        if ($Url.StartsWith('file://')) {
            try {
                Copy-Item -LiteralPath ([Uri]$Url).LocalPath -Destination $OutFile
                return (Test-Path -LiteralPath $OutFile -PathType Leaf)
            } catch {
                return $false
            }
        }
        $uri = [Uri]$Url
        for ($hop = 0; $hop -le 10; $hop++) {
            if ($uri.Scheme -cne 'https') {
                Stop-Install 'insecure_url' "refusing to download ${uri}: only https:// URLs are allowed, also after a redirect"
            }
            $response = $null
            try {
                $request = [Net.WebRequest]::Create($uri)
                $request.AllowAutoRedirect = $false
                $request.UserAgent = 'oska-install.ps1'
                $response = $request.GetResponse()
                $status = [int]$response.StatusCode
                if ($status -ge 300 -and $status -lt 400) {
                    $location = $response.Headers['Location']
                    if (-not $location) { return $false }
                    $uri = New-Object Uri -ArgumentList $uri, $location
                } else {
                    $body = $response.GetResponseStream()
                    $file = [IO.File]::Create($OutFile)
                    try { $body.CopyTo($file) } finally { $file.Close(); $body.Close() }
                    return $true
                }
            } catch {
                return $false
            } finally {
                if ($response) { $response.Close() }
            }
        }
        return $false
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

    # Numbers of any length, compared as digit strings: -1, 0 or 1.
    function Compare-Number([string]$X, [string]$Y) {
        $X = $X.TrimStart('0')
        $Y = $Y.TrimStart('0')
        if ($X.Length -ne $Y.Length) {
            if ($X.Length -lt $Y.Length) { return -1 }
            return 1
        }
        return [Math]::Sign([string]::CompareOrdinal($X, $Y))
    }

    # -1, 0 or 1 as version $A sorts below, equal to or above $B by semver
    # precedence, so 0.4.0-pre.1 sorts below 0.4.0 and above 0.3.0. Both
    # must be versions as Test-Tag accepts them, without the v. The same
    # order as install.sh's semver_cmp.
    function Compare-Version([string]$A, [string]$B) {
        $ap = ''
        $bp = ''
        $i = $A.IndexOf('-')
        if ($i -ge 0) { $ap = $A.Substring($i + 1); $A = $A.Substring(0, $i) }
        $i = $B.IndexOf('-')
        if ($i -ge 0) { $bp = $B.Substring($i + 1); $B = $B.Substring(0, $i) }
        $x = $A.Split('.')
        $y = $B.Split('.')
        for ($k = 0; $k -lt 3; $k++) {
            $c = Compare-Number $x[$k] $y[$k]
            if ($c -ne 0) { return $c }
        }
        # A release sorts above its pre-releases.
        if (-not $ap -or -not $bp) { return ([int](-not $ap)) - ([int](-not $bp)) }
        $p = $ap.Split('.')
        $q = $bp.Split('.')
        for ($k = 0; $k -lt [Math]::Min($p.Count, $q.Count); $k++) {
            # Numeric identifiers by value and below the others, the others
            # in ASCII order.
            $pn = $p[$k] -cmatch '^[0-9]+$'
            $qn = $q[$k] -cmatch '^[0-9]+$'
            if ($pn -and $qn) { $c = Compare-Number $p[$k] $q[$k] }
            elseif ($pn) { $c = -1 }
            elseif ($qn) { $c = 1 }
            else { $c = [Math]::Sign([string]::CompareOrdinal($p[$k], $q[$k])) }
            if ($c -ne 0) { return $c }
        }
        return [Math]::Sign($p.Count - $q.Count)
    }

    # The version installed now, or $null. The receipt wins over the legacy
    # version file; an install from before receipts has only the latter.
    # Either is advisory: what is not a version counts as nothing installed.
    function Get-PreviousVersion([string]$Target, [string]$Receipt, [string]$VersionFile) {
        if (-not (Test-Path -LiteralPath $Target -PathType Leaf)) { return $null }
        $v = ''
        try {
            if (Test-Path -LiteralPath $Receipt -PathType Leaf) {
                $v = [string]([IO.File]::ReadAllText($Receipt) | ConvertFrom-Json).version
            } elseif (Test-Path -LiteralPath $VersionFile -PathType Leaf) {
                $v = [string]([IO.File]::ReadAllLines($VersionFile) | Select-Object -First 1)
            }
        } catch {
            $v = ''
        }
        if ($v -and (Test-Tag "v$v")) { return $v }
        return $null
    }

    # A name no one can predict, for a staging directory or a temp file.
    function Get-RandomName([string]$Prefix) {
        $Prefix + [IO.Path]::GetRandomFileName().Replace('.', '')
    }

    function Install-Oskac([object[]]$Arguments) {
        $requested = ''
        $allowDowngrade = $false
        $mode = ''
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
            } elseif ($lower -eq '--allow-downgrade' -or $lower -eq '-allowdowngrade') {
                $allowDowngrade = $true
            } elseif ($lower -eq '--check' -or $lower -eq '-check' -or $lower -eq '--uninstall' -or $lower -eq '-uninstall') {
                $m = '--' + $lower.TrimStart('-')
                if ($mode -and $mode -ne $m) {
                    Stop-Install 'usage' "$mode and $m cannot be used together; pick one"
                }
                $mode = $m
            } elseif ($lower -eq '--help' -or $lower -eq '-help' -or $lower -eq '-h') {
                Write-Host 'Install oskac to ~\.oska\bin, verifying its checksum and release signature.'
                Write-Host ''
                Write-Host '  --version X        install release X (0.1.0 or v0.1.0) instead of the latest'
                Write-Host '  --allow-downgrade  with --version, install X even if it is older than'
                Write-Host '                     the installed oskac'
                Write-Host '  --json             print one JSON object on stdout, and nothing else'
                Write-Host '  --help             print this help'
                Write-Host ''
                Write-Host 'Environment: OSKA_HOME (default ~\.oska), OSKA_SITE_URL (https:// only).'
                return
            } else {
                Stop-Install 'usage' "unknown option $a (try --help)"
            }
            $i++
        }
        # Install is the default mode; --check and --uninstall are the
        # others. --version and --allow-downgrade belong to install alone.
        if ($mode) {
            if ($requested) { Stop-Install 'usage' "--version cannot be used with $mode" }
            if ($allowDowngrade) { Stop-Install 'usage' "--allow-downgrade cannot be used with $mode" }
            Stop-Install 'usage' "$mode is not available in this installer yet"
        }
        if ($allowDowngrade -and -not $requested) {
            Stop-Install 'usage' '--allow-downgrade needs --version naming the release to go back to'
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

        $oskaHome = $env:OSKA_HOME
        if (-not $oskaHome) { $oskaHome = Join-Path $HOME '.oska' }
        # An absolute path, so .NET and PowerShell agree on where it is.
        $oskaHome = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($oskaHome)
        $binDir = Join-Path $oskaHome 'bin'
        $target = Join-Path $binDir 'oskac.exe'
        $versionFile = Join-Path $oskaHome 'version'
        $receipt = Join-Path $oskaHome 'receipt.json'
        $previous = Get-PreviousVersion $target $receipt $versionFile

        # The downgrade guard. An older release is installed only when it is
        # named with --version and --allow-downgrade, so a stale
        # latest.json, an old script or a re-run never moves an install back.
        if ($previous -and (Compare-Version $version $previous) -lt 0) {
            if (-not $requested) {
                Stop-Install 'downgrade_refused' "oskac $previous is installed, which is newer than the latest release, $version; nothing was installed (to go back, pass --version $version --allow-downgrade)"
            } elseif (-not $allowDowngrade) {
                Stop-Install 'downgrade_refused' "oskac $previous is installed, which is newer than $version; nothing was installed (to downgrade, add --allow-downgrade)"
            }
        }

        # A running oskac.exe cannot be overwritten, only renamed aside to
        # oskac.exe.old, which a run cannot delete while it is running.
        # Remove one left by an earlier run.
        $old = "$target.old"
        if (Test-Path -LiteralPath $old -PathType Leaf) {
            Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
        }

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

        # The signed first line names the release. A genuine old release
        # served under a newer tag has a valid signature, but names its own
        # tag here.
        $signed = [IO.File]::ReadAllLines($sums) | Select-Object -First 1
        if ($signed -cne "# oska-release $tag") {
            if ($signed -and $signed.StartsWith('# oska-release ')) {
                Stop-Install 'version_mismatch' "the files at $release/ are signed as release $($signed.Substring(15)), not $tag; nothing was installed"
            }
            Stop-Install 'version_mismatch' "SHA256SUMS at $release/ does not name the release it is for; nothing was installed"
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

        $pub = Join-Path $state.tmp 'release.pub'
        Write-Utf8 $pub "$ReleaseKey`n"
        $fingerprint = ''
        try {
            $fingerprint = [string](@(& $keygen.Path -l -f $pub 2>$null) | Select-Object -First 1)
            $fingerprint = ($fingerprint -split ' ')[1]
        } catch {
            $fingerprint = ''
        }
        if (-not ($fingerprint -and $fingerprint.StartsWith('SHA256:'))) {
            Stop-Install 'install_failed' 'ssh-keygen cannot fingerprint the release key'
        }

        # Stage in a new directory under a name no one can predict, made next
        # to the target so the final move stays on one volume. New-Item
        # refuses a name that exists, so nothing planted in $binDir is ever
        # written through.
        try {
            [void](New-Item -ItemType Directory -Force -Path $binDir)
            $stage = Join-Path $binDir (Get-RandomName '.oskac.')
            [void](New-Item -ItemType Directory -Path $stage)
            $state.stage = $stage
            $staged = Join-Path $stage 'oskac.exe'
            Copy-Item -LiteralPath $downloaded -Destination $staged
        } catch {
            Stop-Install 'install_failed' "cannot create a staging directory in ${binDir}: $($_.Exception.Message)"
        }

        $reported = ''
        try {
            $reported = ([string](@(& $staged --version 2>$null) | Select-Object -First 1)).Trim()
        } catch {
            $reported = ''
        }
        if ($reported -cne "oskac $version") {
            Stop-Install 'version_mismatch' "the $tag binary reports '$reported', not 'oskac $version'; nothing was installed"
        }

        try {
            # A running oskac.exe cannot be overwritten, but it can be renamed.
            Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
            if (Test-Path -LiteralPath $target) { Move-Item -LiteralPath $target -Destination $old -Force }
            Move-Item -LiteralPath $staged -Destination $target -Force
            Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue
            Write-Utf8 $versionFile "$version`n"
        } catch {
            Stop-Install 'install_failed' "cannot install to ${target}: $($_.Exception.Message)"
        }

        # The install receipt: what this run installed. It is advisory.
        # Verification never reads it; it only records what counts as
        # installed, what an uninstall may remove and the highest index
        # serial seen. Written to a new file beside it, then renamed into
        # place.
        $files = @(
            [ordered]@{ path = $target; kind = 'binary'; sha256 = $actual },
            [ordered]@{ path = $versionFile; kind = 'record'; sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $versionFile).Hash.ToLowerInvariant() },
            [ordered]@{ path = $receipt; kind = 'record' }
        )
        $record = [ordered]@{
            schema          = 1
            version         = $version
            tag             = $tag
            platform        = $platform
            installed_at    = [DateTime]::UtcNow.ToString("yyyy-MM-dd'T'HH:mm:ss'Z'", [Globalization.CultureInfo]::InvariantCulture)
            site            = $site
            key_fingerprint = $fingerprint
            sha256          = $actual
            installer       = 'ps1'
            index_serial    = 0
            files           = $files
        }
        try {
            $state.receiptTmp = Join-Path $oskaHome (Get-RandomName '.receipt.')
            Write-Utf8 $state.receiptTmp (($record | ConvertTo-Json -Depth 5) + "`n")
            if (Test-Path -LiteralPath $receipt) {
                # [NullString]: PowerShell would pass $null as '', which
                # Replace takes as a backup file name.
                [IO.File]::Replace($state.receiptTmp, $receipt, [NullString]::Value)
            } else {
                [IO.File]::Move($state.receiptTmp, $receipt)
            }
            $state.receiptTmp = $null
        } catch {
            Stop-Install 'install_failed' "cannot write ${receipt}: $($_.Exception.Message)"
        }

        if (-not $previous) { $action = 'installed' }
        else {
            switch (Compare-Version $version $previous) {
                1 { $action = 'updated' }
                0 { $action = 'reinstalled' }
                default { $action = 'downgraded' }
            }
        }
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
                receipt            = $receipt
            }
            Write-Output (New-Object PSObject -Property $result | ConvertTo-Json -Compress)
            return
        }

        switch ($action) {
            'installed' { Write-Host "Installed oskac $version to $target." }
            'reinstalled' { Write-Host "Reinstalled oskac $version to $target." }
            'updated' { Write-Host "Updated oskac $previous to $version in $target." }
            'downgraded' { Write-Host "Downgraded oskac $previous to $version in $target." }
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
        if ($state.stage) { Remove-Item -LiteralPath $state.stage -Recurse -Force -ErrorAction SilentlyContinue }
        if ($state.receiptTmp) { Remove-Item -LiteralPath $state.receiptTmp -Force -ErrorAction SilentlyContinue }
    }
} @args
