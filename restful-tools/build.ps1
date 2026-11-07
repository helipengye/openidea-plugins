$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    & cargo build --locked --release --target wasm32-wasip2
    if ($LASTEXITCODE -ne 0) { throw 'Plugin build failed.' }
    $outputRoot = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $PSScriptRoot 'target' }
    New-Item -ItemType Directory -Force -Path 'dist' | Out-Null
    Copy-Item -LiteralPath (Join-Path $outputRoot 'wasm32-wasip2/release/openidea_plugin_restful_tools.wasm') -Destination 'dist/plugin.wasm'

    # Do not package a component rebuilt accidentally against a newer SDK.
    $component = [Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes((Join-Path $PSScriptRoot 'dist/plugin.wasm')))
    $interfaces = [regex]::Matches($component, 'openidea:plugin/[a-z-]+@\d+\.\d+\.\d+')
    if ($interfaces.Count -eq 0) { throw 'No OpenIDEA interfaces found in component.' }
    foreach ($interface in $interfaces) {
        if (-not $interface.Value.EndsWith('@1.2.0')) { throw "Incompatible interface: $($interface.Value)" }
    }

    # Match plugins/build.sh's source fingerprint when building in this repo.
    $repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../..'))
    if (Test-Path -LiteralPath (Join-Path $repoRoot 'plugins/build.sh')) {
        $sourceLines = @()
        foreach ($scope in @(@($PSScriptRoot, '.'), @($repoRoot, 'sdk/rust/openidea-plugin/Cargo.toml', 'sdk/rust/openidea-plugin/src', 'sdk/rust/openidea-plugin/wit', 'wit'))) {
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
        [IO.File]::WriteAllText((Join-Path $PSScriptRoot 'dist/plugin.wasm.sources'), $fingerprint + "`n", [Text.UTF8Encoding]::new($false))
    }

    Add-Type -AssemblyName System.IO.Compression
    $packagePath = Join-Path $PSScriptRoot 'dist/community.restful-tools-0.1.2.oiplugin'
    $stream = [IO.File]::Open($packagePath, [IO.FileMode]::Create)
    try {
        $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create)
        try {
            foreach ($name in @('plugin.toml', 'dist/plugin.wasm', 'README.md', 'LICENSE', 'web/index.html', 'web/page.css', 'web/page.js', 'web/locales/en.json', 'web/locales/zh.json')) {
                $entry = $archive.CreateEntry($name, [IO.Compression.CompressionLevel]::Optimal)
                $entryStream = $entry.Open()
                $source = [IO.File]::OpenRead((Join-Path $PSScriptRoot $name))
                try { $source.CopyTo($entryStream) }
                finally { $source.Dispose(); $entryStream.Dispose() }
            }
        } finally { $archive.Dispose() }
    } finally { $stream.Dispose() }
    Write-Output "Built API 1.2 plugin: $packagePath"
} finally { Pop-Location }
