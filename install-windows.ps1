# Claude Code Account Switcher - Windows Quick Installer
# Run with: irm https://raw.githubusercontent.com/.../install-windows.ps1 | iex

param(
    [switch]$SkipPolicyCheck,
    [string]$Version = "latest"
)

$ErrorActionPreference = "Stop"

Write-Host ""
Write-Host "Claude Code Account Switcher - Windows Installer" -ForegroundColor Cyan
Write-Host "================================================" -ForegroundColor Cyan
Write-Host ""

# Check ExecutionPolicy
if (-not $SkipPolicyCheck) {
    Write-Host "[1/5] Checking PowerShell ExecutionPolicy..." -ForegroundColor Yellow
    $policy = Get-ExecutionPolicy -Scope CurrentUser
    if ($policy -eq "Restricted" -or $policy -eq "Undefined") {
        Write-Host "  Current policy: $policy (blocks profile scripts)" -ForegroundColor Red
        Write-Host ""
        Write-Host "  Claude account switcher needs to add shell integration to your PowerShell profile."
        Write-Host "  This requires ExecutionPolicy to be at least RemoteSigned."
        Write-Host ""
        $response = Read-Host "  Set ExecutionPolicy to RemoteSigned for CurrentUser? (Y/N)"
        
        if ($response -match "^[Yy]") {
            try {
                Set-ExecutionPolicy -Scope CurrentUser RemoteSigned -Force
                Write-Host "  ✓ ExecutionPolicy updated successfully" -ForegroundColor Green
            } catch {
                Write-Host "  ✗ Failed to update ExecutionPolicy: $_" -ForegroundColor Red
                exit 1
            }
        } else {
            Write-Host "  Installation cancelled." -ForegroundColor Red
            exit 1
        }
    } else {
        Write-Host "  ✓ ExecutionPolicy is $policy (sufficient)" -ForegroundColor Green
    }
}

Write-Host ""
Write-Host "[2/5] Detecting system..." -ForegroundColor Yellow
$arch = if ([Environment]::Is64BitOperatingSystem) { "x86_64" } else { "i686" }
$assetName = "claude-acc-windows-$arch.exe"
Write-Host "  ✓ Detected: Windows $arch" -ForegroundColor Green

Write-Host ""
Write-Host "[3/5] Fetching latest release..." -ForegroundColor Yellow
try {
    $release = if ($Version -eq "latest") {
        Invoke-RestMethod -Uri "https://api.github.com/repos/Masniper/claude-code-account-switcher/releases/latest"
    } else {
        Invoke-RestMethod -Uri "https://api.github.com/repos/Masniper/claude-code-account-switcher/releases/tags/$Version"
    }
    $asset = $release.assets | Where-Object { $_.name -eq $assetName }
    if (-not $asset) {
        Write-Host "  ✗ Asset $assetName not found" -ForegroundColor Red
        exit 1
    }
    $downloadUrl = $asset.browser_download_url
    $version = $release.tag_name
    Write-Host "  ✓ Found version: $version" -ForegroundColor Green
} catch {
    Write-Host "  ✗ Failed to fetch release: $_" -ForegroundColor Red
    exit 1
}

Write-Host ""
Write-Host "[4/5] Downloading claude-acc.exe..." -ForegroundColor Yellow
$tempFile = Join-Path $env:TEMP "claude-acc-installer.exe"
try {
    $ProgressPreference = "SilentlyContinue"
    Invoke-WebRequest -Uri $downloadUrl -OutFile $tempFile
    $ProgressPreference = "Continue"
    $size = (Get-Item $tempFile).Length / 1MB
    Write-Host "  ✓ Downloaded: $([math]::Round($size, 2)) MB" -ForegroundColor Green
} catch {
    Write-Host "  ✗ Download failed: $_" -ForegroundColor Red
    exit 1
}

Write-Host ""
Write-Host "[5/5] Running claude-acc install..." -ForegroundColor Yellow
Write-Host ""
try {
    & $tempFile install
    if ($LASTEXITCODE -eq 0) {
        Write-Host ""
        Write-Host "✓ Installation complete!" -ForegroundColor Green
        Write-Host ""
        Write-Host "Next steps:" -ForegroundColor Cyan
        Write-Host "  1. Restart PowerShell (or run: . `$PROFILE)" -ForegroundColor White
        Write-Host "  2. Run: claude-acc add work" -ForegroundColor White
        Write-Host "  3. Run: claude-acc link work" -ForegroundColor White
        Write-Host ""
        Remove-Item $tempFile -Force -ErrorAction SilentlyContinue
    } else {
        Write-Host "✗ Installation failed" -ForegroundColor Red
        Write-Host "Binary saved at: $tempFile" -ForegroundColor Yellow
        exit $LASTEXITCODE
    }
} catch {
    Write-Host "✗ Installation error: $_" -ForegroundColor Red
    Write-Host "Binary saved at: $tempFile" -ForegroundColor Yellow
    exit 1
}
