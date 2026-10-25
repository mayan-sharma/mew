# Builds Mew and packs a Velopack installer: dist\releases\Mew-win-Setup.exe
# Requires: Rust (stable MSVC), Visual Studio C++ build tools, Node.js, and `dotnet tool install -g vpk`.
param([string]$Version)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
if (-not $Version) { $Version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value }

# Pinned llama.cpp release (Vulkan build: uses any GPU, falls back to CPU).
$llamaBuild = 'b11714'
$llamaSha256 = 'D4765447307F29FC0B4C8AE5B8B806C46B3E515D0CE5C2A54DD90D9B2C7A392F'
$llamaZip = Join-Path $root ".cache\llama-$llamaBuild-win-vulkan-x64.zip"

Write-Host "Building the Mew window (ui/)"
Push-Location ui
if (-not (Test-Path node_modules)) { npm ci; if ($LASTEXITCODE) { throw 'npm ci failed' } }
npm run build
if ($LASTEXITCODE) { Pop-Location; throw 'ui build failed' }
Pop-Location

Write-Host "Building Mew $Version"
cargo build --release
if ($LASTEXITCODE) { throw 'cargo build failed' }

if (-not (Test-Path $llamaZip)) {
    New-Item -ItemType Directory -Force (Split-Path $llamaZip) | Out-Null
    $url = "https://github.com/ggml-org/llama.cpp/releases/download/$llamaBuild/llama-$llamaBuild-bin-win-vulkan-x64.zip"
    Invoke-WebRequest $url -OutFile $llamaZip
}
if ((Get-FileHash $llamaZip -Algorithm SHA256).Hash -ne $llamaSha256) { throw "llama.cpp zip checksum mismatch" }

$stage = Join-Path $root 'dist\Mew'
$llamaTmp = Join-Path $root 'dist\llama-unpacked'
foreach ($dir in $stage, $llamaTmp) {
    if (Test-Path $dir) { Remove-Item $dir -Recurse -Force }
    New-Item -ItemType Directory $dir | Out-Null
}
New-Item -ItemType Directory (Join-Path $stage 'llama') | Out-Null
Expand-Archive $llamaZip -DestinationPath $llamaTmp

# Visual C++ runtime, app-local, so no separate redistributable install is needed.
$vs = & "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" -latest -property installationPath
$crt = Get-ChildItem "$vs\VC\Redist\MSVC" -Directory | Where-Object Name -Match '^\d+\.\d+' |
    Sort-Object { [version]$_.Name } | Select-Object -Last 1
$crtDir = Get-ChildItem "$($crt.FullName)\x64" -Directory -Filter 'Microsoft.VC*.CRT' | Select-Object -First 1
$crtFiles = 'msvcp140.dll', 'msvcp140_1.dll', 'vcruntime140.dll', 'vcruntime140_1.dll' | ForEach-Object { Join-Path $crtDir.FullName $_ }

Copy-Item target\release\mew.exe, assets\NOTICE.txt -Destination $stage
Copy-Item $crtFiles -Destination $stage
$llamaFiles = @('llama-server.exe', 'llama-server-impl.dll', 'llama.dll', 'llama-common.dll', 'mtmd.dll', 'ggml.dll',
    'ggml-base.dll', 'ggml-vulkan.dll', 'libomp.dll', 'LICENSE-LLVM-OpenMP')
$llamaFiles += (Get-ChildItem $llamaTmp -Filter 'ggml-cpu-*.dll').Name   # CPU backends, picked at runtime
foreach ($f in $llamaFiles) { Copy-Item (Join-Path $llamaTmp $f) (Join-Path $stage 'llama') }
Copy-Item $crtFiles -Destination (Join-Path $stage 'llama')
Remove-Item $llamaTmp -Recurse -Force

$mb = [math]::Round((Get-ChildItem $stage -Recurse -File | Measure-Object Length -Sum).Sum / 1MB, 1)
Write-Host "Staged $mb MB in $stage"

vpk pack --packId Mew --packVersion $Version --packDir $stage --mainExe mew.exe --packTitle Mew --packAuthors Mew `
    --runtime win-x64 --shortcuts StartMenuRoot --icon assets\mew.ico --outputDir dist\releases
if ($LASTEXITCODE) { throw 'vpk pack failed' }
Get-ChildItem dist\releases | Select-Object Name, @{ n = 'MB'; e = { [math]::Round($_.Length / 1MB, 1) } } | Format-Table -AutoSize
