$ErrorActionPreference = 'Stop'
& (Join-Path $PSScriptRoot '../scripts/build-plugin.ps1') -PluginPath $PSScriptRoot
