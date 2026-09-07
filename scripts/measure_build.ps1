#!/usr/bin/env pwsh
<#
.SYNOPSIS
    M1 任务 4：编译时间基线测量。

.DESCRIPTION
    测量增量构建耗时（取 Runs 次中的较小值，避开首次磁盘缓存抖动）：
      noop       无改动（样式改动在 M4 走热重载，不触发重编译；此处即 cargo 自身开销）
      structure  结构改动（内核 lieui-core/src/tree.rs —— 影响面最大）
      props      属性表改动（lieui-core/src/props/keys.rs）
      leaf       叶子改动（lieui-text/src/spec.rs —— 最下游 crate）
    加 -WithCold 额外测量 clean 后的全量构建（约 1 分钟）。

.EXAMPLE
    .\scripts\measure_build.ps1
    .\scripts\measure_build.ps1 -UpdateDoc -WithCold
#>
param(
    [switch]$UpdateDoc,
    [switch]$WithCold,
    [int]$Runs = 2
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# cargo 会把进度写到 stderr；PS 5.1 下 `2>&1` 会变成 ErrorRecord 并在 EAP=Stop 时中断，
# 因此统一走 cmd 的重定向，两个 PowerShell 版本行为一致。
function Invoke-CargoBuild {
    cmd /c "cargo build --workspace --examples >nul 2>&1"
    if ($LASTEXITCODE -ne 0) { throw "cargo build 失败（exit=$LASTEXITCODE）" }
}

function Touch([string]$p) {
    if (-not (Test-Path $p)) { throw "找不到文件：$p" }
    (Get-Item $p).LastWriteTime = Get-Date
}

function Measure-Build([string]$label, [scriptblock]$prepare) {
    $best = [double]::MaxValue
    for ($i = 0; $i -lt $Runs; $i++) {
        & $prepare
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        Invoke-CargoBuild
        $sw.Stop()
        $best = [Math]::Min($best, $sw.Elapsed.TotalMilliseconds)
    }
    [pscustomobject]@{ 场景 = $label; 耗时_ms = [int]$best }
}

Write-Host '== 预热 ==' -ForegroundColor Cyan
Invoke-CargoBuild

Write-Host '== 增量场景 ==' -ForegroundColor Cyan
$results = @(
    (Measure-Build 'noop（无改动）' { })
    (Measure-Build 'structure（lieui-core/src/tree.rs）' { Touch 'crates/lieui-core/src/tree.rs' })
    (Measure-Build 'props（lieui-core/src/props/keys.rs）' { Touch 'crates/lieui-core/src/props/keys.rs' })
    (Measure-Build 'leaf（lieui-text/src/spec.rs）' { Touch 'crates/lieui-text/src/spec.rs' })
)

if ($WithCold) {
    Write-Host '== 冷构建（clean）==' -ForegroundColor Cyan
    cmd /c "cargo clean >nul 2>&1"
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    Invoke-CargoBuild
    $sw.Stop()
    $results = ,([pscustomobject]@{ 场景 = 'cold（clean 后全量）'; 耗时_ms = [int]$sw.Elapsed.TotalMilliseconds }) + $results
}

$results | ForEach-Object { '{0,-42} {1,7} ms' -f $_.场景, $_.耗时_ms }

if ($UpdateDoc) {
    $lines = @('| 场景 | 耗时 |', '|---|---|')
    foreach ($r in $results) { $lines += ('| {0} | {1} ms |' -f $r.场景, $r.耗时_ms) }
    $table = $lines -join "`n"
    $doc = Get-Content (Join-Path $root 'docs/m1-build-times.md') -Raw
    $doc = [regex]::Replace($doc, '(?s)<!-- MEASURED:BEGIN -->.*<!-- MEASURED:END -->', "<!-- MEASURED:BEGIN -->`n$table`n<!-- MEASURED:END -->")
    [System.IO.File]::WriteAllText((Join-Path $root 'docs/m1-build-times.md'), $doc, (New-Object System.Text.UTF8Encoding $false))
    Write-Host '已更新 docs/m1-build-times.md' -ForegroundColor Green
}
