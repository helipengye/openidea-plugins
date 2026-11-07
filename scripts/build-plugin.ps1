param([Parameter(Mandatory = $true)][string]$PluginPath)

$ErrorActionPreference = 'Stop'
Push-Location (Resolve-Path -LiteralPath $PluginPath).Path
try {
    $pluginRoot = (Get-Location).Path
    $manifest = Get-Content -LiteralPath 'plugin.toml' -Raw
    $pluginId = [regex]::Match($manifest, '(?m)^id\s*=\s*"([^"]+)"').Groups[1].Value
    $version = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
    if (-not $pluginId -or -not $version) { throw 'Plugin id and version are required.' }

    & cargo build --locked --release --target wasm32-wasip2
    if ($LASTEXITCODE -ne 0) { throw 'Plugin build failed.' }
    $metadata = & cargo metadata --locked --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Could not resolve plugin dependencies.' }
    $package = $metadata.packages | Where-Object id -eq $metadata.resolve.root
    $crateName = $package.name.Replace('-', '_')
    $componentPath = Join-Path $metadata.target_directory "wasm32-wasip2/release/$crateName.wasm"
    New-Item -ItemType Directory -Force -Path 'dist' | Out-Null
    Copy-Item -LiteralPath $componentPath -Destination 'dist/plugin.wasm'

    # Keep local packages compatible with the pinned API 1.2 SDK.
    $component = [Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes((Join-Path $pluginRoot 'dist/plugin.wasm')))
    $interfaces = [regex]::Matches($component, 'openidea:plugin/[a-z-]+@\d+\.\d+\.\d+')
    if ($interfaces.Count -eq 0) { throw 'No OpenIDEA interfaces found in component.' }
    foreach ($interface in $interfaces) {
        if (-not $interface.Value.EndsWith('@1.2.0')) { throw "Incompatible interface: $($interface.Value)" }
    }

    # Fingerprint plugin sources and the SDK actually resolved by Cargo.
    $sdkManifest = ($metadata.packages | Where-Object name -eq 'openidea-plugin').manifest_path
    $sdkRoot = [IO.Path]::GetFullPath((Join-Path (Split-Path $sdkManifest -Parent) '../../..'))
    $sourceLines = @()
    foreach ($scope in @(@($pluginRoot, '.'), @($sdkRoot, 'sdk/rust/openidea-plugin/Cargo.toml', 'sdk/rust/openidea-plugin/src', 'sdk/rust/openidea-plugin/wit', 'wit'))) {
        [string[]]$files = @(& git -C $scope[0] ls-files --cached --others --exclude-standard -- $scope[1..($scope.Count - 1)])
        if ($LASTEXITCODE -ne 0) { throw 'Could not fingerprint plugin sources.' }
        [Array]::Sort($files, [StringComparer]::Ordinal)
        foreach ($file in ($files | Select-Object -Unique)) {
            if ($file -match '(^|/)(dist|target|node_modules)/|(^|/)\.DS_Store$|^web(-src)?/|^web\.sources$') { continue }
            $full = Join-Path $scope[0] $file
            if (Test-Path -LiteralPath $full -PathType Leaf) {
                $hash = (Get-FileHash -LiteralPath $full -Algorithm SHA256).Hash.ToLowerInvariant()
                $sourceLines += "$file $hash"
            }
        }
    }
    $sha = [Security.Cryptography.SHA256]::Create()
    try { $digest = $sha.ComputeHash([Text.Encoding]::UTF8.GetBytes(($sourceLines -join "`n") + "`n")) }
    finally { $sha.Dispose() }
    $fingerprint = ([BitConverter]::ToString($digest)).Replace('-', '').ToLowerInvariant()
    [IO.File]::WriteAllText((Join-Path $pluginRoot 'dist/plugin.wasm.sources'), $fingerprint + "`n", [Text.UTF8Encoding]::new($false))

    $files = @('plugin.toml', 'dist/plugin.wasm', 'README.md', 'README.zh-CN.md', 'LICENSE')
    if (Test-Path -LiteralPath 'web' -PathType Container) {
        $files += Get-ChildItem -LiteralPath 'web' -File -Recurse | ForEach-Object {
            $_.FullName.Substring($pluginRoot.Length + 1).Replace('\', '/')
        }
    }
    Add-Type -AssemblyName System.IO.Compression
    $packagePath = Join-Path $pluginRoot "dist/$pluginId-$version.oiplugin"
    $stream = [IO.File]::Open($packagePath, [IO.FileMode]::Create)
    try {
        $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create)
        try {
            foreach ($name in $files) {
                $entry = $archive.CreateEntry($name, [IO.Compression.CompressionLevel]::Optimal)
                $entryStream = $entry.Open()
                $source = [IO.File]::OpenRead((Join-Path $pluginRoot $name))
                try { $source.CopyTo($entryStream) }
                finally { $source.Dispose(); $entryStream.Dispose() }
            }
        } finally { $archive.Dispose() }
    } finally { $stream.Dispose() }
    Write-Output "Built unsigned API 1.2 plugin: $packagePath"
} finally { Pop-Location }
