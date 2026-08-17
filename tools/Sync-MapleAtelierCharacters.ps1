#requires -Version 5.1

[CmdletBinding()]
param(
    # Existing eight plus ten visually distinct public-gallery outfits. Every
    # entry is still normalized through stand1/walk1/ladder before import.
    [int[]]$OutfitIds = @(3, 5, 6, 9, 13, 14, 15, 22, 23, 32, 33, 35, 38, 57, 81, 82, 83, 85),
    [string]$SkinRoot = (Join-Path (Split-Path -Parent $PSScriptRoot) 'private-assets\skins'),
    [string]$PackToml = (Join-Path (Split-Path -Parent $PSScriptRoot) 'private-assets\skins\active-pack\pack.toml'),
    [string]$BasePack = (Join-Path (Split-Path -Parent $PSScriptRoot) 'sprites'),
    [string]$ValidatorExe = (Join-Path (Split-Path -Parent $PSScriptRoot) 'bin\maple-agent-market.exe'),
    [string]$WorkshopModule = (Join-Path $PSScriptRoot 'MapleSkinWorkshop.psm1'),
    [string]$WorkRoot = (Join-Path ([System.IO.Path]::GetTempPath()) 'MapleAgentMarket-Atelier'),
    [switch]$KeepWork
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:GalleryUri = 'https://maple-atelier.org/api/outfits/public?sort=popular&limit=100'
$script:AtelierRoot = 'https://maple-atelier.org'
$script:MapleIoRoot = 'https://maplestory.io'
$script:DefaultRegion = 'TWMS'
$script:DefaultVersion = '256'

function Get-Utf8Json {
    param([Parameter(Mandatory = $true)][string]$Uri)

    $client = New-Object System.Net.WebClient
    try {
        $client.Headers['User-Agent'] = 'Maple-Agent-Market/local-character-sync'
        $bytes = $client.DownloadData($Uri)
        $text = [System.Text.Encoding]::UTF8.GetString($bytes)
        return $text | ConvertFrom-Json
    } finally {
        $client.Dispose()
    }
}

function Save-RemoteFile {
    param(
        [Parameter(Mandatory = $true)][string]$Uri,
        [Parameter(Mandatory = $true)][string]$Path
    )

    $client = New-Object System.Net.WebClient
    try {
        $client.Headers['User-Agent'] = 'Maple-Agent-Market/local-character-sync'
        $client.DownloadFile($Uri, $Path)
    } finally {
        $client.Dispose()
    }
}

function Get-OptionalText {
    param($Value, [Parameter(Mandatory = $true)][string]$Fallback)
    if ($null -eq $Value -or [string]::IsNullOrWhiteSpace([string]$Value)) {
        return $Fallback
    }
    return [string]$Value
}

function Get-ObjectProperty {
    param(
        [Parameter(Mandatory = $true)]$Object,
        [Parameter(Mandatory = $true)][string]$Name
    )
    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property) {
        return $null
    }
    return $property.Value
}

function Get-CharacterRenderUri {
    param(
        [Parameter(Mandatory = $true)]$Outfit,
        [Parameter(Mandatory = $true)][string]$Stance
    )

    $slots = $Outfit.payload.slots
    $entries = New-Object 'System.Collections.Generic.List[object]'
    $skin = Get-ObjectProperty -Object $slots -Name 'skin'
    if ($null -ne $skin) {
        $skinRegion = Get-OptionalText -Value (Get-ObjectProperty -Object $skin -Name 'region') -Fallback $script:DefaultRegion
        $skinVersion = Get-OptionalText -Value (Get-ObjectProperty -Object $skin -Name 'version') -Fallback $script:DefaultVersion
        [void]$entries.Add([ordered]@{
            itemId = [int]$skin.id
            region = $skinRegion
            version = $skinVersion
        })
        [void]$entries.Add([ordered]@{
            itemId = [int]$skin.id + 10000
            region = $skinRegion
            version = $skinVersion
        })
    }

    foreach ($property in $slots.PSObject.Properties) {
        if ($property.Name -in @('skin', 'ear')) {
            continue
        }
        $item = $property.Value
        if ($null -eq $item) {
            continue
        }
        $entry = [ordered]@{
            itemId = [int]$item.id
            region = Get-OptionalText -Value (Get-ObjectProperty -Object $item -Name 'region') -Fallback $script:DefaultRegion
            version = Get-OptionalText -Value (Get-ObjectProperty -Object $item -Name 'version') -Fallback $script:DefaultVersion
        }
        if ($property.Name -eq 'face') {
            $entry.animationName = Get-OptionalText -Value (Get-ObjectProperty -Object $Outfit.payload -Name 'expression') -Fallback 'default'
        }
        [void]$entries.Add($entry)
    }

    $json = $entries.ToArray() | ConvertTo-Json -Compress -Depth 8
    if ($json.Length -lt 2 -or $json[0] -ne '[' -or $json[$json.Length - 1] -ne ']') {
        throw "Outfit $($Outfit.id) cannot be converted to character render parameters."
    }
    $payload = [Uri]::EscapeDataString($json.Substring(1, $json.Length - 2))
    $ear = Get-ObjectProperty -Object $slots -Name 'ear'
    $earId = if ($null -ne $ear) { [int]$ear.id } else { 90000 }
    $showEars = ($earId -eq 90001).ToString().ToLowerInvariant()
    $showLefEars = ($earId -eq 90002).ToString().ToLowerInvariant()
    $showHighLefEars = ($earId -eq 90003).ToString().ToLowerInvariant()
    return ('{0}/api/character/{1}/{2}/animated?showears={3}&showLefEars={4}&showHighLefEars={5}&resize=1&flipX=false&renderMode=1&padX=30&padY=50' -f $script:MapleIoRoot, $payload, $Stance, $showEars, $showLefEars, $showHighLefEars)
}

function Export-NormalizedAnimationFrames {
    param(
        [Parameter(Mandatory = $true)][string]$GifPath,
        [Parameter(Mandatory = $true)][string[]]$FrameNames,
        [Parameter(Mandatory = $true)][string]$Destination
    )

    Add-Type -AssemblyName System.Drawing
    $image = [System.Drawing.Image]::FromFile($GifPath)
    try {
        if ($image.Width -ne 96 -or $image.Height -ne 96) {
            throw "$(Split-Path -Leaf $GifPath) must be a 96 x 96 maplestory.io animation."
        }
        $dimension = New-Object System.Drawing.Imaging.FrameDimension($image.FrameDimensionsList[0])
        $frameCount = $image.GetFrameCount($dimension)
        if ($frameCount -lt 1) {
            throw "$(Split-Path -Leaf $GifPath) does not contain animation frames."
        }

        for ($index = 0; $index -lt $FrameNames.Count; $index++) {
            [void]$image.SelectActiveFrame($dimension, ($index % $frameCount))
            $canvas = New-Object System.Drawing.Bitmap 96, 72, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
            try {
                $graphics = [System.Drawing.Graphics]::FromImage($canvas)
                try {
                    $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
                    $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighSpeed
                    $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
                    $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
                    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::None
                    $graphics.Clear([System.Drawing.Color]::Transparent)
                    $destinationRect = New-Object System.Drawing.Rectangle 12, 0, 72, 72
                    $sourceRect = New-Object System.Drawing.Rectangle 0, 0, 96, 96
                    $graphics.DrawImage($image, $destinationRect, $sourceRect, [System.Drawing.GraphicsUnit]::Pixel)
                } finally {
                    $graphics.Dispose()
                }
                $outputPath = Join-Path $Destination ($FrameNames[$index] + '.png')
                $canvas.Save($outputPath, [System.Drawing.Imaging.ImageFormat]::Png)
            } finally {
                $canvas.Dispose()
            }
        }
    } finally {
        $image.Dispose()
    }
}

function Set-ImportedMetadata {
    param(
        [Parameter(Mandatory = $true)]$Imported,
        [Parameter(Mandatory = $true)]$Outfit
    )

    $metadataPath = Join-Path $Imported.path 'metadata.json'
    $metadata = ([System.IO.File]::ReadAllText($metadataPath, [System.Text.Encoding]::UTF8) | ConvertFrom-Json)
    $metadata.rights = 'nexon-derived-local-evaluation-only'
    $metadata | Add-Member -NotePropertyName sourceType -NotePropertyValue 'maple-atelier-public-outfit' -Force
    $metadata | Add-Member -NotePropertyName sourceOutfitId -NotePropertyValue ([int]$Outfit.id) -Force
    $metadata | Add-Member -NotePropertyName sourceTitle -NotePropertyValue ([string]$Outfit.title) -Force
    $metadata | Add-Member -NotePropertyName sourceAuthor -NotePropertyValue ([string]$Outfit.authorName) -Force
    $metadata | Add-Member -NotePropertyName sourceUrl -NotePropertyValue ("$($script:AtelierRoot)/outfit/$($Outfit.id)") -Force
    $metadata | Add-Member -NotePropertyName assetProvider -NotePropertyValue $script:MapleIoRoot -Force
    $json = ($metadata | ConvertTo-Json -Depth 8) + [Environment]::NewLine
    [System.IO.File]::WriteAllText($metadataPath, $json, (New-Object System.Text.UTF8Encoding($false)))
}

if ($OutfitIds.Count -eq 0) {
    throw 'At least one Maple Atelier outfit ID is required.'
}
if (-not (Test-Path -LiteralPath $WorkshopModule -PathType Leaf)) {
    throw "Maple skin workshop module not found: $WorkshopModule"
}
if (-not (Test-Path -LiteralPath $PackToml -PathType Leaf)) {
    throw "Character pack manifest not found: $PackToml"
}

Import-Module -Name $WorkshopModule -Force
$gallery = Get-Utf8Json -Uri $script:GalleryUri
$rows = @($gallery.rows)
$selected = foreach ($id in @($OutfitIds | Sort-Object -Unique)) {
    $match = @($rows | Where-Object { [int]$_.id -eq $id })
    if ($match.Count -ne 1) {
        throw "Outfit ID $id is not present in the Maple Atelier public gallery."
    }
    $match[0]
}

[void][System.IO.Directory]::CreateDirectory([System.IO.Path]::GetFullPath($SkinRoot))
[void][System.IO.Directory]::CreateDirectory($WorkRoot)
$runRoot = Join-Path ([System.IO.Path]::GetFullPath($WorkRoot)) ('run-' + [guid]::NewGuid().ToString('N'))
[void][System.IO.Directory]::CreateDirectory($runRoot)
$results = New-Object 'System.Collections.Generic.List[object]'
try {
    foreach ($outfit in $selected) {
        $safeTitle = ([string]$outfit.title -replace '[\\/:*?"<>|]', '_').Trim()
        if ([string]::IsNullOrWhiteSpace($safeTitle)) {
            $safeTitle = 'Untitled outfit'
        }
        $sourceFolder = Join-Path $runRoot ('Atelier-{0:d3}-{1}' -f [int]$outfit.id, $safeTitle)
        [void][System.IO.Directory]::CreateDirectory($sourceFolder)

        $animations = @(
            [pscustomobject]@{ Stance = 'stand1'; Frames = @('stand-0', 'stand-1', 'stand-2') },
            [pscustomobject]@{ Stance = 'walk1'; Frames = @('walk-0', 'walk-1', 'walk-2', 'walk-3') },
            [pscustomobject]@{ Stance = 'ladder'; Frames = @('climb-0', 'climb-1') }
        )
        foreach ($animation in $animations) {
            $gifPath = Join-Path $sourceFolder ($animation.Stance + '.gif')
            $renderUri = Get-CharacterRenderUri -Outfit $outfit -Stance $animation.Stance
            Save-RemoteFile -Uri $renderUri -Path $gifPath
            Export-NormalizedAnimationFrames -GifPath $gifPath -FrameNames $animation.Frames -Destination $sourceFolder
            Remove-Item -LiteralPath $gifPath -Force
        }

        $imported = Import-MapleSkinFolder -SourceFolder $sourceFolder -SkinRoot $SkinRoot -PackToml $PackToml
        Set-ImportedMetadata -Imported $imported -Outfit $outfit
        [void]$results.Add([pscustomobject]@{
            outfitId = [int]$outfit.id
            title = [string]$outfit.title
            author = [string]$outfit.authorName
            skinId = [string]$imported.id
            previewPath = [string]$imported.previewPath
            sourceUrl = "$($script:AtelierRoot)/outfit/$($outfit.id)"
        })
        Write-Host ('Added: {0} ({1})' -f [string]$outfit.title, [string]$imported.id)
    }

    $manifest = [pscustomobject]@{
        schemaVersion = 1
        generatedUtc = [DateTime]::UtcNow.ToString('o')
        notice = 'NEXON-derived images for local evaluation only; never include in Git or public releases.'
        gallery = $script:GalleryUri
        assetProvider = $script:MapleIoRoot
        characters = $results.ToArray()
    }
    $manifestPath = Join-Path ([System.IO.Path]::GetFullPath($SkinRoot)) 'maple-atelier-catalog.json'
    $manifestJson = ($manifest | ConvertTo-Json -Depth 8) + [Environment]::NewLine
    [System.IO.File]::WriteAllText($manifestPath, $manifestJson, (New-Object System.Text.UTF8Encoding($false)))
    $catalogPack = New-MapleCatalogSkinPack -BasePack $BasePack -SkinRoot $SkinRoot -CatalogPath $manifestPath -ValidatorExe $ValidatorExe
    Write-Host ('Completed: {0} local characters; manifest: {1}' -f $results.Count, $manifestPath)
    Write-Host ('Catalog pack: {0}' -f $catalogPack)
    $results.ToArray()
} finally {
    if (-not $KeepWork -and (Test-Path -LiteralPath $runRoot -PathType Container)) {
        $resolvedWork = [System.IO.Path]::GetFullPath($WorkRoot).TrimEnd('\')
        $resolvedRun = [System.IO.Path]::GetFullPath($runRoot)
        if (-not $resolvedRun.StartsWith($resolvedWork + '\', [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to remove a path outside the work root: $resolvedRun"
        }
        Remove-Item -LiteralPath $resolvedRun -Recurse -Force
    }
}
