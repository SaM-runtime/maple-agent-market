#requires -Version 5.1

[CmdletBinding()]
param(
    [string]$InstallRoot,
    [string]$BasePack,
    [string]$AssetRoot,
    [string]$ValidatorExe,
    [string]$WorkshopModule,
    [string]$CharacterCatalog
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scriptDirectory = [IO.Path]::GetDirectoryName($MyInvocation.MyCommand.Path)
$defaultInstallRoot = Split-Path -Parent $scriptDirectory
if ([string]::IsNullOrWhiteSpace($InstallRoot)) { $InstallRoot = $defaultInstallRoot }
if ([string]::IsNullOrWhiteSpace($BasePack)) { $BasePack = Join-Path $InstallRoot 'sprites' }
if ([string]::IsNullOrWhiteSpace($AssetRoot)) { $AssetRoot = Join-Path $InstallRoot 'private-assets\skills' }
if ([string]::IsNullOrWhiteSpace($ValidatorExe)) { $ValidatorExe = Join-Path $InstallRoot 'bin\maple-agent-market.exe' }
if ([string]::IsNullOrWhiteSpace($WorkshopModule)) { $WorkshopModule = Join-Path $scriptDirectory 'MapleSkinWorkshop.psm1' }
if ([string]::IsNullOrWhiteSpace($CharacterCatalog)) { $CharacterCatalog = Join-Path $InstallRoot 'private-assets\skins\maple-atelier-catalog.json' }

$script:MapleIoRoot = 'https://maplestory.io'
$script:Region = 'GMS'
$script:Version = 62
$script:ManagedSourceFolder = 'gms62-bb-pre-full-v2'
$script:Skills = @(
    [pscustomobject]@{
        Id = 2321008
        Name = 'Genesis'
        TraditionalName = '天怒'
        Animation = 'training_skill_holy_light'
        FrameBook = 'hit'
        CanvasWidth = 256
        CanvasHeight = 256
        AnchorX = 128
        AnchorY = 224
    },
    [pscustomobject]@{
        Id = 1311006
        Name = 'Dragon Roar'
        TraditionalName = '龍咆嘯'
        Animation = 'training_skill_dragon_pulse'
        FrameBook = 'effect'
        CanvasWidth = 288
        CanvasHeight = 288
        AnchorX = 144
        AnchorY = 216
    }
)

function Get-Utf8Json {
    param([Parameter(Mandatory = $true)][string]$Uri)
    $client = New-Object System.Net.WebClient
    try {
        $client.Headers['User-Agent'] = 'Maple-Agent-Market/local-classic-skill-sync'
        $bytes = $client.DownloadData($Uri)
        return ([Text.Encoding]::UTF8.GetString($bytes) | ConvertFrom-Json)
    } finally {
        $client.Dispose()
    }
}

function Get-OpaquePalette {
    param([Parameter(Mandatory = $true)][string]$PackToml)
    $inside = $false
    $entries = New-Object 'System.Collections.Generic.List[object]'
    foreach ($line in [IO.File]::ReadAllLines($PackToml, [Text.Encoding]::UTF8)) {
        if ($line -match '^\s*\[palette\]\s*$') {
            $inside = $true
            continue
        }
        if ($inside -and $line -match '^\s*\[') { break }
        if (-not $inside) { continue }
        if ($line -match '^\s*"(?<key>[^"\\]|\\["\\])"\s*=\s*"(?<value>#[0-9A-Fa-f]{6}|transparent)"') {
            $keyText = [string]$matches.key
            if ($keyText -eq '\"') { $keyText = '"' }
            if ($keyText -eq '\\') { $keyText = '\' }
            if ($keyText.Length -ne 1 -or $keyText -eq '.' -or $matches.value -eq 'transparent') {
                continue
            }
            [void]$entries.Add([pscustomobject]@{
                Key = [char]$keyText[0]
                Rgb = [Convert]::ToInt32(([string]$matches.value).Substring(1), 16)
            })
        }
    }
    if ($entries.Count -lt 8) {
        throw '素材包 palette 沒有足夠的實色可量化技能影格。'
    }
    return $entries.ToArray()
}

function Initialize-ClassicSkillCodec {
    if ('MapleClassicSkillCodec' -as [type]) { return }
    Add-Type -AssemblyName System.Drawing
    Add-Type -ReferencedAssemblies 'System.Drawing' -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Text;

public static class MapleClassicSkillCodec
{
    public static void Encode(string sourcePath, string destinationPath, char[] keys, int[] rgbs)
    {
        if (keys == null || rgbs == null || keys.Length < 8 || keys.Length != rgbs.Length)
            throw new InvalidDataException("palette is empty or inconsistent");

        using (Bitmap bitmap = new Bitmap(sourcePath))
        {
            if (bitmap.Width < 1 || bitmap.Height < 1 || bitmap.Width > 320 || bitmap.Height > 320)
                throw new InvalidDataException(Path.GetFileName(sourcePath) + " has an unsupported canvas");

            Dictionary<int, char> cache = new Dictionary<int, char>();
            using (StreamWriter writer = new StreamWriter(destinationPath, false, new UTF8Encoding(false)))
            {
                writer.WriteLine("# GMS v62 skill frame; local-only NEXON derivative; do not redistribute.");
                writer.WriteLine("@frame 0");
                for (int y = 0; y < bitmap.Height; y++)
                {
                    for (int x = 0; x < bitmap.Width; x++)
                    {
                        if (x != 0) writer.Write(' ');
                        Color pixel = bitmap.GetPixel(x, y);
                        if (pixel.A < 48)
                        {
                            writer.Write('.');
                            continue;
                        }
                        int rgb = (pixel.R << 16) | (pixel.G << 8) | pixel.B;
                        char key;
                        if (!cache.TryGetValue(rgb, out key))
                        {
                            long best = long.MaxValue;
                            int bestIndex = 0;
                            for (int i = 0; i < rgbs.Length; i++)
                            {
                                int candidate = rgbs[i];
                                long dr = pixel.R - ((candidate >> 16) & 255);
                                long dg = pixel.G - ((candidate >> 8) & 255);
                                long db = pixel.B - (candidate & 255);
                                long distance = dr * dr + dg * dg + db * db;
                                if (distance < best)
                                {
                                    best = distance;
                                    bestIndex = i;
                                    if (distance == 0) break;
                                }
                            }
                            key = keys[bestIndex];
                            cache[rgb] = key;
                        }
                        writer.Write(key);
                    }
                    writer.WriteLine();
                }
            }
            // A transparent lead/tail frame is intentional in classic skill
            // timelines. Keeping it preserves the source delay and prevents
            // the following visible frames from starting too early.
        }
    }
}
'@
}

function Export-NormalizedFrame {
    param(
        [Parameter(Mandatory = $true)]$Frame,
        [Parameter(Mandatory = $true)]$Skill,
        [Parameter(Mandatory = $true)][string]$RawPath,
        [Parameter(Mandatory = $true)][string]$NormalizedPath
    )
    Add-Type -AssemblyName System.Drawing
    $bytes = [Convert]::FromBase64String([string]$Frame.image)
    [IO.File]::WriteAllBytes($RawPath, $bytes)
    $stream = New-Object IO.MemoryStream(,$bytes)
    $image = [Drawing.Image]::FromStream($stream)
    try {
        $canvas = New-Object Drawing.Bitmap ([int]$Skill.CanvasWidth),([int]$Skill.CanvasHeight),([Drawing.Imaging.PixelFormat]::Format32bppArgb)
        try {
            $graphics = [Drawing.Graphics]::FromImage($canvas)
            try {
                $graphics.CompositingMode = [Drawing.Drawing2D.CompositingMode]::SourceCopy
                $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
                $graphics.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::Half
                $graphics.SmoothingMode = [Drawing.Drawing2D.SmoothingMode]::None
                $graphics.Clear([Drawing.Color]::Transparent)
                $x = [int]$Skill.AnchorX - [int]$Frame.origin.x
                $y = [int]$Skill.AnchorY - [int]$Frame.origin.y
                $graphics.DrawImageUnscaled($image, $x, $y)
            } finally {
                $graphics.Dispose()
            }
            # Preserve the original game canvas. The old v1 tool reduced the
            # entire 256/288 px effect to 96 px and the renderer then enlarged
            # it again, discarding most ring, rune and glow detail.
            $canvas.Save($NormalizedPath, [Drawing.Imaging.ImageFormat]::Png)
        } finally {
            $canvas.Dispose()
        }
    } finally {
        $image.Dispose()
        $stream.Dispose()
    }
}

function Get-GreatestCommonDivisor {
    param(
        [Parameter(Mandatory = $true)][int]$Left,
        [Parameter(Mandatory = $true)][int]$Right
    )
    $a = [Math]::Abs($Left)
    $b = [Math]::Abs($Right)
    while ($b -ne 0) {
        $remainder = $a % $b
        $a = $b
        $b = $remainder
    }
    return $a
}

function Get-CompleteFrameTimeline {
    param([Parameter(Mandatory = $true)][object[]]$Frames)
    if ($Frames.Count -lt 1) { throw '技能動畫沒有任何影格。' }
    $frameMs = 0
    foreach ($frame in $Frames) {
        $delay = [int]$frame.delay
        if ($delay -lt 1) { throw "技能影格含無效延遲：$delay ms" }
        $frameMs = if ($frameMs -eq 0) { $delay } else { Get-GreatestCommonDivisor -Left $frameMs -Right $delay }
    }
    if ($frameMs -lt 20) {
        throw "技能影格的共同播放間隔過小：$frameMs ms"
    }
    $indices = New-Object 'System.Collections.Generic.List[int]'
    for ($sourceIndex = 0; $sourceIndex -lt $Frames.Count; $sourceIndex++) {
        $repeats = [int]$Frames[$sourceIndex].delay / $frameMs
        for ($repeat = 0; $repeat -lt $repeats; $repeat++) {
            [void]$indices.Add($sourceIndex)
        }
    }
    return [pscustomobject]@{
        FrameMs = $frameMs
        SourceDurationMs = [int](($Frames | Measure-Object -Property delay -Sum).Sum)
        SourceIndices = $indices.ToArray()
    }
}

function Set-PackAnimation {
    param(
        [Parameter(Mandatory = $true)][string]$PackToml,
        [Parameter(Mandatory = $true)][string]$Animation,
        [Parameter(Mandatory = $true)][string[]]$Frames,
        [Parameter(Mandatory = $true)][int]$FrameMs
    )
    $text = [IO.File]::ReadAllText($PackToml, [Text.Encoding]::UTF8)
    $newline = if ($text.Contains("`r`n")) { "`r`n" } else { "`n" }
    $frameLines = ($Frames | ForEach-Object { '  "' + $_ + '",' }) -join $newline
    $section = '[animations.' + $Animation + ']' + $newline + 'frames = [' + $newline + $frameLines + $newline + ']' + $newline + 'frame_ms = ' + $FrameMs
    $pattern = '(?ms)^\[animations\.' + [regex]::Escape($Animation) + '\]\r?\n.*?(?=^\[|\z)'
    if ([regex]::IsMatch($text, $pattern)) {
        $text = [regex]::Replace($text, $pattern, $section + $newline + $newline)
    } else {
        $text = $text.TrimEnd() + $newline + $newline + $section + $newline
    }
    [IO.File]::WriteAllText($PackToml, $text, (New-Object Text.UTF8Encoding($false)))
}

function Invoke-PackValidation {
    param([string]$PackPath)
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $ValidatorExe
    $info.Arguments = 'validate-pack "' + $PackPath.Replace('"', '\"') + '"'
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $info
    try {
        if (-not $process.Start()) { throw '無法啟動素材包驗證器。' }
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $process.WaitForExit()
        $output = ($stdout.Result + [Environment]::NewLine + $stderr.Result).Trim()
        if ($process.ExitCode -ne 0) { throw "技能素材包驗證失敗：$output" }
        return $output
    } finally {
        $process.Dispose()
    }
}

function Update-CharacterPacksWithClassicSkills {
    Import-Module -Name $WorkshopModule -Force
    $skinRoot = Join-Path $InstallRoot 'private-assets\skins'
    $settingsPath = Join-Path $skinRoot 'skin-settings.json'
    $catalogPack = New-MapleCatalogSkinPack -BasePack $BasePack -SkinRoot $skinRoot -CatalogPath $CharacterCatalog -ValidatorExe $ValidatorExe
    $activePack = Get-MapleActiveSkinPack -BasePack $catalogPack -SkinRoot $skinRoot -SettingsPath $settingsPath -ValidatorExe $ValidatorExe
    return [pscustomobject]@{ CatalogPack = $catalogPack; ActivePack = $activePack }
}

foreach ($path in @($BasePack, $ValidatorExe, $WorkshopModule, $CharacterCatalog)) {
    if (-not (Test-Path -LiteralPath $path)) { throw "缺少必要路徑：$path" }
}
$assetRootFull = [IO.Path]::GetFullPath($AssetRoot).TrimEnd('\')
[void][IO.Directory]::CreateDirectory($assetRootFull)
$finalSources = Join-Path $assetRootFull $script:ManagedSourceFolder
$manifestPath = Join-Path $assetRootFull 'classic-skill-catalog.json'
if (Test-Path -LiteralPath $finalSources) {
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        throw "技能來源資料夾已存在但缺少 manifest；拒絕猜測或覆寫：$finalSources"
    }
    $existingManifest = [IO.File]::ReadAllText($manifestPath, [Text.Encoding]::UTF8) | ConvertFrom-Json
    $existingIds = @($existingManifest.skills | ForEach-Object { [int]$_.id })
    $packText = [IO.File]::ReadAllText((Join-Path $BasePack 'pack.toml'), [Text.Encoding]::UTF8)
    foreach ($skill in $script:Skills) {
        $sourceFolder = Join-Path $finalSources ([string]$skill.Id + '-' + $skill.Name.Replace(' ','-').ToLowerInvariant())
        $hasAnimation = $packText -match ('(?m)^\[animations\.' + [regex]::Escape([string]$skill.Animation) + '\]\s*$')
        $hasFrames = @(Get-ChildItem -LiteralPath $BasePack -File -Filter ($skill.Animation + '_*.sprite')).Count -gt 0
        if ($existingIds -notcontains [int]$skill.Id -or -not (Test-Path -LiteralPath $sourceFolder -PathType Container) -or -not $hasAnimation -or -not $hasFrames) {
            throw "既有技能快取不完整；拒絕部分重用或覆寫：$finalSources"
        }
    }
    $validation = Invoke-PackValidation -PackPath $BasePack
    $packs = Update-CharacterPacksWithClassicSkills
    Write-Host "Reused verified classic skill cache: $finalSources"
    Write-Host "Validated: $validation"
    Write-Host "Character catalog rebuilt: $($packs.CatalogPack)"
    Write-Host "Active pack rebuilt: $($packs.ActivePack)"
    Write-Host "Manifest: $manifestPath"
    return
}
$staging = Join-Path $assetRootFull ('.staging-' + [guid]::NewGuid().ToString('N'))
$stagingPack = Join-Path $staging 'pack'
$stagingSources = Join-Path $staging 'sources'
[void][IO.Directory]::CreateDirectory($staging)
try {
    Copy-Item -LiteralPath $BasePack -Destination $stagingPack -Recurse
    [void][IO.Directory]::CreateDirectory($stagingSources)
    $palette = @(Get-OpaquePalette -PackToml (Join-Path $stagingPack 'pack.toml'))
    Initialize-ClassicSkillCodec
    $keys = [char[]]@($palette | ForEach-Object { [char]$_.Key })
    $rgbs = [int[]]@($palette | ForEach-Object { [int]$_.Rgb })
    $manifestSkills = New-Object 'System.Collections.Generic.List[object]'

    foreach ($skill in $script:Skills) {
        $metadataUrl = '{0}/api/{1}/{2}/job/skill/{3}' -f $script:MapleIoRoot,$script:Region,$script:Version,$skill.Id
        $metadata = Get-Utf8Json -Uri $metadataUrl
        if ([int]$metadata.id -ne [int]$skill.Id) { throw "技能 ID 回應不符：$($skill.Id)" }
        $book = @($metadata.($skill.FrameBook))
        if ($book.Count -lt 1) { throw "$($skill.Name) 找不到 $($skill.FrameBook) 動畫。" }
        if ($book.Count -ne 1) { throw "$($skill.Name) 的 $($skill.FrameBook) 含 $($book.Count) 層；拒絕只輸出第一層。" }
        $frames = @($book[0].frames)
        $timeline = Get-CompleteFrameTimeline -Frames $frames
        $skillSource = Join-Path $stagingSources ([string]$skill.Id + '-' + $skill.Name.Replace(' ','-').ToLowerInvariant())
        [void][IO.Directory]::CreateDirectory($skillSource)
        $generated = New-Object 'System.Collections.Generic.List[object]'
        $spriteNames = New-Object 'System.Collections.Generic.List[string]'
        for ($outputIndex = 0; $outputIndex -lt $timeline.SourceIndices.Count; $outputIndex++) {
            $sourceIndex = [int]$timeline.SourceIndices[$outputIndex]
            if ($sourceIndex -lt 0 -or $sourceIndex -ge $frames.Count) {
                throw "$($skill.Name) frame index $sourceIndex 超出 $($frames.Count) 張。"
            }
            $rawPath = Join-Path $skillSource ('raw-{0}.png' -f $sourceIndex)
            $normalizedPath = Join-Path $skillSource ('normalized-{0}.png' -f $outputIndex)
            Export-NormalizedFrame -Frame $frames[$sourceIndex] -Skill $skill -RawPath $rawPath -NormalizedPath $normalizedPath
            $spriteName = '{0}_{1}.sprite' -f $skill.Animation,$outputIndex
            $spritePath = Join-Path $stagingPack $spriteName
            [MapleClassicSkillCodec]::Encode($normalizedPath, $spritePath, $keys, $rgbs)
            [void]$spriteNames.Add($spriteName)
            [void]$generated.Add([pscustomobject]@{
                outputFrame = $outputIndex
                sourceFrame = $sourceIndex
                sourceDelayMs = [int]$frames[$sourceIndex].delay
                sourceOrigin = [pscustomobject]@{ x = [int]$frames[$sourceIndex].origin.x; y = [int]$frames[$sourceIndex].origin.y }
                rawSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $rawPath).Hash.ToLowerInvariant()
                normalizedSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $normalizedPath).Hash.ToLowerInvariant()
                spriteSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $spritePath).Hash.ToLowerInvariant()
            })
        }
        Set-PackAnimation -PackToml (Join-Path $stagingPack 'pack.toml') -Animation $skill.Animation -Frames $spriteNames.ToArray() -FrameMs $timeline.FrameMs
        [void]$manifestSkills.Add([pscustomobject]@{
            id = [int]$skill.Id
            name = [string]$skill.Name
            traditionalName = [string]$skill.TraditionalName
            metadataUrl = $metadataUrl
            frameBook = [string]$skill.FrameBook
            targetAnimation = [string]$skill.Animation
            gameVersion = 'GMS v62 (pre-Big Bang)'
            sourceFrameCount = $frames.Count
            playbackFrameCount = $timeline.SourceIndices.Count
            frameMs = $timeline.FrameMs
            sourceDurationMs = $timeline.SourceDurationMs
            generatedFrames = $generated.ToArray()
        })
        Write-Host ('Prepared: {0} / {1}' -f $skill.Name,$skill.TraditionalName)
    }

    $validation = Invoke-PackValidation -PackPath $stagingPack
    [IO.Directory]::Move($stagingSources, $finalSources)
    foreach ($skill in $script:Skills) {
        Get-ChildItem -LiteralPath $stagingPack -File -Filter ($skill.Animation + '_*.sprite') | ForEach-Object {
            Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $BasePack $_.Name) -Force
        }
    }
    Copy-Item -LiteralPath (Join-Path $stagingPack 'pack.toml') -Destination (Join-Path $BasePack 'pack.toml') -Force

    $manifest = [pscustomobject]@{
        schemaVersion = 2
        retrievedUtc = [DateTime]::UtcNow.ToString('o')
        scope = 'local-personal-evaluation-only'
        redistribution = 'not-authorized; never commit, publish, upload, or bundle these NEXON-derived files'
        sourceService = $script:MapleIoRoot
        sourceDisclaimer = 'maplestory.io is unofficial; all returned media remains NEXON property'
        noLocalClientUnpacking = $true
        region = $script:Region
        version = $script:Version
        skills = $manifestSkills.ToArray()
    }
    [IO.File]::WriteAllText($manifestPath, (($manifest | ConvertTo-Json -Depth 12) + [Environment]::NewLine), (New-Object Text.UTF8Encoding($false)))

    $packs = Update-CharacterPacksWithClassicSkills
    Write-Host "Validated: $validation"
    Write-Host "Character catalog rebuilt: $($packs.CatalogPack)"
    Write-Host "Active pack rebuilt: $($packs.ActivePack)"
    Write-Host "Manifest: $manifestPath"
} finally {
    if (Test-Path -LiteralPath $staging -PathType Container) {
        $resolvedStaging = [IO.Path]::GetFullPath($staging)
        if (-not $resolvedStaging.StartsWith($assetRootFull + '\', [StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to remove staging outside skill root: $resolvedStaging"
        }
        Remove-Item -LiteralPath $resolvedStaging -Recurse -Force
    }
}
