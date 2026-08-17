#requires -Version 5.1

[CmdletBinding()]
param(
    [string]$RepositoryRoot,
    [string]$ValidatorExe
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) {
    $scriptDirectory = [IO.Path]::GetDirectoryName($MyInvocation.MyCommand.Path)
    $RepositoryRoot = Split-Path -Parent (Split-Path -Parent $scriptDirectory)
}

function Assert-True {
    param([bool]$Condition, [Parameter(Mandatory = $true)][string]$Message)
    if (-not $Condition) { throw "ASSERTION_FAILED|$Message" }
}

$generator = Join-Path $RepositoryRoot 'tools\New-MapleStarterPack.ps1'
$bootstrap = Join-Path $RepositoryRoot 'tools\Bootstrap-MapleLocalAssets.ps1'
$workshop = Join-Path $RepositoryRoot 'tools\MapleSkinWorkshop.psm1'
$recipe = Join-Path $RepositoryRoot 'tools\recipes\community-maple-atelier.json'

$friendlyLauncher = Join-Path $RepositoryRoot '建立本機素材.cmd'

Assert-True (Test-Path -LiteralPath $generator -PathType Leaf) 'starter-pack generator is missing'
Assert-True (Test-Path -LiteralPath $bootstrap -PathType Leaf) 'local asset bootstrap is missing'
Assert-True (Test-Path -LiteralPath $workshop -PathType Leaf) 'skin workshop is missing'
Assert-True (Test-Path -LiteralPath $recipe -PathType Leaf) 'curated character recipe is missing'
Assert-True (Test-Path -LiteralPath $friendlyLauncher -PathType Leaf) 'friendly asset launcher is missing'
$recipeData = Get-Content -LiteralPath $recipe -Raw -Encoding UTF8 | ConvertFrom-Json
Assert-True (@($recipeData.outfitIds).Count -eq 19) 'community recipe must pin nineteen public outfits'
Assert-True (@($recipeData.characterRenders).Count -eq 1) 'community recipe must pin the current custom TWMS look'
$launcherText = Get-Content -LiteralPath $friendlyLauncher -Raw -Encoding UTF8
Assert-True ($launcherText.Contains('-IncludeClassicSkills')) 'friendly launcher must rebuild the curated BB-pre skill set'

if ([string]::IsNullOrWhiteSpace($ValidatorExe)) {
    $candidates = @(
        (Join-Path $RepositoryRoot 'target\debug\maple-agent-market.exe'),
        (Join-Path $RepositoryRoot 'target\release\maple-agent-market.exe')
    )
    $foundValidator = @($candidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1)
    if ($foundValidator.Count -gt 0) { $ValidatorExe = [string]$foundValidator[0] } else { $ValidatorExe = $null }
}

$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ('MapleAgentMarket-PublicBootstrapTest-' + [guid]::NewGuid().ToString('N'))
try {
    $basePack = Join-Path $tempRoot 'private-assets\skins\base-pack'
    & $generator -OutputPath $basePack

    Assert-True (Test-Path -LiteralPath (Join-Path $basePack 'pack.toml') -PathType Leaf) 'starter pack manifest was not written'
    Assert-True (@(Get-ChildItem -LiteralPath $basePack -File -Filter 'market_avatar_hires_*.sprite').Count -eq 8) 'starter pack needs eight base poses'
    Assert-True (@(Get-ChildItem -LiteralPath $basePack -File -Filter 'market_avatar_stand_hires_*.sprite').Count -eq 24) 'starter pack needs twenty-four stand frames'
    Assert-True (@(Get-ChildItem -LiteralPath $basePack -File -Filter 'market_avatar_walk_hires_*.sprite').Count -eq 32) 'starter pack needs thirty-two walk frames'
    Assert-True (@(Get-ChildItem -LiteralPath $basePack -File -Filter 'market_avatar_climb_hires_*.sprite').Count -eq 16) 'starter pack needs sixteen climb frames'

    if (-not [string]::IsNullOrWhiteSpace($ValidatorExe)) {
        & $ValidatorExe validate-pack $basePack | Out-Null
        Assert-True ($LASTEXITCODE -eq 0) 'starter pack did not pass native validate-pack'

        $skinRoot = Join-Path $tempRoot 'private-assets\skins'
        $json = & $bootstrap -ProjectRoot $RepositoryRoot -OutputRoot $skinRoot -ValidatorExe $ValidatorExe -StarterOnly -AcceptThirdPartyAssetNotice
        Assert-True ($LASTEXITCODE -eq 0) 'offline bootstrap returned a non-zero exit code'
        $result = ($json -join [Environment]::NewLine) | ConvertFrom-Json
        Assert-True ([int]$result.characterCount -eq 8) 'offline bootstrap must expose eight original starter characters'
        Assert-True (Test-Path -LiteralPath ([string]$result.catalogPack) -PathType Container) 'offline bootstrap catalog pack is missing'
        & $ValidatorExe validate-pack ([string]$result.catalogPack) | Out-Null
        Assert-True ($LASTEXITCODE -eq 0) 'offline bootstrap catalog pack did not pass validate-pack'
    }

    Write-Output 'Local asset bootstrap tests passed'
} finally {
    if (Test-Path -LiteralPath $tempRoot -PathType Container) {
        $resolvedTemp = [IO.Path]::GetFullPath($tempRoot)
        $systemTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
        if (-not $resolvedTemp.StartsWith($systemTemp, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to remove a test path outside the system temp directory: $resolvedTemp"
        }
        Remove-Item -LiteralPath $resolvedTemp -Recurse -Force
    }
}
