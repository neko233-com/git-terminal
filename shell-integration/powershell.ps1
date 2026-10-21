# Copyright 2026 git-next contributors. MIT license.
if ($env:GIT_TERMINAL_SYNTAX_HIGHLIGHTING -ne '0') {
    Import-Module PSReadLine -ErrorAction SilentlyContinue
}
if ($env:GIT_TERMINAL_GIT_PROMPT -eq '0') { return }
$global:GitTerminalOriginalPrompt = (Get-Item Function:\prompt).ScriptBlock
function global:prompt {
    $previousSuccess = $?
    $branch = ''
    if ($PWD.Provider.Name -eq 'FileSystem' -and $env:GIT_NEXT_EXECUTABLE) {
        $candidate = $PWD.ProviderPath
        $found = [bool]$env:GIT_DIR
        while (-not $found -and $candidate) {
            $found = Test-Path -LiteralPath (Join-Path $candidate '.git')
            if (-not $found) { $candidate = [IO.Path]::GetDirectoryName($candidate) }
        }
        if ($found) {
            $previousNativeExit = $global:LASTEXITCODE
            $branch = & $env:GIT_NEXT_EXECUTABLE -C $PWD.ProviderPath symbolic-ref --quiet --short HEAD 2>$null
            if (-not $branch) { $branch = & $env:GIT_NEXT_EXECUTABLE -C $PWD.ProviderPath rev-parse --short HEAD 2>$null }
            $global:LASTEXITCODE = $previousNativeExit
        }
    }
    if ($branch) { Write-Host ('[' + $branch + '] ') -NoNewline -ForegroundColor Magenta }
    $title = $PWD.Path + $(if ($branch) { ' [' + $branch + ']' } else { '' })
    $Host.UI.RawUI.WindowTitle = [regex]::Replace($title, '[\x00-\x1f\x7f]', '?')
    if ($global:GitTerminalOriginalPrompt) { & $global:GitTerminalOriginalPrompt } else { 'PS ' + $PWD.Path + '> ' }
}
