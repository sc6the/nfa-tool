# Launch only the locally built app with isolated account storage and a debug port.
# No Steam login/logout operation is performed by this helper.
$ErrorActionPreference = 'Stop'
$root = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$data = Join-Path $root 'artifacts\native-smoke-data'
New-Item -ItemType Directory -Path $data -Force | Out-Null
$previousAppData = $env:APPDATA
$previousBrowserArgs = $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
try {
    $env:APPDATA = $data
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9224'
    $process = Start-Process -FilePath (Join-Path $root 'nfa.exe') -WorkingDirectory $root -WindowStyle Hidden -PassThru
    Write-Output "Native smoke process ID: $($process.Id)"
} finally {
    $env:APPDATA = $previousAppData
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $previousBrowserArgs
}
