#requires -Version 5.1

<#
.SYNOPSIS
Builds a local Maple Agent Market character/skill pack from public recipes.

.DESCRIPTION
Repository-distributed code and recipe IDs are MIT licensed. Third-party
MapleStory renders are fetched only after the caller explicitly accepts the
notice, are written below a local `private-assets` directory, and remain
outside the repository license and public release artifacts.
#>
[CmdletBinding()]
param(
    [string]$ProjectRoot,
    [string]$OutputRoot,
    [string]$ValidatorExe,
    [string]$RecipePath,
    [switch]$StarterOnly,
    [switch]$IncludeClassicSkills,
    [switch]$AcceptThirdPartyAssetNotice
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($PSVersionTable.PSEdition -ne 'Desktop') {
    throw 'UNSUPPORTED_POWERSHELL|請用 Windows PowerShell 5.1 的 powershell.exe 執行；目前的 pwsh / PowerShell Core 無法載入此工具所需的 System.Drawing。'
}

function Get-Utf8NoBom { New-Object System.Text.UTF8Encoding($false) }

function Resolve-Validator {
    param([string]$Root, [string]$ExplicitPath)
    if (-not [string]::IsNullOrWhiteSpace($ExplicitPath)) {
        $full = [IO.Path]::GetFullPath($ExplicitPath)
        if (-not (Test-Path -LiteralPath $full -PathType Leaf)) { throw "找不到 validate-pack 執行檔：$full" }
        return $full
    }
    $candidates = @(
        (Join-Path $Root 'maple-agent-market.exe'),
        (Join-Path $Root 'bin\maple-agent-market.exe'),
        (Join-Path $Root 'target\debug\maple-agent-market.exe'),
        (Join-Path $Root 'target\release\maple-agent-market.exe')
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { return [IO.Path]::GetFullPath($candidate) }
    }
    throw '找不到 maple-agent-market.exe。請先執行 cargo build --bin maple-agent-market，或以 -ValidatorExe 指定檔案。'
}

$scriptDirectory = [IO.Path]::GetDirectoryName($MyInvocation.MyCommand.Path)
if ([string]::IsNullOrWhiteSpace($ProjectRoot)) { $ProjectRoot = Split-Path -Parent $scriptDirectory }
$ProjectRoot = [IO.Path]::GetFullPath($ProjectRoot)
if ([string]::IsNullOrWhiteSpace($OutputRoot)) { $OutputRoot = Join-Path $ProjectRoot 'private-assets\skins' }
$OutputRoot = [IO.Path]::GetFullPath($OutputRoot)
if ([string]::IsNullOrWhiteSpace($RecipePath)) { $RecipePath = Join-Path $scriptDirectory 'recipes\community-maple-atelier.json' }
$RecipePath = [IO.Path]::GetFullPath($RecipePath)

if (-not $StarterOnly -and -not $AcceptThirdPartyAssetNotice) {
    throw 'THIRD_PARTY_NOTICE_REQUIRED|下載前請閱讀 README 的素材界線，並加上 -AcceptThirdPartyAssetNotice。'
}
if ($IncludeClassicSkills -and $StarterOnly) {
    throw 'INPUT_INVALID|-StarterOnly 與 -IncludeClassicSkills 不能同時使用。'
}
if (-not (Test-Path -LiteralPath $RecipePath -PathType Leaf)) { throw "找不到角色配方：$RecipePath" }

$validator = Resolve-Validator -Root $ProjectRoot -ExplicitPath $ValidatorExe
$generator = Join-Path $scriptDirectory 'New-MapleStarterPack.ps1'
$workshop = Join-Path $scriptDirectory 'MapleSkinWorkshop.psm1'
$syncCharacters = Join-Path $scriptDirectory 'Sync-MapleAtelierCharacters.ps1'
$syncSkills = Join-Path $scriptDirectory 'Sync-ClassicSkillEffects.ps1'
$importCharacter = Join-Path $scriptDirectory 'Import-MapleAtelierCharacter.ps1'
foreach ($required in @($generator, $workshop, $syncCharacters, $syncSkills, $importCharacter)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "公開素材工具不完整：$required" }
}

[void][IO.Directory]::CreateDirectory($OutputRoot)
$basePack = Join-Path $OutputRoot 'base-pack'
if (-not (Test-Path -LiteralPath (Join-Path $basePack 'pack.toml') -PathType Leaf)) {
    if (Test-Path -LiteralPath $basePack) { throw "基底素材目錄已存在但不完整，拒絕覆寫：$basePack" }
    & $generator -OutputPath $basePack | Out-Null
}
& $validator validate-pack $basePack | Out-Null
if ($LASTEXITCODE -ne 0) { throw '原創基底素材包未通過 validate-pack。' }

$catalogPath = Join-Path $OutputRoot 'maple-atelier-catalog.json'
if ($StarterOnly) {
    $emptyCatalog = [pscustomobject]@{
        schemaVersion = 1
        generatedUtc = [DateTime]::UtcNow.ToString('o')
        notice = 'Original procedural starter characters only; no third-party raster output.'
        characters = @()
    }
    [IO.File]::WriteAllText($catalogPath, (($emptyCatalog | ConvertTo-Json -Depth 5) + [Environment]::NewLine), (Get-Utf8NoBom))
    Import-Module -Name $workshop -Force
    $catalogPack = New-MapleCatalogSkinPack -BasePack $basePack -SkinRoot $OutputRoot -CatalogPath $catalogPath -ValidatorExe $validator
} else {
    $recipe = [IO.File]::ReadAllText($RecipePath, [Text.Encoding]::UTF8) | ConvertFrom-Json
    $outfitIds = @($recipe.outfitIds | ForEach-Object { [int]$_ })
    if ($outfitIds.Count -lt 1) { throw '角色配方沒有 outfitIds。' }
    [void](& $syncCharacters -OutfitIds $outfitIds -SkinRoot $OutputRoot -PackToml (Join-Path $basePack 'pack.toml') -BasePack $basePack -ValidatorExe $validator -WorkshopModule $workshop)
    foreach ($render in @($recipe.characterRenders)) {
        $childOutput = @(& powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $importCharacter -InputUrl ([string]$render.url) -DisplayName ([string]$render.title) -SkinRoot $OutputRoot -PackToml (Join-Path $basePack 'pack.toml') -BasePack $basePack -ValidatorExe $validator -WorkshopModule $workshop -NoCompletionDialog 2>&1)
        if ($LASTEXITCODE -ne 0) {
            throw ('自訂角色配方匯入失敗：' + ($childOutput -join [Environment]::NewLine))
        }
    }
    $catalogPack = Join-Path $OutputRoot 'catalog-pack'
    if ($IncludeClassicSkills) {
        $expectedDefault = [IO.Path]::GetFullPath((Join-Path $ProjectRoot 'private-assets\skins'))
        if (-not $OutputRoot.Equals($expectedDefault, [StringComparison]::OrdinalIgnoreCase)) {
            throw '技能同步目前只支援 ProjectRoot\private-assets\skins，避免寫到意外的角色工作區。'
        }
        & $syncSkills -InstallRoot $ProjectRoot -BasePack $basePack -AssetRoot (Join-Path $ProjectRoot 'private-assets\skills') -ValidatorExe $validator -WorkshopModule $workshop -CharacterCatalog $catalogPath | Out-Null
    }
}

& $validator validate-pack $catalogPack | Out-Null
if ($LASTEXITCODE -ne 0) { throw '本機角色 catalog-pack 未通過 validate-pack。' }
$manifestText = [IO.File]::ReadAllText((Join-Path $catalogPack 'pack.toml'), [Text.Encoding]::UTF8)
$characterCount = 0
if ($manifestText -match '(?s)\[characters\].*?names\s*=\s*\[(?<names>.*?)\]') {
    $characterCount = @([regex]::Matches($matches.names, '"(?:[^"\\]|\\.)*"')).Count
}

$result = [ordered]@{
    ok = $true
    profile = if ($StarterOnly) { 'original-starter' } else { 'community-local' }
    characterCount = $characterCount
    basePack = [IO.Path]::GetFullPath($basePack)
    catalogPack = [IO.Path]::GetFullPath($catalogPack)
    catalog = [IO.Path]::GetFullPath($catalogPath)
    includesClassicSkills = [bool]$IncludeClassicSkills
    launchArguments = @('--theme', 'maple', 'floating', '--pack-dir', [IO.Path]::GetFullPath($catalogPack))
    rightsNotice = 'Generated third-party renders remain local-only and are not covered by the repository MIT license.'
}
Write-Output ($result | ConvertTo-Json -Depth 6 -Compress)
