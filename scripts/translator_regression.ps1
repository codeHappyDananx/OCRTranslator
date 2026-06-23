$ErrorActionPreference = "Stop"
$ProjectRoot = "F:\AI\dn-ocr-translator"
$Providers = @(
  "microsoft",
  "ModernMt",
  "youdaodict",
  "itrans",
  "yandex",
  "papago",
  "bing",
  "qqTranSmart",
  "caiyun",
  "lingva",
  "qqimt",
  "google",
  "ali",
  "deepl_1",
  "TranslateCom",
  "huoshan"
)
$ProbeText = "Cooldown reduction. Quest reward."

function Test-ChineseLikeText {
  param([string]$Text)
  return $Text -match '[\u4e00-\u9fff]'
}

Push-Location $ProjectRoot
try {
  foreach ($Provider in $Providers) {
    Write-Host "[RUN] $Provider"
    $psi = [System.Diagnostics.ProcessStartInfo]::new()
    $psi.FileName = "cargo"
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $safeProvider = $Provider.Replace('"', '\"')
    $safeText = $ProbeText.Replace('"', '\"')
    $psi.Arguments = "run -q -p app-core --example translate_probe -- ""$safeProvider"" ""$safeText"""
    $process = [System.Diagnostics.Process]::Start($psi)
    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()
    $process.WaitForExit()
    $output = ($stdout + $stderr).Trim()
    $exitCode = $process.ExitCode
    if ($exitCode -ne 0) {
      throw "[FAIL] $Provider`n$output"
    }
    $joined = ($output | Out-String).Trim()
    if ([string]::IsNullOrWhiteSpace($joined)) {
      throw "[FAIL] $Provider returned empty text"
    }
    if (-not (Test-ChineseLikeText $joined)) {
      throw "[FAIL] $Provider did not return Chinese-like text: $joined"
    }
    Write-Host "[PASS] $Provider => $joined"
  }
  Write-Host "Translator regression passed."
}
finally {
  Pop-Location
}
