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
        $gitDir = $env:GIT_DIR
        $found = [bool]$gitDir
        while (-not $found -and $candidate) {
            $gitDir = Join-Path $candidate '.git'
            $found = Test-Path -LiteralPath $gitDir
            if (-not $found) { $candidate = [IO.Path]::GetDirectoryName($candidate) }
        }
        if ($found) {
            try {
                if (-not [IO.Path]::IsPathRooted($gitDir)) { $gitDir = Join-Path $PWD.ProviderPath $gitDir }
                if ([IO.File]::Exists($gitDir)) {
                    $link = [IO.File]::ReadAllText($gitDir).TrimEnd("`r", "`n")
                    if ($link.StartsWith('gitdir: ')) {
                        $target = $link.Substring(8)
                        if (-not [IO.Path]::IsPathRooted($target)) { $target = Join-Path ([IO.Path]::GetDirectoryName($gitDir)) $target }
                        $gitDir = $target
                    }
                }
                $head = [IO.File]::ReadAllText((Join-Path $gitDir 'HEAD')).TrimEnd("`r", "`n")
                if ($head.StartsWith('ref: refs/heads/')) { $branch = $head.Substring(16) }
                elseif ($head -match '^(?:[0-9a-fA-F]{40}|[0-9a-fA-F]{64})$') { $branch = $head.Substring(0, 7) }
            } catch { $branch = '' }
        }
    }
    if ($branch) { Write-Host ('[' + $branch + '] ') -NoNewline -ForegroundColor Magenta }
    $title = $PWD.Path + $(if ($branch) { ' [' + $branch + ']' } else { '' })
    $Host.UI.RawUI.WindowTitle = [regex]::Replace($title, '[\x00-\x1f\x7f]', '?')
    if ($global:GitTerminalOriginalPrompt) { & $global:GitTerminalOriginalPrompt } else { 'PS ' + $PWD.Path + '> ' }
}
