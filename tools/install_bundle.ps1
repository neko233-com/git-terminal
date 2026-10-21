# Copyright 2026 git-next contributors. MIT license.
param([Parameter(Mandatory)][string]$Package, [string]$Log)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$msi = (Resolve-Path -LiteralPath $Package).Path
$installer = New-Object -ComObject WindowsInstaller.Installer
$database = $installer.OpenDatabase($msi, 0)
function Get-MsiProperty([string]$Name) {
    $view = $database.OpenView('SELECT `Value` FROM `Property` WHERE `Property` = ''' + $Name + '''')
    [void]$view.Execute(); $row = $view.Fetch(); $value = if ($row) { $row.StringData(1) } else { '' }; [void]$view.Close(); return $value
}
if ((Get-MsiProperty 'Manufacturer') -cne 'neko233-com' -or (Get-MsiProperty 'ProductName') -cne 'git-next') { throw 'Select a complete git-next bundle MSI.' }
$product = Get-MsiProperty 'ProductCode'
$state = $installer.ProductState($product)
$arguments = @('/i',('"'+$msi+'"'),'/passive','/norestart')
$temporary = $null
try {
    if ($state -eq 5) {
        $basename = $installer.ProductInfo($product, 'PackageName')
        if (-not $basename -or [IO.Path]::GetFileName($basename) -cne $basename -or $basename -notlike '*.msi') { throw 'Installed MSI source filename is invalid.' }
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('git-terminal-install-' + [Guid]::NewGuid().ToString('N'))
        [void][IO.Directory]::CreateDirectory($temporary)
        $copy = Join-Path $temporary $basename
        Copy-Item -LiteralPath $msi -Destination $copy
        if ((Get-FileHash -LiteralPath $copy).Hash -cne (Get-FileHash -LiteralPath $msi).Hash) { throw 'Installer copy changed.' }
        $arguments = @('/i',('"'+$copy+'"'),'REINSTALL=ALL','REINSTALLMODE=vamus','/passive','/norestart')
    } elseif ($state -ne -1) { throw 'The installed product requires recovery before updating.' }
    if ($Log) { $arguments += @('/l*v',('"'+[IO.Path]::GetFullPath($Log)+'"')) }
    $admin = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    $options = @{FilePath=(Join-Path $env:SystemRoot 'System32\msiexec.exe');ArgumentList=$arguments;Wait=$true;PassThru=$true;WindowStyle='Hidden'}
    $allUsers = Get-MsiProperty 'ALLUSERS'
    if ($allUsers -eq '1' -and -not $admin) { $options.Verb = 'RunAs' }
    $process = Start-Process @options
    if ($process.ExitCode -notin @(0,3010)) { throw ('Windows Installer failed: '+$process.ExitCode) }
    [pscustomobject]@{status='installed';exit_code=$process.ExitCode;repair=($state -eq 5);source_name_preserved=($state -eq 5)} | ConvertTo-Json
} finally {
    if ($temporary) {
        $resolved = [IO.Path]::GetFullPath($temporary)
        $root = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
        if ($resolved.StartsWith($root,[StringComparison]::OrdinalIgnoreCase) -and [IO.Path]::GetFileName($resolved).StartsWith('git-terminal-install-')) { Remove-Item -LiteralPath $resolved -Recurse -Force }
    }
}
