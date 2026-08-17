# play.ps1 - Single-command downloader and runner for BTS Gift Card

$GiftDir = "$HOME/bts-gift"
if (-not (Test-Path $GiftDir)) {
    New-Item -ItemType Directory -Path $GiftDir -Force | Out-Null
}

Set-Location $GiftDir

Write-Host "Unpacking your gift card..." -ForegroundColor Green

# TODO: Replace the URL below with your actual GitHub repository release URL!
$ZipUrl = "https://github.com/BootlegYouki/2026-gift/releases/download/v1.0.0/gift.zip"
$ZipFile = "$GiftDir/gift.zip"

# Download the bundled ZIP
Invoke-WebRequest -Uri $ZipUrl -OutFile $ZipFile

# Unpack the ZIP (which contains gift.exe and the downloads/ directory)
Expand-Archive -Path $ZipFile -DestinationPath $GiftDir -Force

# Clean up ZIP download
Remove-Item $ZipFile -Force

# Automatically add the folder to the User PATH if not already present
$RegPath = "HKCU:\Environment"
$CurrentPath = (Get-ItemProperty -Path $RegPath -Name Path -ErrorAction SilentlyContinue).Path
$ResolvedGiftDir = [System.IO.Path]::GetFullPath($GiftDir)

if (-not $CurrentPath) {
    Set-ItemProperty -Path $RegPath -Name Path -Value $ResolvedGiftDir
    $env:Path = "$env:Path;$ResolvedGiftDir"
    Write-Host "Added $ResolvedGiftDir to User PATH." -ForegroundColor Cyan
} elseif (($CurrentPath -split ';') -notcontains $ResolvedGiftDir) {
    # Ensure we don't double-append semicolons
    $Separator = if ($CurrentPath.EndsWith(';')) { "" } else { ";" }
    $NewPath = "$CurrentPath$Separator$ResolvedGiftDir"
    Set-ItemProperty -Path $RegPath -Name Path -Value $NewPath
    $env:Path = "$env:Path;$ResolvedGiftDir"
    Write-Host "Added $ResolvedGiftDir to User PATH." -ForegroundColor Cyan
}

# Launch the birthday gift!
Clear-Host
.\gift.exe

