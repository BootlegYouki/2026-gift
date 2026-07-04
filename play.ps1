# play.ps1 - Single-command downloader and runner for BTS Gift Card

$GiftDir = "$HOME/bts-gift"
if (-not (Test-Path $GiftDir)) {
    New-Item -ItemType Directory -Path $GiftDir -Force | Out-Null
}

Set-Location $GiftDir

Write-Host "Unpacking your gift card..." -ForegroundColor Green

# TODO: Replace the URL below with your actual GitHub repository release URL!
$ZipUrl = "https://github.com/YOUR_GITHUB_USERNAME/cmd-tui/releases/download/v1.0.0/gift.zip"
$ZipFile = "$GiftDir/gift.zip"

# Download the bundled ZIP
Invoke-WebRequest -Uri $ZipUrl -OutFile $ZipFile

# Unpack the ZIP (which contains cmd-tui.exe and the downloads/ directory)
Expand-Archive -Path $ZipFile -DestinationPath $GiftDir -Force

# Clean up ZIP download
Remove-Item $ZipFile -Force

# Launch the birthday gift!
Clear-Host
.\cmd-tui.exe
