Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:MapleSkinSchemaVersion = 1
$script:MapleSkinSlotCount = 8
$script:MapleSkinFrameNames = @(
    'stand-0', 'stand-1', 'stand-2',
    'walk-0', 'walk-1', 'walk-2', 'walk-3',
    'climb-0', 'climb-1'
)
$script:MapleSkinOptionalActionGroups = @(
    [pscustomobject]@{ Name = 'stand2'; Frames = @('stand2-0', 'stand2-1', 'stand2-2') },
    [pscustomobject]@{ Name = 'alert'; Frames = @('alert-0', 'alert-1', 'alert-2') },
    [pscustomobject]@{ Name = 'sit'; Frames = @('sit-0') }
)
$script:MapleSkinSlotAliases = @(
    '素材狐', '動作貓', '介面星', '程式熊',
    '測試鳥', '文件兔', '安全鹿', '協作楓'
)

function Get-MapleUtf8NoBom {
    return New-Object System.Text.UTF8Encoding($false)
}

function New-MapleTransientLeaf {
    param([Parameter(Mandatory = $true)][string]$Prefix)
    return $Prefix + [guid]::NewGuid().ToString('N').Substring(0, 16)
}

function Get-MapleFullPath {
    param([Parameter(Mandatory = $true)][string]$Path)
    return [System.IO.Path]::GetFullPath($Path)
}

function Assert-MapleChildPath {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$Candidate
    )
    $rootPath = (Get-MapleFullPath $Root).TrimEnd('\')
    $candidatePath = Get-MapleFullPath $Candidate
    if (-not $candidatePath.StartsWith($rootPath + '\', [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "路徑超出角色皮膚工作區：$candidatePath"
    }
    return $candidatePath
}

function Copy-MapleSettingsObject {
    param([Parameter(Mandatory = $true)]$Settings)
    return ($Settings | ConvertTo-Json -Depth 8 | ConvertFrom-Json)
}

function New-MapleInvalidSettingsException {
    param([Parameter(Mandatory = $true)][string]$Message)
    return [System.IO.InvalidDataException]::new($Message)
}

function Test-MapleSkinId {
    param([Parameter(Mandatory = $true)][string]$SkinId)
    return [bool]($SkinId -match '^builtin-[0-7]$' -or $SkinId -match '^user-[0-9a-f]{12}$')
}

function Assert-MapleSkinSettings {
    param([Parameter(Mandatory = $true)]$Settings)
    if ([int]$Settings.schemaVersion -ne $script:MapleSkinSchemaVersion) {
        throw (New-MapleInvalidSettingsException "不支援的角色皮膚設定版本：$($Settings.schemaVersion)")
    }
    $assignments = @($Settings.assignments)
    if ($assignments.Count -ne $script:MapleSkinSlotCount) {
        throw (New-MapleInvalidSettingsException '角色皮膚設定必須剛好包含 8 個槽位。')
    }
    for ($slot = 0; $slot -lt $script:MapleSkinSlotCount; $slot++) {
        $assignment = $assignments[$slot]
        if ([int]$assignment.slot -ne $slot) {
            throw (New-MapleInvalidSettingsException "角色皮膚槽位順序錯誤：預期 $slot。")
        }
        if (-not (Test-MapleSkinId ([string]$assignment.skinId))) {
            throw (New-MapleInvalidSettingsException "無效的角色皮膚 ID：$($assignment.skinId)")
        }
        if ($null -eq $assignment.locked) {
            throw (New-MapleInvalidSettingsException "角色皮膚槽位 $($slot + 1) 缺少 locked 狀態。")
        }
    }
}

function Get-ShuffledMapleBuiltins {
    param([int]$Seed)
    $random = New-Object System.Random($Seed)
    $values = New-Object 'System.Collections.Generic.List[string]'
    for ($slot = 0; $slot -lt $script:MapleSkinSlotCount; $slot++) {
        [void]$values.Add(('builtin-{0}' -f $slot))
    }
    for ($index = $values.Count - 1; $index -gt 0; $index--) {
        $swap = $random.Next($index + 1)
        $temporary = $values[$index]
        $values[$index] = $values[$swap]
        $values[$swap] = $temporary
    }
    return $values.ToArray()
}

function New-MapleSkinSettings {
    [CmdletBinding()]
    param([int]$Seed = [Environment]::TickCount)

    $order = @(Get-ShuffledMapleBuiltins -Seed $Seed)
    $assignments = @()
    for ($slot = 0; $slot -lt $script:MapleSkinSlotCount; $slot++) {
        $assignments += [pscustomobject]@{
            slot = $slot
            skinId = $order[$slot]
            locked = $false
        }
    }
    return [pscustomobject]@{
        schemaVersion = $script:MapleSkinSchemaVersion
        assignments = $assignments
        updatedUtc = [DateTime]::UtcNow.ToString('o')
    }
}

function Set-MapleRandomSkinAssignments {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]$Settings,
        [int]$Seed = [Environment]::TickCount,
        [switch]$RespectLocks
    )

    Assert-MapleSkinSettings $Settings
    $randomizedSettings = Copy-MapleSettingsObject $Settings
    if (-not $RespectLocks) {
        foreach ($assignment in @($randomizedSettings.assignments)) {
            $assignment.locked = $false
        }
    }

    $lockedBuiltins = @(
        @($randomizedSettings.assignments) |
            Where-Object { [bool]$_.locked -and [string]$_.skinId -match '^builtin-[0-7]$' } |
            ForEach-Object { [string]$_.skinId } |
            Sort-Object -Unique
    )
    $available = @(
        Get-ShuffledMapleBuiltins -Seed $Seed |
            Where-Object { $lockedBuiltins -notcontains $_ }
    )
    $cursor = 0
    foreach ($assignment in @($randomizedSettings.assignments)) {
        if ($RespectLocks -and [bool]$assignment.locked) {
            continue
        }
        if ($cursor -ge $available.Count) {
            throw '無法為未鎖定槽位建立不重複的內建造型。'
        }
        $assignment.skinId = $available[$cursor]
        $cursor++
    }
    $randomizedSettings.updatedUtc = [DateTime]::UtcNow.ToString('o')
    return $randomizedSettings
}

function Save-MapleJsonFile {
    param(
        [Parameter(Mandatory = $true)]$JsonValue,
        [Parameter(Mandatory = $true)][string]$Path,
        [int]$Depth = 8
    )
    $fullPath = Get-MapleFullPath $Path
    $parent = Split-Path -Parent $fullPath
    [void][System.IO.Directory]::CreateDirectory($parent)
    $temporary = Join-Path $parent (New-MapleTransientLeaf -Prefix '.m-')
    $backup = $fullPath + '.bak'
    $json = ($JsonValue | ConvertTo-Json -Depth $Depth) + [Environment]::NewLine
    [System.IO.File]::WriteAllText($temporary, $json, (Get-MapleUtf8NoBom))
    if (Test-Path -LiteralPath $fullPath -PathType Leaf) {
        [System.IO.File]::Replace($temporary, $fullPath, $backup, $true)
    } else {
        [System.IO.File]::Move($temporary, $fullPath)
    }
}

function Save-MapleSkinSettings {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)]$Settings,
        [Parameter(Mandatory = $true)][string]$Path
    )
    Assert-MapleSkinSettings $Settings
    $copy = Copy-MapleSettingsObject $Settings
    $copy.updatedUtc = [DateTime]::UtcNow.ToString('o')
    Save-MapleJsonFile -JsonValue $copy -Path $Path
}

function Get-MapleSkinSettings {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "找不到角色皮膚設定：$Path"
    }
    $settingsJson = [System.IO.File]::ReadAllText((Get-MapleFullPath $Path))
    try {
        $settings = $settingsJson | ConvertFrom-Json
    } catch [System.ArgumentException] {
        throw (New-MapleInvalidSettingsException "角色皮膚設定 JSON 損壞：$($_.Exception.Message)")
    }
    Assert-MapleSkinSettings $settings
    return $settings
}

function Get-MapleCompositeHash {
    param([Parameter(Mandatory = $true)][string[]]$Files)
    $lines = foreach ($file in $Files | Sort-Object) {
        $hashedFile = Get-Item -LiteralPath $file -ErrorAction Stop
        $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $hashedFile.FullName).Hash.ToLowerInvariant()
        '{0}|{1}|{2}' -f $hashedFile.Name, $hashedFile.Length, $hash
    }
    $bytes = [System.Text.Encoding]::UTF8.GetBytes(($lines -join "`n"))
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        return ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
    } finally {
        $sha.Dispose()
    }
}

function Get-MapleOpaquePalette {
    param([Parameter(Mandatory = $true)][string]$PackToml)
    $insidePalette = $false
    $entries = New-Object System.Collections.Generic.List[object]
    foreach ($line in [System.IO.File]::ReadAllLines((Get-MapleFullPath $PackToml))) {
        if ($line -match '^\s*\[palette\]\s*$') {
            $insidePalette = $true
            continue
        }
        if ($insidePalette -and $line -match '^\s*\[') {
            break
        }
        if (-not $insidePalette) {
            continue
        }
        if ($line -match '^\s*"(?<key>[^"\\]|\\["\\])"\s*=\s*"(?<value>#[0-9A-Fa-f]{6}|transparent)"') {
            $keyText = [string]$matches.key
            if ($keyText -eq '\"') { $keyText = '"' }
            if ($keyText -eq '\\') { $keyText = '\' }
            if ($keyText.Length -ne 1 -or $keyText -eq '.' -or $matches.value -eq 'transparent') {
                continue
            }
            $hex = [string]$matches.value
            $rgb = [Convert]::ToInt32($hex.Substring(1), 16)
            [void]$entries.Add([pscustomobject]@{ Key = [char]$keyText[0]; Rgb = $rgb })
        }
    }
    if ($entries.Count -lt 8) {
        throw '角色素材包的 palette 無法提供足夠的實色。'
    }
    return $entries.ToArray()
}

function Initialize-MapleSkinRasterCodec {
    if ('MapleSkinRasterCodec' -as [type]) {
        return
    }
    Add-Type -AssemblyName System.Drawing
    Add-Type -ReferencedAssemblies 'System.Drawing' -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Text;

public static class MapleSkinRasterCodec
{
    public static void EncodePng(string sourcePath, string destinationPath, char[] keys, int[] rgbs)
    {
        if (keys == null || rgbs == null || keys.Length == 0 || keys.Length != rgbs.Length)
            throw new InvalidDataException("palette is empty or inconsistent");

        using (Bitmap bitmap = new Bitmap(sourcePath))
        {
            if (bitmap.Width != 96 || bitmap.Height != 72)
                throw new InvalidDataException(Path.GetFileName(sourcePath) + " 必須是 96 x 72 PNG。");

            bool hasOpaquePixel = false;
            Dictionary<int, char> cache = new Dictionary<int, char>();
            using (StreamWriter writer = new StreamWriter(destinationPath, false, new UTF8Encoding(false)))
            {
                writer.WriteLine("# User-supplied local-only skin; fixed 96x72 paperdoll canvas.");
                writer.WriteLine("@frame 0");
                for (int y = 0; y < bitmap.Height; y++)
                {
                    for (int x = 0; x < bitmap.Width; x++)
                    {
                        if (x != 0) writer.Write(' ');
                        Color pixel = bitmap.GetPixel(x, y);
                        if (pixel.A < 128)
                        {
                            writer.Write('.');
                            continue;
                        }
                        hasOpaquePixel = true;
                        int rgb = (pixel.R << 16) | (pixel.G << 8) | pixel.B;
                        char key;
                        if (!cache.TryGetValue(rgb, out key))
                        {
                            long bestDistance = long.MaxValue;
                            int bestIndex = 0;
                            for (int index = 0; index < rgbs.Length; index++)
                            {
                                int candidate = rgbs[index];
                                long dr = pixel.R - ((candidate >> 16) & 255);
                                long dg = pixel.G - ((candidate >> 8) & 255);
                                long db = pixel.B - (candidate & 255);
                                long distance = dr * dr + dg * dg + db * db;
                                if (distance < bestDistance)
                                {
                                    bestDistance = distance;
                                    bestIndex = index;
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
            if (!hasOpaquePixel)
            {
                File.Delete(destinationPath);
                throw new InvalidDataException(Path.GetFileName(sourcePath) + " 不可為全透明圖片。");
            }
        }
    }

    public static void DecodeSprite(string sourcePath, string destinationPath, char[] keys, int[] argbs)
    {
        Dictionary<char, int> palette = new Dictionary<char, int>();
        for (int index = 0; index < keys.Length; index++) palette[keys[index]] = argbs[index];
        palette['.'] = 0;

        List<string[]> rows = new List<string[]>();
        bool inFrame = false;
        foreach (string rawLine in File.ReadAllLines(sourcePath, Encoding.UTF8))
        {
            string line = rawLine;
            int comment = line.IndexOf('#');
            if (comment >= 0) line = line.Substring(0, comment);
            line = line.Trim();
            if (line.Length == 0) continue;
            if (line.StartsWith("@frame", StringComparison.Ordinal))
            {
                if (inFrame && rows.Count > 0) break;
                inFrame = true;
                continue;
            }
            if (inFrame) rows.Add(line.Split((char[])null, StringSplitOptions.RemoveEmptyEntries));
        }
        if (rows.Count == 0) throw new InvalidDataException("sprite has no frame");
        int width = rows[0].Length;
        using (Bitmap bitmap = new Bitmap(width, rows.Count, PixelFormat.Format32bppArgb))
        {
            for (int y = 0; y < rows.Count; y++)
            {
                if (rows[y].Length != width) throw new InvalidDataException("sprite row width mismatch");
                for (int x = 0; x < width; x++)
                {
                    if (rows[y][x].Length != 1) throw new InvalidDataException("sprite token is not one character");
                    char key = rows[y][x][0];
                    int argb;
                    if (!palette.TryGetValue(key, out argb)) throw new InvalidDataException("unknown palette key");
                    bitmap.SetPixel(x, y, Color.FromArgb(argb));
                }
            }
            bitmap.Save(destinationPath, ImageFormat.Png);
        }
    }
}
'@
}

function Convert-MaplePngToSprite {
    param(
        [Parameter(Mandatory = $true)][string]$SourcePath,
        [Parameter(Mandatory = $true)][string]$DestinationPath,
        [Parameter(Mandatory = $true)][object[]]$Palette
    )
    Initialize-MapleSkinRasterCodec
    $keys = [char[]]@($Palette | ForEach-Object { [char]$_.Key })
    $rgbs = [int[]]@($Palette | ForEach-Object { [int]$_.Rgb })
    [MapleSkinRasterCodec]::EncodePng($SourcePath, $DestinationPath, $keys, $rgbs)
}

function Assert-MaplePngHeader {
    param([Parameter(Mandatory = $true)][string]$Path)
    $stream = [System.IO.File]::OpenRead((Get-MapleFullPath $Path))
    try {
        $header = New-Object byte[] 24
        if ($stream.Read($header, 0, $header.Length) -ne $header.Length) {
            throw "$(Split-Path -Leaf $Path) 不是完整的 PNG。"
        }
    } finally {
        $stream.Dispose()
    }
    $signature = @(137, 80, 78, 71, 13, 10, 26, 10)
    for ($index = 0; $index -lt $signature.Count; $index++) {
        if ($header[$index] -ne $signature[$index]) {
            throw "$(Split-Path -Leaf $Path) 不是有效的 PNG。"
        }
    }
    if ($header[12] -ne 73 -or $header[13] -ne 72 -or $header[14] -ne 68 -or $header[15] -ne 82) {
        throw "$(Split-Path -Leaf $Path) 缺少 PNG IHDR。"
    }
    $width = ([int]$header[16] -shl 24) -bor ([int]$header[17] -shl 16) -bor ([int]$header[18] -shl 8) -bor [int]$header[19]
    $height = ([int]$header[20] -shl 24) -bor ([int]$header[21] -shl 16) -bor ([int]$header[22] -shl 8) -bor [int]$header[23]
    if ($width -ne 96 -or $height -ne 72) {
        throw "$(Split-Path -Leaf $Path) 必須是 96 x 72 PNG。"
    }
}

function Import-MapleSkinFolder {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$SourceFolder,
        [Parameter(Mandatory = $true)][string]$SkinRoot,
        [Parameter(Mandatory = $true)][string]$PackToml
    )

    $source = Get-Item -LiteralPath $SourceFolder -ErrorAction Stop
    if (-not $source.PSIsContainer) {
        throw '角色皮膚匯入來源必須是資料夾。'
    }
    $required = @()
    foreach ($frameName in $script:MapleSkinFrameNames) {
        $path = Join-Path $source.FullName ($frameName + '.png')
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "缺少必要圖片：$frameName.png"
        }
        $sourcePngFile = Get-Item -LiteralPath $path
        if ($sourcePngFile.Length -gt 4MB) {
            throw "$($sourcePngFile.Name) 超過 4 MB 的單檔限制。"
        }
        Assert-MaplePngHeader -Path $sourcePngFile.FullName
        $required += $sourcePngFile.FullName
    }

    # Newer importers can provide real stand2 / alert / sit drawings.  They
    # are optional so existing nine-frame user folders retain their historic
    # fallback behaviour, but a partial action is rejected rather than mixed
    # with a different pose by accident.
    $optional = @()
    foreach ($group in @($script:MapleSkinOptionalActionGroups)) {
        $paths = @($group.Frames | ForEach-Object { Join-Path $source.FullName ($_.ToString() + '.png') })
        $present = @($paths | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf })
        if ($present.Count -eq 0) { continue }
        if ($present.Count -ne $paths.Count) { throw "選用動作 $($group.Name) 的圖片必須完整。" }
        foreach ($path in $paths) {
            $file = Get-Item -LiteralPath $path
            if ($file.Length -gt 4MB) { throw "$($file.Name) 超過 4 MB 的單檔限制。" }
            Assert-MaplePngHeader -Path $file.FullName
            $optional += $file.FullName
        }
    }

    $contentHash = Get-MapleCompositeHash -Files @($required + $optional)
    $skinId = 'user-' + $contentHash.Substring(0, 12)
    $importsRoot = Join-Path (Get-MapleFullPath $SkinRoot) 'imports'
    [void][System.IO.Directory]::CreateDirectory($importsRoot)
    $destination = Assert-MapleChildPath -Root $importsRoot -Candidate (Join-Path $importsRoot $skinId)
    if (Test-Path -LiteralPath $destination -PathType Container) {
        return [pscustomobject]@{
            id = $skinId
            displayName = $source.Name
            path = $destination
            previewPath = Join-Path $destination 'stand-0.png'
        }
    }

    $staging = Assert-MapleChildPath -Root $importsRoot -Candidate (Join-Path $importsRoot (New-MapleTransientLeaf -Prefix '.i-'))
    [void][System.IO.Directory]::CreateDirectory($staging)
    try {
        $palette = @(Get-MapleOpaquePalette -PackToml $PackToml)
        foreach ($frameName in $script:MapleSkinFrameNames) {
            $sourcePng = Join-Path $source.FullName ($frameName + '.png')
            $copiedPng = Join-Path $staging ($frameName + '.png')
            $spritePath = Join-Path $staging ($frameName + '.sprite')
            Copy-Item -LiteralPath $sourcePng -Destination $copiedPng
            Convert-MaplePngToSprite -SourcePath $copiedPng -DestinationPath $spritePath -Palette $palette
        }
        foreach ($optionalPng in @($optional)) {
            $frameName = [System.IO.Path]::GetFileNameWithoutExtension($optionalPng)
            $copiedPng = Join-Path $staging ($frameName + '.png')
            $spritePath = Join-Path $staging ($frameName + '.sprite')
            Copy-Item -LiteralPath $optionalPng -Destination $copiedPng
            Convert-MaplePngToSprite -SourcePath $copiedPng -DestinationPath $spritePath -Palette $palette
        }
        $metadata = [pscustomobject]@{
            schemaVersion = $script:MapleSkinSchemaVersion
            id = $skinId
            displayName = $source.Name
            rights = 'user-supplied-local-only'
            canvas = '96x72'
            importedUtc = [DateTime]::UtcNow.ToString('o')
        }
        Save-MapleJsonFile -JsonValue $metadata -Path (Join-Path $staging 'metadata.json')
        [System.IO.Directory]::Move($staging, $destination)
    } catch {
        if (Test-Path -LiteralPath $staging -PathType Container) {
            Remove-Item -LiteralPath $staging -Recurse -Force
        }
        throw
    }
    return [pscustomobject]@{
        id = $skinId
        displayName = $source.Name
        path = $destination
        previewPath = Join-Path $destination 'stand-0.png'
    }
}

function Get-MapleSkinCatalog {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$SkinRoot,
        [string]$BasePack
    )
    $catalog = New-Object System.Collections.Generic.List[object]
    for ($slot = 0; $slot -lt $script:MapleSkinSlotCount; $slot++) {
        $preview = $null
        if ($BasePack) {
            $preview = Join-Path (Join-Path (Get-MapleFullPath $SkinRoot) 'previews') ('builtin-{0}.png' -f $slot)
        }
        [void]$catalog.Add([pscustomobject]@{
            Id = 'builtin-{0}' -f $slot
            DisplayName = '內建紙娃娃 {0}' -f ($slot + 1)
            Type = 'builtin'
            Path = $null
            PreviewPath = $preview
        })
    }
    $importsRoot = Join-Path (Get-MapleFullPath $SkinRoot) 'imports'
    if (Test-Path -LiteralPath $importsRoot -PathType Container) {
        foreach ($directory in Get-ChildItem -LiteralPath $importsRoot -Directory | Where-Object { $_.Name -match '^user-[0-9a-f]{12}$' } | Sort-Object Name) {
            $metadataPath = Join-Path $directory.FullName 'metadata.json'
            if (-not (Test-Path -LiteralPath $metadataPath -PathType Leaf)) {
                continue
            }
            $metadataJson = [System.IO.File]::ReadAllText($metadataPath)
            try {
                $metadata = $metadataJson | ConvertFrom-Json
                if ([string]$metadata.id -ne $directory.Name) {
                    continue
                }
                [void]$catalog.Add([pscustomobject]@{
                    Id = [string]$metadata.id
                    DisplayName = '自訂｜{0}' -f [string]$metadata.displayName
                    Type = 'user'
                    Path = $directory.FullName
                    PreviewPath = Join-Path $directory.FullName 'stand-0.png'
                })
            } catch [System.ArgumentException] {
                continue
            }
        }
    }
    return $catalog.ToArray()
}

function Export-MapleBuiltinSkinPreviews {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$BasePack,
        [Parameter(Mandatory = $true)][string]$SkinRoot
    )
    $paletteEntries = @(Get-MapleOpaquePalette -PackToml (Join-Path $BasePack 'pack.toml'))
    $transparentEntry = [pscustomobject]@{ Key = [char]'.'; Argb = 0 }
    $decodeEntries = @($transparentEntry) + @($paletteEntries | ForEach-Object {
        [pscustomobject]@{ Key = [char]$_.Key; Argb = [int](0xFF000000 -bor [int]$_.Rgb) }
    })
    $keys = [char[]]@($decodeEntries | ForEach-Object { [char]$_.Key })
    $argbs = [int[]]@($decodeEntries | ForEach-Object { [int]$_.Argb })
    $previewRoot = Join-Path (Get-MapleFullPath $SkinRoot) 'previews'
    [void][System.IO.Directory]::CreateDirectory($previewRoot)
    Initialize-MapleSkinRasterCodec
    for ($slot = 0; $slot -lt $script:MapleSkinSlotCount; $slot++) {
        $source = Join-Path $BasePack ('market_avatar_hires_{0}.sprite' -f $slot)
        $destination = Join-Path $previewRoot ('builtin-{0}.png' -f $slot)
        if (-not (Test-Path -LiteralPath $destination -PathType Leaf) -or (Get-Item -LiteralPath $source).LastWriteTimeUtc -gt (Get-Item -LiteralPath $destination).LastWriteTimeUtc) {
            [MapleSkinRasterCodec]::DecodeSprite($source, $destination, $keys, $argbs)
        }
    }
    return $previewRoot
}

function Copy-MapleSkinToSlot {
    param([Parameter(Mandatory = $true)]$CopyRequest)
    $BasePack = [string]$CopyRequest.BasePack
    $SkinRoot = [string]$CopyRequest.SkinRoot
    $SkinId = [string]$CopyRequest.SkinId
    $TargetSlot = [int]$CopyRequest.TargetSlot
    $DestinationPack = [string]$CopyRequest.DestinationPack
    if ($SkinId -match '^builtin-(?<slot>[0-7])$') {
        $sourceSlot = [int]$matches.slot
        Copy-Item -LiteralPath (Join-Path $BasePack ('market_avatar_hires_{0}.sprite' -f $sourceSlot)) -Destination (Join-Path $DestinationPack ('market_avatar_hires_{0}.sprite' -f $TargetSlot)) -Force
        for ($pose = 0; $pose -lt 3; $pose++) {
            Copy-Item -LiteralPath (Join-Path $BasePack ('market_avatar_stand_hires_{0}.sprite' -f ($sourceSlot * 3 + $pose))) -Destination (Join-Path $DestinationPack ('market_avatar_stand_hires_{0}.sprite' -f ($TargetSlot * 3 + $pose))) -Force
        }
        for ($pose = 0; $pose -lt 4; $pose++) {
            Copy-Item -LiteralPath (Join-Path $BasePack ('market_avatar_walk_hires_{0}.sprite' -f ($sourceSlot * 4 + $pose))) -Destination (Join-Path $DestinationPack ('market_avatar_walk_hires_{0}.sprite' -f ($TargetSlot * 4 + $pose))) -Force
        }
        for ($pose = 0; $pose -lt 2; $pose++) {
            Copy-Item -LiteralPath (Join-Path $BasePack ('market_avatar_climb_hires_{0}.sprite' -f ($sourceSlot * 2 + $pose))) -Destination (Join-Path $DestinationPack ('market_avatar_climb_hires_{0}.sprite' -f ($TargetSlot * 2 + $pose))) -Force
        }
        for ($pose = 0; $pose -lt 3; $pose++) {
            Copy-Item -LiteralPath (Join-Path $BasePack ('market_avatar_stand2_hires_{0}.sprite' -f ($sourceSlot * 3 + $pose))) -Destination (Join-Path $DestinationPack ('market_avatar_stand2_hires_{0}.sprite' -f ($TargetSlot * 3 + $pose))) -Force
            Copy-Item -LiteralPath (Join-Path $BasePack ('market_avatar_alert_hires_{0}.sprite' -f ($sourceSlot * 3 + $pose))) -Destination (Join-Path $DestinationPack ('market_avatar_alert_hires_{0}.sprite' -f ($TargetSlot * 3 + $pose))) -Force
            $attackSource = Join-Path $BasePack ('training_avatar_attack_hires_{0}.sprite' -f ($sourceSlot * 3 + $pose))
            if (Test-Path -LiteralPath $attackSource -PathType Leaf) {
                Copy-Item -LiteralPath $attackSource -Destination (Join-Path $DestinationPack ('training_avatar_attack_hires_{0}.sprite' -f ($TargetSlot * 3 + $pose))) -Force
            }
        }
        Copy-Item -LiteralPath (Join-Path $BasePack ('market_avatar_sit_hires_{0}.sprite' -f $sourceSlot)) -Destination (Join-Path $DestinationPack ('market_avatar_sit_hires_{0}.sprite' -f $TargetSlot)) -Force
        return
    }

    if ($SkinId -notmatch '^user-[0-9a-f]{12}$') {
        throw "無效的角色皮膚 ID：$SkinId"
    }
    $importsRoot = Join-Path (Get-MapleFullPath $SkinRoot) 'imports'
    $sourceRoot = Assert-MapleChildPath -Root $importsRoot -Candidate (Join-Path $importsRoot $SkinId)
    if (-not (Test-Path -LiteralPath $sourceRoot -PathType Container)) {
        throw "找不到自訂角色皮膚：$SkinId"
    }
    Copy-Item -LiteralPath (Join-Path $sourceRoot 'stand-0.sprite') -Destination (Join-Path $DestinationPack ('market_avatar_hires_{0}.sprite' -f $TargetSlot)) -Force
    for ($pose = 0; $pose -lt 3; $pose++) {
        Copy-Item -LiteralPath (Join-Path $sourceRoot ('stand-{0}.sprite' -f $pose)) -Destination (Join-Path $DestinationPack ('market_avatar_stand_hires_{0}.sprite' -f ($TargetSlot * 3 + $pose))) -Force
    }
    for ($pose = 0; $pose -lt 4; $pose++) {
        Copy-Item -LiteralPath (Join-Path $sourceRoot ('walk-{0}.sprite' -f $pose)) -Destination (Join-Path $DestinationPack ('market_avatar_walk_hires_{0}.sprite' -f ($TargetSlot * 4 + $pose))) -Force
    }
    for ($pose = 0; $pose -lt 2; $pose++) {
        Copy-Item -LiteralPath (Join-Path $sourceRoot ('climb-{0}.sprite' -f $pose)) -Destination (Join-Path $DestinationPack ('market_avatar_climb_hires_{0}.sprite' -f ($TargetSlot * 2 + $pose))) -Force
    }
    # Nine-frame imports predate the optional status actions. Reusing that
    # skin's own stand1 frames keeps identity/scale stable when no optional
    # drawing was supplied; when all files of one action exist, use them.
    $hasStand2 = @(0..2 | Where-Object { Test-Path -LiteralPath (Join-Path $sourceRoot ('stand2-{0}.sprite' -f $_)) -PathType Leaf }).Count -eq 3
    $hasAlert = @(0..2 | Where-Object { Test-Path -LiteralPath (Join-Path $sourceRoot ('alert-{0}.sprite' -f $_)) -PathType Leaf }).Count -eq 3
    for ($pose = 0; $pose -lt 3; $pose++) {
        $fallback = Join-Path $sourceRoot ('stand-{0}.sprite' -f $pose)
        $stand2 = if ($hasStand2) { Join-Path $sourceRoot ('stand2-{0}.sprite' -f $pose) } else { $fallback }
        $alert = if ($hasAlert) { Join-Path $sourceRoot ('alert-{0}.sprite' -f $pose) } else { $fallback }
        Copy-Item -LiteralPath $stand2 -Destination (Join-Path $DestinationPack ('market_avatar_stand2_hires_{0}.sprite' -f ($TargetSlot * 3 + $pose))) -Force
        Copy-Item -LiteralPath $alert -Destination (Join-Path $DestinationPack ('market_avatar_alert_hires_{0}.sprite' -f ($TargetSlot * 3 + $pose))) -Force
    }
    $sit = Join-Path $sourceRoot 'sit-0.sprite'
    if (-not (Test-Path -LiteralPath $sit -PathType Leaf)) { $sit = Join-Path $sourceRoot 'stand-0.sprite' }
    Copy-Item -LiteralPath $sit -Destination (Join-Path $DestinationPack ('market_avatar_sit_hires_{0}.sprite' -f $TargetSlot)) -Force
}

function Disable-MapleAttackAnimation {
    param([Parameter(Mandatory = $true)][string]$DestinationPack)
    $manifestPath = Join-Path $DestinationPack 'pack.toml'
    $lines = [System.IO.File]::ReadAllLines($manifestPath)
    $output = New-Object 'System.Collections.Generic.List[string]'
    $skipping = $false
    foreach ($line in $lines) {
        if ($line.Trim() -eq '[animations.training_avatar_attack_hires]') {
            $skipping = $true
            continue
        }
        if ($skipping -and $line.TrimStart().StartsWith('[')) {
            $skipping = $false
        }
        if (-not $skipping) {
            [void]$output.Add($line)
        }
    }
    [System.IO.File]::WriteAllLines($manifestPath, $output, (Get-MapleUtf8NoBom))
    Get-ChildItem -LiteralPath $DestinationPack -File -Filter 'training_avatar_attack_hires_*.sprite' |
        Remove-Item -Force
}

function Set-MaplePackAnimationFrames {
    param(
        [Parameter(Mandatory = $true)][string]$PackToml,
        [Parameter(Mandatory = $true)][string]$AnimationName,
        [Parameter(Mandatory = $true)][string]$FilePrefix,
        [Parameter(Mandatory = $true)][int]$FrameCount
    )
    $lines = [System.IO.File]::ReadAllLines($PackToml)
    $section = '[animations.{0}]' -f $AnimationName
    $sectionIndex = [Array]::IndexOf($lines, $section)
    if ($sectionIndex -lt 0) {
        throw "角色素材包缺少動畫區段：$section"
    }
    $framesStart = -1
    for ($index = $sectionIndex + 1; $index -lt $lines.Length; $index++) {
        if ($lines[$index].TrimStart().StartsWith('[')) {
            break
        }
        if ($lines[$index].TrimStart().StartsWith('frames')) {
            $framesStart = $index
            break
        }
    }
    if ($framesStart -lt 0) {
        throw "角色素材包動畫缺少 frames：$section"
    }
    $framesEnd = $framesStart
    while ($framesEnd -lt $lines.Length -and -not $lines[$framesEnd].Contains(']')) {
        $framesEnd++
    }
    if ($framesEnd -ge $lines.Length) {
        throw "角色素材包動畫 frames 未結束：$section"
    }

    $output = New-Object 'System.Collections.Generic.List[string]'
    for ($index = 0; $index -lt $framesStart; $index++) {
        [void]$output.Add($lines[$index])
    }
    [void]$output.Add('frames = [')
    for ($index = 0; $index -lt $FrameCount; $index += 2) {
        $entries = New-Object 'System.Collections.Generic.List[string]'
        [void]$entries.Add(('"{0}_{1}.sprite"' -f $FilePrefix, $index))
        if ($index + 1 -lt $FrameCount) {
            [void]$entries.Add(('"{0}_{1}.sprite"' -f $FilePrefix, ($index + 1)))
        }
        [void]$output.Add(('  {0},' -f ($entries -join ', ')))
    }
    [void]$output.Add(']')
    for ($index = $framesEnd + 1; $index -lt $lines.Length; $index++) {
        [void]$output.Add($lines[$index])
    }
    [System.IO.File]::WriteAllLines($PackToml, $output, (Get-MapleUtf8NoBom))
}

function Get-MaplePackCharacterNames {
    param([Parameter(Mandatory = $true)][string]$PackToml)
    $lines = [System.IO.File]::ReadAllLines($PackToml)
    $sectionIndex = [Array]::IndexOf($lines, '[characters]')
    if ($sectionIndex -lt 0) {
        return @()
    }
    $names = New-Object 'System.Collections.Generic.List[string]'
    $reading = $false
    for ($index = $sectionIndex + 1; $index -lt $lines.Length; $index++) {
        $trimmed = $lines[$index].Trim()
        if ($trimmed.StartsWith('[')) {
            break
        }
        if (-not $reading) {
            if ($trimmed.StartsWith('names')) {
                $reading = $true
            }
            continue
        }
        if ($trimmed -eq ']') {
            break
        }
        $jsonString = $trimmed.TrimEnd(',')
        if ($jsonString.StartsWith('"') -and $jsonString.EndsWith('"')) {
            [void]$names.Add([string]($jsonString | ConvertFrom-Json))
        }
    }
    return $names.ToArray()
}

function Set-MaplePackCharacterNames {
    param(
        [Parameter(Mandatory = $true)][string]$PackToml,
        [Parameter(Mandatory = $true)][string[]]$Names
    )
    $lines = [System.IO.File]::ReadAllLines($PackToml)
    $sectionIndex = [Array]::IndexOf($lines, '[characters]')
    $output = New-Object 'System.Collections.Generic.List[string]'
    if ($sectionIndex -ge 0) {
        $sectionEnd = $sectionIndex + 1
        while ($sectionEnd -lt $lines.Length -and -not $lines[$sectionEnd].TrimStart().StartsWith('[')) {
            $sectionEnd++
        }
        for ($index = 0; $index -lt $sectionIndex; $index++) {
            [void]$output.Add($lines[$index])
        }
        for ($index = $sectionEnd; $index -lt $lines.Length; $index++) {
            [void]$output.Add($lines[$index])
        }
    } else {
        foreach ($line in $lines) {
            [void]$output.Add($line)
        }
    }

    $paletteIndex = $output.IndexOf('[palette]')
    if ($paletteIndex -lt 0) {
        throw '角色素材包缺少 [palette] 區段。'
    }
    $catalogLines = New-Object 'System.Collections.Generic.List[string]'
    [void]$catalogLines.Add('[characters]')
    [void]$catalogLines.Add('names = [')
    foreach ($name in $Names) {
        $encoded = $name | ConvertTo-Json -Compress
        [void]$catalogLines.Add(('  {0},' -f $encoded))
    }
    [void]$catalogLines.Add(']')
    [void]$catalogLines.Add('')
    $output.InsertRange($paletteIndex, [string[]]$catalogLines.ToArray())
    [System.IO.File]::WriteAllLines($PackToml, $output, (Get-MapleUtf8NoBom))
}

function Get-MapleCompactCatalogName {
    param([Parameter(Mandatory = $true)][string]$DisplayName)
    $name = $DisplayName -replace '^自訂｜', '' -replace '^Atelier-\d+-', ''
    if ([string]::IsNullOrWhiteSpace($name)) {
        return '自訂角色'
    }
    return $name.Trim()
}

function New-MapleCatalogSkinPack {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$BasePack,
        [Parameter(Mandatory = $true)][string]$SkinRoot,
        [Parameter(Mandatory = $true)][string]$CatalogPath,
        [Parameter(Mandatory = $true)][string]$ValidatorExe
    )
    if (-not (Test-Path -LiteralPath (Join-Path $BasePack 'pack.toml') -PathType Leaf)) {
        throw "找不到基底素材包：$BasePack"
    }
    if (-not (Test-Path -LiteralPath $CatalogPath -PathType Leaf)) {
        throw "找不到角色清單：$CatalogPath"
    }
    $catalogManifest = [System.IO.File]::ReadAllText($CatalogPath, [System.Text.Encoding]::UTF8) | ConvertFrom-Json
    $characters = @($catalogManifest.characters)
    $root = Get-MapleFullPath $SkinRoot
    $destination = Assert-MapleChildPath -Root $root -Candidate (Join-Path $root 'catalog-pack')
    $rollback = Assert-MapleChildPath -Root $root -Candidate (Join-Path $root 'catalog-rollback-pack')
    $staging = Assert-MapleChildPath -Root $root -Candidate (Join-Path $root (New-MapleTransientLeaf -Prefix '.c-'))
    try {
        Copy-Item -LiteralPath (Get-MapleFullPath $BasePack) -Destination $staging -Recurse
        $names = New-Object 'System.Collections.Generic.List[string]'
        foreach ($name in @('素材狐', '動作貓', '介面星', '程式熊', '測試鳥', '文件兔', '安全鹿', '協作楓')) {
            [void]$names.Add($name)
        }
        for ($index = 0; $index -lt $characters.Count; $index++) {
            $skinId = [string]$characters[$index].skinId
            Copy-MapleSkinToSlot -CopyRequest ([pscustomobject]@{
                BasePack = $BasePack
                SkinRoot = $root
                SkinId = $skinId
                TargetSlot = 8 + $index
                DestinationPack = $staging
            })
            [void]$names.Add((Get-MapleCompactCatalogName -DisplayName ([string]$characters[$index].title)))
        }

        $characterCount = $names.Count
        $manifestPath = Join-Path $staging 'pack.toml'
        foreach ($animation in @(
            [pscustomobject]@{ Name = 'market_avatar_hires'; Prefix = 'market_avatar_hires'; PerCharacter = 1 },
            [pscustomobject]@{ Name = 'market_avatar_stand_hires'; Prefix = 'market_avatar_stand_hires'; PerCharacter = 3 },
            [pscustomobject]@{ Name = 'market_avatar_walk_hires'; Prefix = 'market_avatar_walk_hires'; PerCharacter = 4 },
            [pscustomobject]@{ Name = 'market_avatar_climb_hires'; Prefix = 'market_avatar_climb_hires'; PerCharacter = 2 },
            [pscustomobject]@{ Name = 'market_avatar_stand2_hires'; Prefix = 'market_avatar_stand2_hires'; PerCharacter = 3 },
            [pscustomobject]@{ Name = 'market_avatar_sit_hires'; Prefix = 'market_avatar_sit_hires'; PerCharacter = 1 },
            [pscustomobject]@{ Name = 'market_avatar_alert_hires'; Prefix = 'market_avatar_alert_hires'; PerCharacter = 3 }
        )) {
            Set-MaplePackAnimationFrames -PackToml $manifestPath -AnimationName $animation.Name -FilePrefix $animation.Prefix -FrameCount ($characterCount * [int]$animation.PerCharacter)
        }
        Set-MaplePackCharacterNames -PackToml $manifestPath -Names $names.ToArray()
        Disable-MapleAttackAnimation -DestinationPack $staging

        $validation = Invoke-MaplePackValidation -ValidatorExe $ValidatorExe -PackPath $staging
        if ($validation.ExitCode -ne 0) {
            throw "$characterCount 款角色素材包驗證失敗：$($validation.Output)"
        }
        Remove-MapleGeneratedDirectory -SkinRoot $root -Path $rollback
        if (Test-Path -LiteralPath $destination -PathType Container) {
            [System.IO.Directory]::Move($destination, $rollback)
        }
        try {
            [System.IO.Directory]::Move($staging, $destination)
        } catch {
            if ((-not (Test-Path -LiteralPath $destination)) -and (Test-Path -LiteralPath $rollback -PathType Container)) {
                [System.IO.Directory]::Move($rollback, $destination)
            }
            throw
        }
    } catch {
        if (Test-Path -LiteralPath $staging -PathType Container) {
            Remove-MapleGeneratedDirectory -SkinRoot $root -Path $staging
        }
        throw
    }
    return $destination
}

function Get-MapleResolvedPackFingerprint {
    param(
        [Parameter(Mandatory = $true)][string]$BasePack,
        [Parameter(Mandatory = $true)][string]$SkinRoot,
        [Parameter(Mandatory = $true)]$Settings
    )
    $files = @((Join-Path $BasePack 'pack.toml'))
    $files += @(Get-ChildItem -LiteralPath $BasePack -File -Filter 'market_avatar*.sprite' | Select-Object -ExpandProperty FullName)
    $files += @(Get-ChildItem -LiteralPath $BasePack -File -Filter 'training_avatar_attack_hires_*.sprite' | Select-Object -ExpandProperty FullName)
    $files += @(Get-ChildItem -LiteralPath $BasePack -File -Filter 'training_skill_*.sprite' | Select-Object -ExpandProperty FullName)
    foreach ($assignment in @($Settings.assignments)) {
        if ([string]$assignment.skinId -match '^user-[0-9a-f]{12}$') {
            $userRoot = Join-Path (Join-Path $SkinRoot 'imports') ([string]$assignment.skinId)
            $files += @(Get-ChildItem -LiteralPath $userRoot -File -Filter '*.sprite' -ErrorAction Stop | Select-Object -ExpandProperty FullName)
        }
    }
    $fileFingerprint = Get-MapleCompositeHash -Files @($files | Sort-Object -Unique)
    $assignmentText = @($Settings.assignments | ForEach-Object { '{0}:{1}' -f $_.slot, $_.skinId }) -join '|'
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($fileFingerprint + '|' + $assignmentText)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        return ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
    } finally {
        $sha.Dispose()
    }
}

function Remove-MapleGeneratedDirectory {
    param(
        [Parameter(Mandatory = $true)][string]$SkinRoot,
        [Parameter(Mandatory = $true)][string]$Path
    )
    $safe = Assert-MapleChildPath -Root $SkinRoot -Candidate $Path
    if (Test-Path -LiteralPath $safe -PathType Container) {
        Remove-Item -LiteralPath $safe -Recurse -Force
    }
}

function Invoke-MaplePackValidation {
    param(
        [Parameter(Mandatory = $true)][string]$ValidatorExe,
        [Parameter(Mandatory = $true)][string]$PackPath
    )
    $startInfo = New-Object System.Diagnostics.ProcessStartInfo
    $startInfo.FileName = Get-MapleFullPath $ValidatorExe
    $startInfo.Arguments = 'validate-pack "{0}"' -f ((Get-MapleFullPath $PackPath).Replace('"', '\"'))
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $startInfo
    try {
        if (-not $process.Start()) {
            throw '無法啟動角色素材包驗證器。'
        }
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $process.WaitForExit()
        $output = ($stdout.Result + [Environment]::NewLine + $stderr.Result).Trim()
        return [pscustomobject]@{ ExitCode = $process.ExitCode; Output = $output }
    } finally {
        $process.Dispose()
    }
}

function New-MapleResolvedSkinPack {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$BasePack,
        [Parameter(Mandatory = $true)][string]$SkinRoot,
        [Parameter(Mandatory = $true)]$Settings,
        [Parameter(Mandatory = $true)][string]$ValidatorExe
    )
    Assert-MapleSkinSettings $Settings
    if (-not (Test-Path -LiteralPath (Join-Path $BasePack 'pack.toml') -PathType Leaf)) {
        throw "找不到基底素材包：$BasePack"
    }
    if (-not (Test-Path -LiteralPath $ValidatorExe -PathType Leaf)) {
        throw "找不到素材包驗證器：$ValidatorExe"
    }

    $root = Get-MapleFullPath $SkinRoot
    [void][System.IO.Directory]::CreateDirectory($root)
    $active = Assert-MapleChildPath -Root $root -Candidate (Join-Path $root 'active-pack')
    $rollback = Assert-MapleChildPath -Root $root -Candidate (Join-Path $root 'rollback-pack')
    $fingerprint = Get-MapleResolvedPackFingerprint -BasePack $BasePack -SkinRoot $root -Settings $Settings
    $activeMarker = Join-Path $active '.skin-profile.sha256'
    if ((Test-Path -LiteralPath $activeMarker -PathType Leaf) -and ([System.IO.File]::ReadAllText($activeMarker).Trim() -eq $fingerprint)) {
        return $active
    }

    $staging = Assert-MapleChildPath -Root $root -Candidate (Join-Path $root (New-MapleTransientLeaf -Prefix '.r-'))
    try {
        Copy-Item -LiteralPath (Get-MapleFullPath $BasePack) -Destination $staging -Recurse
        for ($slot = 0; $slot -lt $script:MapleSkinSlotCount; $slot++) {
            Copy-MapleSkinToSlot -CopyRequest ([pscustomobject]@{
                BasePack = $BasePack
                SkinRoot = $root
                SkinId = [string]$Settings.assignments[$slot].skinId
                TargetSlot = $slot
                DestinationPack = $staging
            })
        }
        $baseCharacterNames = @(Get-MaplePackCharacterNames -PackToml (Join-Path $staging 'pack.toml'))
        if ($baseCharacterNames.Count -ge $script:MapleSkinSlotCount) {
            $catalogById = @{}
            foreach ($entry in @(Get-MapleSkinCatalog -SkinRoot $root -BasePack $BasePack)) {
                $catalogById[[string]$entry.Id] = $entry
            }
            $resolvedNames = New-Object 'System.Collections.Generic.List[string]'
            for ($slot = 0; $slot -lt $script:MapleSkinSlotCount; $slot++) {
                $skinId = [string]$Settings.assignments[$slot].skinId
                if ($skinId -match '^builtin-(?<slot>[0-7])$') {
                    [void]$resolvedNames.Add([string]$baseCharacterNames[[int]$matches.slot])
                } elseif ($catalogById.ContainsKey($skinId)) {
                    [void]$resolvedNames.Add((Get-MapleCompactCatalogName -DisplayName ([string]$catalogById[$skinId].DisplayName)))
                } else {
                    [void]$resolvedNames.Add(('角色 {0:00}' -f ($slot + 1)))
                }
            }
            for ($slot = $script:MapleSkinSlotCount; $slot -lt $baseCharacterNames.Count; $slot++) {
                [void]$resolvedNames.Add([string]$baseCharacterNames[$slot])
            }
            Set-MaplePackCharacterNames -PackToml (Join-Path $staging 'pack.toml') -Names $resolvedNames.ToArray()
        }
        if (@($Settings.assignments | Where-Object { [string]$_.skinId -match '^user-' }).Count -gt 0) {
            # A nine-frame custom bundle has no authored attack pose. Disabling
            # the optional set makes the renderer use that same skin's mapped
            # alert/stand fallback instead of borrowing a different body.
            Disable-MapleAttackAnimation -DestinationPack $staging
        }
        [System.IO.File]::WriteAllText((Join-Path $staging '.skin-profile.sha256'), $fingerprint + [Environment]::NewLine, (Get-MapleUtf8NoBom))
        $validation = Invoke-MaplePackValidation -ValidatorExe $ValidatorExe -PackPath $staging
        if ($validation.ExitCode -ne 0) {
            throw "角色皮膚包驗證失敗：$($validation.Output)"
        }

        Remove-MapleGeneratedDirectory -SkinRoot $root -Path $rollback
        if (Test-Path -LiteralPath $active -PathType Container) {
            [System.IO.Directory]::Move($active, $rollback)
        }
        try {
            [System.IO.Directory]::Move($staging, $active)
        } catch {
            if ((-not (Test-Path -LiteralPath $active)) -and (Test-Path -LiteralPath $rollback -PathType Container)) {
                [System.IO.Directory]::Move($rollback, $active)
            }
            throw
        }
    } catch {
        if (Test-Path -LiteralPath $staging -PathType Container) {
            Remove-MapleGeneratedDirectory -SkinRoot $root -Path $staging
        }
        throw
    }
    return $active
}

function Get-MapleActiveSkinPack {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$BasePack,
        [Parameter(Mandatory = $true)][string]$SkinRoot,
        [Parameter(Mandatory = $true)][string]$SettingsPath,
        [Parameter(Mandatory = $true)][string]$ValidatorExe
    )
    if (Test-Path -LiteralPath $SettingsPath -PathType Leaf) {
        try {
            $settings = Get-MapleSkinSettings -Path $SettingsPath
        } catch [System.IO.InvalidDataException] {
            $backupPath = $SettingsPath + '.invalid-' + [DateTime]::UtcNow.ToString('yyyyMMddHHmmss')
            Copy-Item -LiteralPath $SettingsPath -Destination $backupPath
            $settings = New-MapleSkinSettings
            Save-MapleSkinSettings -Settings $settings -Path $SettingsPath
        }
    } else {
        $settings = New-MapleSkinSettings
        Save-MapleSkinSettings -Settings $settings -Path $SettingsPath
    }
    return New-MapleResolvedSkinPack -BasePack $BasePack -SkinRoot $SkinRoot -Settings $settings -ValidatorExe $ValidatorExe
}

function Show-MapleSkinWorkshop {
    [CmdletBinding()]
    param([Parameter(Mandatory = $true)]$WorkshopRequest)
    # Windows PowerShell event handlers use this modal's live local scope. Keeping
    # control construction and callbacks together avoids module-global UI state.
    $BasePack = [string]$WorkshopRequest.BasePack
    $SkinRoot = [string]$WorkshopRequest.SkinRoot
    $SettingsPath = [string]$WorkshopRequest.SettingsPath
    $ValidatorExe = [string]$WorkshopRequest.ValidatorExe
    $PreviewPath = [string]$WorkshopRequest.PreviewPath

    Add-Type -AssemblyName System.Windows.Forms
    Add-Type -AssemblyName System.Drawing
    [System.Windows.Forms.Application]::EnableVisualStyles()

    [void](Export-MapleBuiltinSkinPreviews -BasePack $BasePack -SkinRoot $SkinRoot)
    $loadWarning = $null
    if (Test-Path -LiteralPath $SettingsPath -PathType Leaf) {
        try {
            $settings = Get-MapleSkinSettings -Path $SettingsPath
        } catch [System.IO.InvalidDataException] {
            $loadWarning = $_.Exception.Message
            $settings = New-MapleSkinSettings
        }
    } else {
        $settings = New-MapleSkinSettings
    }
    $state = [pscustomobject]@{
        Settings = $settings
        Catalog = @(Get-MapleSkinCatalog -SkinRoot $SkinRoot -BasePack $BasePack)
        Applied = $false
    }

    $form = New-Object System.Windows.Forms.Form
    $form.Text = 'Maple Agent Market｜角色造型工坊'
    $form.ClientSize = New-Object System.Drawing.Size(780, 720)
    $form.StartPosition = 'CenterScreen'
    $form.FormBorderStyle = 'FixedDialog'
    $form.MaximizeBox = $false
    $form.MinimizeBox = $false
    $form.AutoScaleMode = [System.Windows.Forms.AutoScaleMode]::Dpi
    $form.BackColor = [System.Drawing.Color]::FromArgb(35, 43, 33)
    $form.ForeColor = [System.Drawing.Color]::FromArgb(255, 246, 211)
    $form.Font = New-Object System.Drawing.Font('Microsoft JhengHei UI', 10)

    $title = New-Object System.Windows.Forms.Label
    $title.Text = '角色造型工坊'
    $title.Font = New-Object System.Drawing.Font('Microsoft JhengHei UI', 19, [System.Drawing.FontStyle]::Bold)
    $title.AutoSize = $true
    $title.Location = New-Object System.Drawing.Point(24, 17)
    $form.Controls.Add($title)

    $subtitle = New-Object System.Windows.Forms.Label
    $subtitle.Text = "八個固定槽位依 Agent 首次進場順序分配。鎖定的槽位不會被『重抽未鎖定』改掉。"
    $subtitle.AutoSize = $true
    $subtitle.ForeColor = [System.Drawing.Color]::FromArgb(159, 196, 91)
    $subtitle.Location = New-Object System.Drawing.Point(27, 58)
    $form.Controls.Add($subtitle)

    $reroll = New-Object System.Windows.Forms.Button
    $reroll.Text = '重抽未鎖定'
    $reroll.Size = New-Object System.Drawing.Size(140, 36)
    $reroll.Location = New-Object System.Drawing.Point(27, 94)
    $reroll.BackColor = [System.Drawing.Color]::FromArgb(79, 151, 82)
    $reroll.ForeColor = [System.Drawing.Color]::White
    $reroll.FlatStyle = 'Flat'
    $form.Controls.Add($reroll)

    $randomAll = New-Object System.Windows.Forms.Button
    $randomAll.Text = '全部隨機'
    $randomAll.Size = New-Object System.Drawing.Size(130, 36)
    $randomAll.Location = New-Object System.Drawing.Point(175, 94)
    $randomAll.BackColor = [System.Drawing.Color]::FromArgb(48, 91, 57)
    $randomAll.ForeColor = [System.Drawing.Color]::White
    $randomAll.FlatStyle = 'Flat'
    $form.Controls.Add($randomAll)

    $restore = New-Object System.Windows.Forms.Button
    $restore.Text = '恢復原始順序'
    $restore.Size = New-Object System.Drawing.Size(150, 36)
    $restore.Location = New-Object System.Drawing.Point(313, 94)
    $restore.BackColor = [System.Drawing.Color]::FromArgb(48, 53, 73)
    $restore.ForeColor = [System.Drawing.Color]::White
    $restore.FlatStyle = 'Flat'
    $form.Controls.Add($restore)

    $import = New-Object System.Windows.Forms.Button
    $import.Text = '匯入 9 張 PNG…'
    $import.Size = New-Object System.Drawing.Size(210, 36)
    $import.Location = New-Object System.Drawing.Point(543, 94)
    $import.BackColor = [System.Drawing.Color]::FromArgb(113, 78, 45)
    $import.ForeColor = [System.Drawing.Color]::White
    $import.FlatStyle = 'Flat'
    $form.Controls.Add($import)

    $panel = New-Object System.Windows.Forms.Panel
    $panel.Location = New-Object System.Drawing.Point(24, 145)
    $panel.Size = New-Object System.Drawing.Size(732, 455)
    $panel.AutoScroll = $true
    $panel.BackColor = [System.Drawing.Color]::FromArgb(27, 34, 27)
    $panel.BorderStyle = 'FixedSingle'
    $form.Controls.Add($panel)

    $comboBoxes = @()
    $lockBoxes = @()
    $pictureBoxes = @()

    function Set-WorkshopPreview {
        param(
            [Parameter(Mandatory = $true)][System.Windows.Forms.ComboBox]$Combo,
            [Parameter(Mandatory = $true)][System.Windows.Forms.PictureBox]$Picture
        )
        if ($null -ne $Picture.Image) {
            $old = $Picture.Image
            $Picture.Image = $null
            $old.Dispose()
        }
        $selected = $Combo.SelectedItem
        if ($null -eq $selected -or -not $selected.PreviewPath -or -not (Test-Path -LiteralPath ([string]$selected.PreviewPath) -PathType Leaf)) {
            return
        }
        $sourceImage = [System.Drawing.Image]::FromFile([string]$selected.PreviewPath)
        try {
            $Picture.Image = $sourceImage.Clone()
        } finally {
            $sourceImage.Dispose()
        }
    }

    function Set-WorkshopComboSelection {
        param(
            [Parameter(Mandatory = $true)][System.Windows.Forms.ComboBox]$Combo,
            [Parameter(Mandatory = $true)][string]$SkinId
        )
        for ($index = 0; $index -lt $Combo.Items.Count; $index++) {
            if ([string]$Combo.Items[$index].Id -eq $SkinId) {
                $Combo.SelectedIndex = $index
                return
            }
        }
        $Combo.SelectedIndex = 0
    }

    for ($slot = 0; $slot -lt $script:MapleSkinSlotCount; $slot++) {
        $rowY = 8 + ($slot * 56)
        $picture = New-Object System.Windows.Forms.PictureBox
        $picture.Location = New-Object System.Drawing.Point(9, ($rowY + 3))
        $picture.Size = New-Object System.Drawing.Size(58, 46)
        $picture.SizeMode = [System.Windows.Forms.PictureBoxSizeMode]::Zoom
        $picture.BackColor = [System.Drawing.Color]::FromArgb(18, 23, 18)
        $panel.Controls.Add($picture)

        $slotLabel = New-Object System.Windows.Forms.Label
        $slotLabel.Text = '槽位 {0}｜{1}' -f ($slot + 1), $script:MapleSkinSlotAliases[$slot]
        $slotLabel.AutoSize = $false
        $slotLabel.Size = New-Object System.Drawing.Size(132, 24)
        $slotLabel.Location = New-Object System.Drawing.Point(78, ($rowY + 14))
        $panel.Controls.Add($slotLabel)

        $combo = New-Object System.Windows.Forms.ComboBox
        $combo.DropDownStyle = 'DropDownList'
        $combo.DisplayMember = 'DisplayName'
        $combo.Size = New-Object System.Drawing.Size(330, 30)
        $combo.Location = New-Object System.Drawing.Point(214, ($rowY + 9))
        foreach ($skin in $state.Catalog) {
            [void]$combo.Items.Add($skin)
        }
        $combo.Tag = $picture
        $combo.Add_SelectedIndexChanged({
            Set-WorkshopPreview -Combo $this -Picture ([System.Windows.Forms.PictureBox]$this.Tag)
        })
        $panel.Controls.Add($combo)

        $lock = New-Object System.Windows.Forms.CheckBox
        $lock.Text = '鎖定'
        $lock.AutoSize = $true
        $lock.Location = New-Object System.Drawing.Point(565, ($rowY + 14))
        $lock.Checked = [bool]$state.Settings.assignments[$slot].locked
        $panel.Controls.Add($lock)

        $comboBoxes += $combo
        $lockBoxes += $lock
        $pictureBoxes += $picture
        Set-WorkshopComboSelection -Combo $combo -SkinId ([string]$state.Settings.assignments[$slot].skinId)
    }

    function Sync-WorkshopSettingsFromUi {
        $assignments = @()
        for ($slot = 0; $slot -lt $script:MapleSkinSlotCount; $slot++) {
            $selected = $comboBoxes[$slot].SelectedItem
            if ($null -eq $selected) {
                throw "槽位 $($slot + 1) 尚未選擇造型。"
            }
            $assignments += [pscustomobject]@{
                slot = $slot
                skinId = [string]$selected.Id
                locked = [bool]$lockBoxes[$slot].Checked
            }
        }
        $state.Settings = [pscustomobject]@{
            schemaVersion = $script:MapleSkinSchemaVersion
            assignments = $assignments
            updatedUtc = [DateTime]::UtcNow.ToString('o')
        }
    }

    function Apply-WorkshopSettingsToUi {
        for ($slot = 0; $slot -lt $script:MapleSkinSlotCount; $slot++) {
            Set-WorkshopComboSelection -Combo $comboBoxes[$slot] -SkinId ([string]$state.Settings.assignments[$slot].skinId)
            $lockBoxes[$slot].Checked = [bool]$state.Settings.assignments[$slot].locked
        }
    }

    function Refresh-WorkshopCatalog {
        $selectedIds = @($comboBoxes | ForEach-Object {
            if ($null -ne $_.SelectedItem) { [string]$_.SelectedItem.Id } else { 'builtin-0' }
        })
        $state.Catalog = @(Get-MapleSkinCatalog -SkinRoot $SkinRoot -BasePack $BasePack)
        for ($slot = 0; $slot -lt $script:MapleSkinSlotCount; $slot++) {
            $comboBoxes[$slot].Items.Clear()
            foreach ($skin in $state.Catalog) {
                [void]$comboBoxes[$slot].Items.Add($skin)
            }
            Set-WorkshopComboSelection -Combo $comboBoxes[$slot] -SkinId $selectedIds[$slot]
        }
    }

    $reroll.Add_Click({
        try {
            Sync-WorkshopSettingsFromUi
            $state.Settings = Set-MapleRandomSkinAssignments -Settings $state.Settings -RespectLocks
            Apply-WorkshopSettingsToUi
        } catch {
            [System.Windows.Forms.MessageBox]::Show($_.Exception.Message, '重抽失敗', 'OK', 'Error') | Out-Null
        }
    })

    $randomAll.Add_Click({
        try {
            Sync-WorkshopSettingsFromUi
            $state.Settings = Set-MapleRandomSkinAssignments -Settings $state.Settings
            Apply-WorkshopSettingsToUi
        } catch {
            [System.Windows.Forms.MessageBox]::Show($_.Exception.Message, '隨機失敗', 'OK', 'Error') | Out-Null
        }
    })

    $restore.Add_Click({
        $assignments = @()
        for ($slot = 0; $slot -lt $script:MapleSkinSlotCount; $slot++) {
            $assignments += [pscustomobject]@{ slot = $slot; skinId = 'builtin-{0}' -f $slot; locked = $false }
        }
        $state.Settings = [pscustomobject]@{
            schemaVersion = $script:MapleSkinSchemaVersion
            assignments = $assignments
            updatedUtc = [DateTime]::UtcNow.ToString('o')
        }
        Apply-WorkshopSettingsToUi
    })

    $import.Add_Click({
        $dialog = New-Object System.Windows.Forms.FolderBrowserDialog
        $dialog.Description = "選擇含 9 張 96 x 72 透明 PNG 的資料夾：stand-0..2、walk-0..3、climb-0..1"
        $dialog.ShowNewFolderButton = $false
        try {
            if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
                $imported = Import-MapleSkinFolder -SourceFolder $dialog.SelectedPath -SkinRoot $SkinRoot -PackToml (Join-Path $BasePack 'pack.toml')
                Refresh-WorkshopCatalog
                [System.Windows.Forms.MessageBox]::Show(
                    "已匯入：$($imported.displayName)`r`n現在可從任一槽位的下拉選單選擇它。",
                    '角色皮膚已匯入',
                    'OK',
                    'Information'
                ) | Out-Null
            }
        } catch {
            [System.Windows.Forms.MessageBox]::Show($_.Exception.Message, '角色皮膚匯入失敗', 'OK', 'Error') | Out-Null
        } finally {
            $dialog.Dispose()
        }
    })

    $notice = New-Object System.Windows.Forms.Label
    $notice.Text = "自訂皮膚只存於 private-assets，不納入 Git。套用後會驗證完整 3 張站立、4 張走路、2 張爬梯；`r`n已開啟的浮動視窗不會被熱改，下一次啟動才會換造型。"
    $notice.AutoSize = $false
    $notice.Size = New-Object System.Drawing.Size(730, 48)
    $notice.Location = New-Object System.Drawing.Point(27, 610)
    $notice.ForeColor = [System.Drawing.Color]::FromArgb(181, 186, 204)
    $form.Controls.Add($notice)

    $cancel = New-Object System.Windows.Forms.Button
    $cancel.Text = '取消'
    $cancel.Size = New-Object System.Drawing.Size(98, 36)
    $cancel.Location = New-Object System.Drawing.Point(542, 668)
    $cancel.BackColor = [System.Drawing.Color]::FromArgb(48, 53, 73)
    $cancel.ForeColor = [System.Drawing.Color]::White
    $cancel.FlatStyle = 'Flat'
    $cancel.Add_Click({ $form.Close() })
    $form.Controls.Add($cancel)

    $apply = New-Object System.Windows.Forms.Button
    $apply.Text = '儲存並套用'
    $apply.Size = New-Object System.Drawing.Size(113, 36)
    $apply.Location = New-Object System.Drawing.Point(646, 668)
    $apply.BackColor = [System.Drawing.Color]::FromArgb(79, 151, 82)
    $apply.ForeColor = [System.Drawing.Color]::White
    $apply.FlatStyle = 'Flat'
    $apply.Add_Click({
        try {
            Sync-WorkshopSettingsFromUi
            $form.UseWaitCursor = $true
            $apply.Enabled = $false
            $apply.Text = '驗證中…'
            $form.Refresh()
            Save-MapleSkinSettings -Settings $state.Settings -Path $SettingsPath
            [void](New-MapleResolvedSkinPack -BasePack $BasePack -SkinRoot $SkinRoot -Settings $state.Settings -ValidatorExe $ValidatorExe)
            $state.Applied = $true
            $form.Close()
        } catch {
            $form.UseWaitCursor = $false
            $apply.Enabled = $true
            $apply.Text = '儲存並套用'
            [System.Windows.Forms.MessageBox]::Show($_.Exception.Message, '套用角色皮膚失敗', 'OK', 'Error') | Out-Null
        }
    })
    $form.Controls.Add($apply)
    $form.AcceptButton = $apply
    $form.CancelButton = $cancel

    $form.Add_Shown({
        if ($loadWarning) {
            [System.Windows.Forms.MessageBox]::Show(
                "$loadWarning`r`n目前先載入一組安全的隨機設定；按『儲存並套用』後才會覆寫。",
                '角色皮膚設定已回復',
                'OK',
                'Warning'
            ) | Out-Null
        }
    })
    $form.Add_FormClosed({
        foreach ($picture in $pictureBoxes) {
            if ($null -ne $picture.Image) {
                $picture.Image.Dispose()
                $picture.Image = $null
            }
        }
    })

    if ($PreviewPath) {
        $previewFullPath = Get-MapleFullPath $PreviewPath
        [void][System.IO.Directory]::CreateDirectory((Split-Path -Parent $previewFullPath))
        $form.Opacity = 0
        $form.ShowInTaskbar = $false
        $form.Show()
        [System.Windows.Forms.Application]::DoEvents()
        $previewBitmap = New-Object System.Drawing.Bitmap($form.Width, $form.Height)
        try {
            $form.DrawToBitmap($previewBitmap, (New-Object System.Drawing.Rectangle(0, 0, $form.Width, $form.Height)))
            $previewBitmap.Save($previewFullPath, [System.Drawing.Imaging.ImageFormat]::Png)
        } finally {
            $previewBitmap.Dispose()
            $form.Hide()
        }
        foreach ($picture in $pictureBoxes) {
            if ($null -ne $picture.Image) {
                $picture.Image.Dispose()
                $picture.Image = $null
            }
        }
        $form.Dispose()
        return $false
    }

    [void]$form.ShowDialog()
    $applied = [bool]$state.Applied
    $form.Dispose()
    return $applied
}

Export-ModuleMember -Function @(
    'New-MapleSkinSettings',
    'Set-MapleRandomSkinAssignments',
    'Save-MapleSkinSettings',
    'Get-MapleSkinSettings',
    'Import-MapleSkinFolder',
    'Get-MapleSkinCatalog',
    'Export-MapleBuiltinSkinPreviews',
    'New-MapleCatalogSkinPack',
    'New-MapleResolvedSkinPack',
    'Get-MapleActiveSkinPack',
    'Show-MapleSkinWorkshop'
)
