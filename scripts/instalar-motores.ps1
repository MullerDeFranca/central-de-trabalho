# instalar-motores.ps1 — baixa o motor de upscale IA (sidecar)
# Uso: powershell -File scripts/instalar-motores.ps1
$ErrorActionPreference = 'Stop'
$dest = Join-Path $PSScriptRoot '..\bin\realesrgan'
if (Test-Path (Join-Path $dest 'realesrgan-ncnn-vulkan.exe')) {
  Write-Host "Motor já instalado em $dest"
  exit 0
}
$url = 'https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesrgan-ncnn-vulkan-20220424-windows.zip'
$zip = Join-Path $env:TEMP 'realesrgan.zip'
Write-Host 'Baixando Real-ESRGAN ncnn Vulkan (~43 MB)...'
Invoke-WebRequest -Uri $url -OutFile $zip
$tmp = Join-Path $env:TEMP ('realesrgan-' + [Guid]::NewGuid())
Expand-Archive $zip -DestinationPath $tmp
$inner = Get-ChildItem $tmp -Directory | Select-Object -First 1
New-Item -ItemType Directory -Force -Path $dest | Out-Null
Copy-Item (Join-Path $inner.FullName '*') $dest -Recurse -Force
Remove-Item $zip, $tmp -Recurse -Force
Write-Host "Motor instalado em $dest"
