# uninstall.ps1 - Uninstaller for BTS Gift Card

$GiftDir = "$HOME/bts-gift"
$ResolvedGiftDir = [System.IO.Path]::GetFullPath($GiftDir)

if (Test-Path $GiftDir) {
    Remove-Item -Recurse -Force $GiftDir
    Write-Host "Removed $GiftDir" -ForegroundColor Green
}

# Clean from User PATH
$RegPath = "HKCU:\Environment"
$CurrentPath = (Get-ItemProperty -Path $RegPath -Name Path -ErrorAction SilentlyContinue).Path
if ($CurrentPath) {
    $Paths = $CurrentPath -split ';' | Where-Object { 
        $_ -and $_ -ne $GiftDir -and $_ -ne $ResolvedGiftDir 
    }
    $NewPath = $Paths -join ';'
    Set-ItemProperty -Path $RegPath -Name Path -Value $NewPath
    Write-Host "Cleaned User PATH." -ForegroundColor Cyan
}

Write-Host "BTS Gift Card successfully uninstalled." -ForegroundColor Magenta
