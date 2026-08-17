#requires -Version 5.1

<#
.SYNOPSIS
Creates the original, redistributable eight-character starter sprite pack.

.DESCRIPTION
The generated `.sprite` files are drawn from simple geometry by this script.
No MapleStory image, screenshot, client file, API response, or third-party
raster asset is embedded in the repository or copied into the result.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$OutputPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-Utf8NoBom { New-Object System.Text.UTF8Encoding($false) }

function Set-Pixel {
    param([char[][]]$Canvas, [int]$X, [int]$Y, [char]$Key)
    if ($Y -ge 0 -and $Y -lt $Canvas.Count -and $X -ge 0 -and $X -lt $Canvas[$Y].Count) {
        $Canvas[$Y][$X] = $Key
    }
}

function Fill-Rectangle {
    param([char[][]]$Canvas, [int]$Left, [int]$Top, [int]$Width, [int]$Height, [char]$Key)
    for ($y = $Top; $y -lt ($Top + $Height); $y++) {
        for ($x = $Left; $x -lt ($Left + $Width); $x++) { Set-Pixel $Canvas $x $y $Key }
    }
}

function Fill-Ellipse {
    param([char[][]]$Canvas, [int]$CenterX, [int]$CenterY, [int]$RadiusX, [int]$RadiusY, [char]$Key)
    for ($y = $CenterY - $RadiusY; $y -le $CenterY + $RadiusY; $y++) {
        for ($x = $CenterX - $RadiusX; $x -le $CenterX + $RadiusX; $x++) {
            $dx = ($x - $CenterX) / [double][Math]::Max(1, $RadiusX)
            $dy = ($y - $CenterY) / [double][Math]::Max(1, $RadiusY)
            if (($dx * $dx + $dy * $dy) -le 1.0) { Set-Pixel $Canvas $x $y $Key }
        }
    }
}

function New-AvatarSpriteText {
    param(
        [int]$Slot,
        [ValidateSet('stand', 'walk', 'climb', 'stand2', 'sit', 'alert')][string]$Pose,
        [int]$Frame
    )

    [char[][]]$canvas = @()
    for ($y = 0; $y -lt 72; $y++) { $canvas += ,([char[]]('.' * 96)) }

    $bob = if ($Pose -eq 'walk') { @(0, -1, 0, 1)[$Frame % 4] } elseif ($Pose -in @('stand', 'stand2')) { @(0, -1, 0)[$Frame % 3] } else { 0 }
    $center = 48
    $accent = [char]([int][char]'0' + $Slot)
    $outline = [char]'n'

    # Distinct, original silhouettes: cap, ears, ribbon, hood, crown, antenna,
    # side buns and maple-like three-point ornament. These are geometric marks,
    # not traced game assets.
    Fill-Ellipse $canvas $center (29 + $bob) 11 12 ([char]'S')
    Fill-Rectangle $canvas 38 (20 + $bob) 21 5 ([char]'H')
    switch ($Slot) {
        0 { Fill-Rectangle $canvas 35 (17 + $bob) 26 4 $accent; Fill-Rectangle $canvas 55 (14 + $bob) 4 5 $accent }
        1 { Fill-Ellipse $canvas 38 (17 + $bob) 4 7 $accent; Fill-Ellipse $canvas 58 (17 + $bob) 4 7 $accent }
        2 { Fill-Rectangle $canvas 43 (13 + $bob) 10 7 $accent; Set-Pixel $canvas 40 (14 + $bob) $accent; Set-Pixel $canvas 56 (14 + $bob) $accent }
        3 { Fill-Ellipse $canvas $center (19 + $bob) 15 7 $accent }
        4 { Fill-Rectangle $canvas 39 (15 + $bob) 4 7 $accent; Fill-Rectangle $canvas 46 (12 + $bob) 4 10 $accent; Fill-Rectangle $canvas 54 (15 + $bob) 4 7 $accent }
        5 { Fill-Rectangle $canvas 47 (10 + $bob) 2 9 $outline; Fill-Ellipse $canvas 48 (9 + $bob) 3 3 $accent }
        6 { Fill-Ellipse $canvas 37 (23 + $bob) 6 7 $accent; Fill-Ellipse $canvas 59 (23 + $bob) 6 7 $accent }
        7 { Fill-Rectangle $canvas 46 (11 + $bob) 5 10 $accent; Fill-Rectangle $canvas 40 (15 + $bob) 17 4 $accent }
    }

    # Face and torso.
    Set-Pixel $canvas 44 (30 + $bob) ([char]'e')
    Set-Pixel $canvas 52 (30 + $bob) ([char]'e')
    Fill-Rectangle $canvas 46 (35 + $bob) 5 2 ([char]'m')
    Fill-Rectangle $canvas 39 (39 + $bob) 19 16 ([char]'B')
    Fill-Rectangle $canvas 42 (55 + $bob) 6 10 ([char]'P')
    Fill-Rectangle $canvas 51 (55 + $bob) 6 10 ([char]'P')

    # Pose-specific limb motion; each family has genuine frame variation.
    if ($Pose -eq 'walk') {
        $swing = @(-4, -1, 3, 0)[$Frame % 4]
        Fill-Rectangle $canvas (34 + $swing) (41 + $bob) 5 14 ([char]'B')
        Fill-Rectangle $canvas (59 - $swing) (41 + $bob) 5 14 ([char]'B')
        Fill-Rectangle $canvas (40 - $swing) (63 + $bob) 7 3 ([char]'n')
        Fill-Rectangle $canvas (52 + $swing) (63 + $bob) 7 3 ([char]'n')
    } elseif ($Pose -eq 'climb') {
        $reach = if (($Frame % 2) -eq 0) { -5 } else { 5 }
        Fill-Rectangle $canvas (35 + $reach) (36 + $bob) 5 18 ([char]'B')
        Fill-Rectangle $canvas (59 - $reach) (36 + $bob) 5 18 ([char]'B')
    } elseif ($Pose -eq 'sit') {
        Fill-Rectangle $canvas 34 (46 + $bob) 8 7 ([char]'B')
        Fill-Rectangle $canvas 57 (46 + $bob) 8 7 ([char]'B')
        Fill-Rectangle $canvas 40 (59 + $bob) 18 6 ([char]'P')
    } elseif ($Pose -eq 'alert') {
        Fill-Rectangle $canvas 32 (33 + $bob) 6 18 ([char]'B')
        Fill-Rectangle $canvas 61 (33 + $bob) 6 18 ([char]'B')
        Fill-Rectangle $canvas 46 (34 + $bob) 5 3 ([char]'e')
    } else {
        $arm = if ($Pose -eq 'stand2') { @(-2, 0, 2)[$Frame % 3] } else { 0 }
        Fill-Rectangle $canvas (34 + $arm) (42 + $bob) 5 13 ([char]'B')
        Fill-Rectangle $canvas (60 - $arm) (42 + $bob) 5 13 ([char]'B')
    }

    $lines = New-Object 'System.Collections.Generic.List[string]'
    [void]$lines.Add('# Original programmatic Maple Agent Market starter avatar; no third-party raster source.')
    [void]$lines.Add('@frame 0')
    foreach ($row in $canvas) { [void]$lines.Add(($row -join ' ')) }
    return ($lines -join [Environment]::NewLine) + [Environment]::NewLine
}

function Add-AnimationToml {
    param(
        [System.Collections.Generic.List[string]]$Lines,
        [string]$Name,
        [string[]]$Frames,
        [int]$FrameMs
    )
    [void]$Lines.Add("[animations.$Name]")
    [void]$Lines.Add('frames = [')
    foreach ($frame in $Frames) { [void]$Lines.Add(('  "{0}",' -f $frame)) }
    [void]$Lines.Add(']')
    [void]$Lines.Add(('frame_ms = {0}' -f $FrameMs))
    [void]$Lines.Add('')
}

$target = [IO.Path]::GetFullPath($OutputPath)
if (Test-Path -LiteralPath $target) { throw "Refusing to overwrite an existing starter pack: $target" }
$parent = Split-Path -Parent $target
[void][IO.Directory]::CreateDirectory($parent)
$staging = Join-Path $parent ('.starter-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($staging)

try {
    $animationFiles = @{}
    foreach ($definition in @(
        [pscustomobject]@{ Name = 'market_avatar_hires'; Pose = 'stand'; PerSlot = 1; FrameMs = 500 },
        [pscustomobject]@{ Name = 'market_avatar_stand_hires'; Pose = 'stand'; PerSlot = 3; FrameMs = 420 },
        [pscustomobject]@{ Name = 'market_avatar_walk_hires'; Pose = 'walk'; PerSlot = 4; FrameMs = 130 },
        [pscustomobject]@{ Name = 'market_avatar_climb_hires'; Pose = 'climb'; PerSlot = 2; FrameMs = 180 },
        [pscustomobject]@{ Name = 'market_avatar_stand2_hires'; Pose = 'stand2'; PerSlot = 3; FrameMs = 360 },
        [pscustomobject]@{ Name = 'market_avatar_sit_hires'; Pose = 'sit'; PerSlot = 1; FrameMs = 560 },
        [pscustomobject]@{ Name = 'market_avatar_alert_hires'; Pose = 'alert'; PerSlot = 3; FrameMs = 230 }
    )) {
        $files = New-Object 'System.Collections.Generic.List[string]'
        for ($slot = 0; $slot -lt 8; $slot++) {
            for ($frame = 0; $frame -lt [int]$definition.PerSlot; $frame++) {
                $flat = $slot * [int]$definition.PerSlot + $frame
                $file = '{0}_{1}.sprite' -f $definition.Name, $flat
                $text = New-AvatarSpriteText -Slot $slot -Pose $definition.Pose -Frame $frame
                [IO.File]::WriteAllText((Join-Path $staging $file), $text, (Get-Utf8NoBom))
                [void]$files.Add($file)
            }
        }
        $animationFiles[$definition.Name] = [pscustomobject]@{ Files = $files.ToArray(); FrameMs = [int]$definition.FrameMs }
    }

    $toml = New-Object 'System.Collections.Generic.List[string]'
    [void]$toml.Add('[pack]')
    [void]$toml.Add('name = "Maple Agent Market Original Starter"')
    [void]$toml.Add('version = "1"')
    [void]$toml.Add('')
    [void]$toml.Add('[characters]')
    [void]$toml.Add('names = ["素材狐", "動作貓", "介面星", "程式熊", "測試鳥", "文件兔", "安全鹿", "協作楓"]')
    [void]$toml.Add('')
    [void]$toml.Add('[palette]')
    [void]$toml.Add('"." = "transparent"')
    foreach ($entry in @(
        @('H', '#4a2a1e'), @('S', '#f4c79a'), @('e', '#241713'), @('m', '#a44a4a'),
        @('B', '#3f7d45'), @('P', '#314a6e'), @('n', '#101820'),
        @('0', '#e76f51'), @('1', '#f4a261'), @('2', '#e9c46a'), @('3', '#2a9d8f'),
        @('4', '#457b9d'), @('5', '#6d597a'), @('6', '#b56576'), @('7', '#84a59d')
    )) { [void]$toml.Add(('"{0}" = "{1}"' -f $entry[0], $entry[1])) }

    # A deterministic 6x6x6 RGB cube gives imported local paperdolls a useful
    # quantization palette while keeping every key/RGB pair unique.
    $reserved = @{}
    foreach ($line in $toml) {
        if ($line -match '#(?<rgb>[0-9a-fA-F]{6})') { $reserved[$matches.rgb.ToLowerInvariant()] = $true }
    }
    $keyCode = 0xE000
    foreach ($red in @(0, 51, 102, 153, 204, 255)) {
        foreach ($green in @(0, 51, 102, 153, 204, 255)) {
            foreach ($blue in @(0, 51, 102, 153, 204, 255)) {
                $hex = '{0:x2}{1:x2}{2:x2}' -f $red, $green, $blue
                if ($reserved.ContainsKey($hex)) { continue }
                $key = [char]$keyCode
                $keyCode++
                [void]$toml.Add(('"{0}" = "#{1}"' -f $key, $hex))
            }
        }
    }
    [void]$toml.Add('')
    [void]$toml.Add('[animations]')
    [void]$toml.Add('')
    foreach ($name in @('market_avatar_hires', 'market_avatar_stand_hires', 'market_avatar_walk_hires', 'market_avatar_climb_hires', 'market_avatar_stand2_hires', 'market_avatar_sit_hires', 'market_avatar_alert_hires')) {
        $entry = $animationFiles[$name]
        Add-AnimationToml -Lines $toml -Name $name -Frames $entry.Files -FrameMs $entry.FrameMs
    }
    [IO.File]::WriteAllLines((Join-Path $staging 'pack.toml'), $toml.ToArray(), (Get-Utf8NoBom))
    [IO.File]::WriteAllText((Join-Path $staging 'GENERATED_BY.txt'), "Generated by tools/New-MapleStarterPack.ps1`nOriginal project artwork; MIT licensed with the repository code.`n", (Get-Utf8NoBom))
    [IO.Directory]::Move($staging, $target)
    Write-Output $target
} catch {
    if (Test-Path -LiteralPath $staging -PathType Container) { Remove-Item -LiteralPath $staging -Recurse -Force }
    throw
}
