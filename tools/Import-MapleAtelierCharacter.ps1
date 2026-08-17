#requires -Version 5.1

<#
.SYNOPSIS
Imports one Maple Atelier outfit or maplestory.io character-render URL into
the local-only Maple Agent Market skin catalog.

.DESCRIPTION
Accepted input is deliberately narrow: a public Maple Atelier outfit URL,
its /simulator?load= URL, or an HTTPS maplestory.io character-render URL
copied from Maple Atelier. The MIT-licensed helper may be distributed, but it
never uploads assets and its generated NEXON-derived output must stay local.

Success writes one JSON result to stdout and exits 0.  Expected input errors
write JSON to stderr and exit 2; unavailable/private outfits exit 4; a failed
download, conversion, or local pack build exits 5.
#>
[CmdletBinding()]
param(
    [string]$InputUrl,

    # Test seam for the no-argument clipboard workflow.  Do not expose this in
    # the UI; an omitted InputUrl normally reads the current Windows clipboard.
    [string]$ClipboardText,

    [string]$DisplayName,
    # Appearance slots 0..7 are built-ins. Values 8+ map to the install-local
    # Maple Atelier catalog in its current displayed order.
    [string]$RemoveAppearanceIndex,
    [string]$SkinRoot,
    [string]$PackToml,
    [string]$BasePack,
    [string]$ValidatorExe,
    [string]$WorkshopModule,
    [string]$WorkRoot = (Join-Path ([System.IO.Path]::GetTempPath()) 'MapleAgentMarket-Atelier-Import'),
    [switch]$ValidateOnly,
    [switch]$KeepWork,
    # Native UI mode: keep the naming form, but let the Rust C panel own the
    # terminal success/error state and hot-reload the rebuilt catalog.
    [switch]$NoCompletionDialog
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scriptRootPath = [string]$PSScriptRoot
# Windows PowerShell 5.1 accepts a verbatim `\\?\` script path for `-File`,
# but its path cmdlets cannot reliably derive a drive from that spelling.
# Normalize only the local spelling; the native launcher already canonicalizes
# and containment-checks the helper before starting this script.
if ($scriptRootPath.StartsWith('\\?\UNC\', [StringComparison]::OrdinalIgnoreCase)) {
    $scriptRootPath = '\\' + $scriptRootPath.Substring(8)
} elseif ($scriptRootPath.StartsWith('\\?\', [StringComparison]::Ordinal)) {
    $scriptRootPath = $scriptRootPath.Substring(4)
}
$script:InstallRoot = Split-Path -Parent $scriptRootPath
if ([string]::IsNullOrWhiteSpace($SkinRoot)) { $SkinRoot = Join-Path $script:InstallRoot 'private-assets\skins' }
if ([string]::IsNullOrWhiteSpace($BasePack)) {
    $baseCandidates = @(
        (Join-Path $script:InstallRoot 'private-assets\skins\base-pack'),
        (Join-Path $script:InstallRoot 'sprites')
    )
    $BasePack = [string](@($baseCandidates | Where-Object { Test-Path -LiteralPath (Join-Path $_ 'pack.toml') -PathType Leaf } | Select-Object -First 1)[0])
    if ([string]::IsNullOrWhiteSpace($BasePack)) { $BasePack = $baseCandidates[0] }
}
if ([string]::IsNullOrWhiteSpace($PackToml)) {
    $activeManifest = Join-Path $script:InstallRoot 'private-assets\skins\active-pack\pack.toml'
    $PackToml = if (Test-Path -LiteralPath $activeManifest -PathType Leaf) { $activeManifest } else { Join-Path $BasePack 'pack.toml' }
}
if ([string]::IsNullOrWhiteSpace($ValidatorExe)) {
    $validatorCandidates = @(
        (Join-Path $script:InstallRoot 'bin\maple-agent-market.exe'),
        (Join-Path $script:InstallRoot 'maple-agent-market.exe'),
        (Join-Path $script:InstallRoot 'target\debug\maple-agent-market.exe'),
        (Join-Path $script:InstallRoot 'target\release\maple-agent-market.exe')
    )
    $ValidatorExe = [string](@($validatorCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1)[0])
    if ([string]::IsNullOrWhiteSpace($ValidatorExe)) { $ValidatorExe = $validatorCandidates[0] }
}
if ([string]::IsNullOrWhiteSpace($WorkshopModule)) { $WorkshopModule = Join-Path $scriptRootPath 'MapleSkinWorkshop.psm1' }
$script:RemoveMode = -not [string]::IsNullOrWhiteSpace($RemoveAppearanceIndex)
$script:ClipboardMode = -not $script:RemoveMode -and [string]::IsNullOrWhiteSpace($InputUrl)
$script:ClipboardInjected = $PSBoundParameters.ContainsKey('ClipboardText')

$script:AtelierRoot = 'https://maple-atelier.org'
$script:MapleIoRoot = 'https://maplestory.io'
$script:DefaultRegion = 'TWMS'
$script:DefaultVersion = '256'
$script:MaxInputCharacters = 16384
$script:MaxRenderPayloadCharacters = 12288
$script:MaxItems = 24
$script:MaxDownloadBytes = 5MB
$script:DownloadTimeoutMs = 15000

function New-ImporterError {
    param(
        [Parameter(Mandatory = $true)][string]$Code,
        [Parameter(Mandatory = $true)][string]$Message
    )
    return [System.InvalidOperationException]::new("$Code|$Message")
}

function Throw-ImporterError {
    param([Parameter(Mandatory = $true)][string]$Code, [Parameter(Mandatory = $true)][string]$Message)
    throw (New-ImporterError -Code $Code -Message $Message)
}

function Get-ImportInputUrl {
    if (-not [string]::IsNullOrWhiteSpace($InputUrl)) { return $InputUrl }
    if ($script:ClipboardInjected) { return $ClipboardText }
    try {
        Add-Type -AssemblyName System.Windows.Forms
        if (-not [System.Windows.Forms.Clipboard]::ContainsText()) { Throw-ImporterError 'INPUT_INVALID' '剪貼簿沒有可匯入的造型網址。' }
        return [System.Windows.Forms.Clipboard]::GetText()
    } catch [System.Management.Automation.RuntimeException] { throw }
      catch { Throw-ImporterError 'INPUT_INVALID' '無法讀取剪貼簿中的造型網址。' }
}

function Show-ClipboardDialog {
    param([Parameter(Mandatory = $true)][string]$Title, [Parameter(Mandatory = $true)][string]$Text, [switch]$Error)
    if (-not $script:ClipboardMode -or $script:ClipboardInjected -or $ValidateOnly -or $NoCompletionDialog) { return }
    try {
        Add-Type -AssemblyName System.Windows.Forms
        $icon = if ($Error) { [System.Windows.Forms.MessageBoxIcon]::Error } else { [System.Windows.Forms.MessageBoxIcon]::Information }
        [void][System.Windows.Forms.MessageBox]::Show($Text, $Title, [System.Windows.Forms.MessageBoxButtons]::OK, $icon)
    } catch { }
}

function Get-ClipboardDisplayName {
    param([Parameter(Mandatory = $true)]$Source)
    if (-not $script:ClipboardMode -or $script:ClipboardInjected -or -not [string]::IsNullOrWhiteSpace($DisplayName)) { return $DisplayName }
    # Public Maple Atelier outfits already have a user-authored title. A raw
    # renderer URL does not, so avoid accumulating indistinguishable entries.
    if ([string]$Source.sourceKind -ne 'maplestory-character-render') { return $DisplayName }
    try {
        Add-Type -AssemblyName System.Windows.Forms
        $form = New-Object System.Windows.Forms.Form
        $form.Text = 'Maple Agent Market｜新增角色'
        $form.StartPosition = [System.Windows.Forms.FormStartPosition]::CenterScreen
        $form.FormBorderStyle = [System.Windows.Forms.FormBorderStyle]::FixedDialog
        $form.MinimizeBox = $false
        $form.MaximizeBox = $false
        $form.TopMost = $true
        $form.ShowInTaskbar = $true
        $form.ClientSize = New-Object System.Drawing.Size(370, 126)
        $label = New-Object System.Windows.Forms.Label
        $label.AutoSize = $true
        $label.Text = '替這個紙娃娃輸入名稱（可自行命名）：'
        $label.Location = New-Object System.Drawing.Point(14, 16)
        $box = New-Object System.Windows.Forms.TextBox
        $box.Text = '自訂造型'
        $box.MaxLength = 60
        $box.Width = 340
        $box.Location = New-Object System.Drawing.Point(14, 42)
        $ok = New-Object System.Windows.Forms.Button
        $ok.Text = '加入'
        $ok.DialogResult = [System.Windows.Forms.DialogResult]::OK
        $ok.Location = New-Object System.Drawing.Point(202, 82)
        $cancel = New-Object System.Windows.Forms.Button
        $cancel.Text = '取消'
        $cancel.DialogResult = [System.Windows.Forms.DialogResult]::Cancel
        $cancel.Location = New-Object System.Drawing.Point(282, 82)
        $form.AcceptButton = $ok
        $form.CancelButton = $cancel
        [void]$form.Controls.AddRange(@($label, $box, $ok, $cancel))
        $form.Add_Shown({
            [void]$form.BringToFront()
            [void]$form.Activate()
            [void]$box.Focus()
            $box.SelectAll()
        })
        $dialogResult = $form.ShowDialog()
        if ($dialogResult -ne [System.Windows.Forms.DialogResult]::OK) { Throw-ImporterError 'IMPORT_CANCELLED' '已取消新增角色，沒有變更任何本機素材。' }
        $value = $box.Text.Trim()
        $form.Dispose()
        if ([string]::IsNullOrWhiteSpace($value)) { Throw-ImporterError 'INPUT_INVALID' '角色名稱不可留白。' }
        return $value
    } catch [System.InvalidOperationException] { throw }
      catch { Throw-ImporterError 'INPUT_INVALID' '無法顯示角色命名視窗。' }
}

function Get-ObjectProperty {
    param([Parameter(Mandatory = $true)]$Object, [Parameter(Mandatory = $true)][string]$Name)
    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property) { return $null }
    return $property.Value
}

function Get-OptionalText {
    param($Value, [Parameter(Mandatory = $true)][string]$Fallback)
    if ($null -eq $Value -or [string]::IsNullOrWhiteSpace([string]$Value)) { return $Fallback }
    return [string]$Value
}

function Get-UrlQueryMap {
    param([Parameter(Mandatory = $true)][Uri]$Uri)
    $values = @{}
    $text = $Uri.Query.TrimStart('?')
    if ([string]::IsNullOrWhiteSpace($text)) { return $values }
    foreach ($pair in $text.Split('&')) {
        if ([string]::IsNullOrWhiteSpace($pair)) { Throw-ImporterError 'INPUT_INVALID' '網址查詢參數格式無效。' }
        $parts = $pair.Split('=', 2)
        $key = [Uri]::UnescapeDataString($parts[0])
        $value = if ($parts.Count -eq 2) { [Uri]::UnescapeDataString($parts[1]) } else { '' }
        if ([string]::IsNullOrWhiteSpace($key) -or $values.ContainsKey($key)) { Throw-ImporterError 'INPUT_INVALID' '網址查詢參數格式無效。' }
        $values[$key] = $value
    }
    return $values
}

function Assert-SafeRegionVersion {
    param([Parameter(Mandatory = $true)][string]$Region, [Parameter(Mandatory = $true)][string]$Version)
    if ($Region -notmatch '^[A-Z]{2,8}$') { Throw-ImporterError 'INPUT_INVALID' '角色素材區服格式不正確。' }
    if ($Version -notmatch '^[0-9]{1,6}[A-Za-z]?$') { Throw-ImporterError 'INPUT_INVALID' '角色素材版本格式不正確。' }
}

function ConvertTo-AtelierOutfitId {
    param([Parameter(Mandatory = $true)][string]$Value)
    if ($Value -notmatch '^[0-9]{1,10}$') { Throw-ImporterError 'INPUT_INVALID' 'Maple Atelier 造型 ID 格式無效。' }
    $id = [Int64]$Value
    if ($id -lt 1 -or $id -gt [Int32]::MaxValue) { Throw-ImporterError 'INPUT_INVALID' 'Maple Atelier 造型 ID 超出範圍。' }
    return [int]$id
}

function ConvertTo-CharacterItem {
    param([Parameter(Mandatory = $true)]$Entry)
    $itemIdValue = Get-ObjectProperty -Object $Entry -Name 'itemId'
    if ($null -eq $itemIdValue -or -not ([string]$itemIdValue -match '^[0-9]{1,10}$')) {
        Throw-ImporterError 'INPUT_INVALID' '角色網址含有無效的 itemId。'
    }
    $itemId = [Int64]$itemIdValue
    if ($itemId -lt 1 -or $itemId -gt [Int32]::MaxValue) { Throw-ImporterError 'INPUT_INVALID' '角色網址含有超出範圍的 itemId。' }
    $region = Get-OptionalText -Value (Get-ObjectProperty -Object $Entry -Name 'region') -Fallback $script:DefaultRegion
    $version = Get-OptionalText -Value (Get-ObjectProperty -Object $Entry -Name 'version') -Fallback $script:DefaultVersion
    Assert-SafeRegionVersion -Region $region -Version $version
    $result = [ordered]@{ itemId = [int]$itemId; region = $region; version = $version }
    $animationName = Get-ObjectProperty -Object $Entry -Name 'animationName'
    if ($null -ne $animationName) {
        $text = [string]$animationName
        if ($text -notmatch '^[A-Za-z0-9_-]{1,48}$') { Throw-ImporterError 'INPUT_INVALID' '角色網址含有無效的臉部表情。' }
        $result.animationName = $text
    }
    return [pscustomobject]$result
}

function ConvertFrom-MapleIoCharacterRenderUrl {
    param([Parameter(Mandatory = $true)][Uri]$Uri)
    $segments = @($Uri.Segments | ForEach-Object { $_.Trim('/') } | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    if ($segments.Count -ne 5 -or $segments[0] -ne 'api' -or $segments[1] -ne 'character' -or [string]::IsNullOrWhiteSpace($segments[2])) {
        Throw-ImporterError 'INPUT_INVALID' 'maplestory.io 網址不是角色渲染網址。'
    }
    if ($segments[3] -notmatch '^[A-Za-z0-9_-]{1,48}$' -or $segments[4] -notmatch '^(animated|[0-9]{1,4})$') {
        Throw-ImporterError 'INPUT_INVALID' 'maplestory.io 角色渲染網址的姿勢或影格無效。'
    }
    $payload = [Uri]::UnescapeDataString($segments[2])
    if ($payload.Length -lt 2 -or $payload.Length -gt $script:MaxRenderPayloadCharacters -or $payload.Contains('[') -or $payload.Contains(']')) {
        Throw-ImporterError 'INPUT_INVALID' 'maplestory.io 角色資料長度或格式無效。'
    }
    try {
        # Windows PowerShell 5.1 keeps a JSON array as one Object[] pipeline
        # item, whereas newer PowerShell versions enumerate it. Normalize both.
        $decodedItems = ("[{0}]" -f $payload) | ConvertFrom-Json
        $rawItems = @()
        foreach ($decodedItem in $decodedItems) { $rawItems += $decodedItem }
    } catch { Throw-ImporterError 'INPUT_INVALID' 'maplestory.io 角色資料不是有效 JSON。' }
    if ($rawItems.Count -lt 1 -or $rawItems.Count -gt $script:MaxItems) { Throw-ImporterError 'INPUT_INVALID' '角色物品數量超出可接受範圍。' }
    $items = @($rawItems | ForEach-Object { ConvertTo-CharacterItem -Entry $_ })
    $query = Get-UrlQueryMap -Uri $Uri
    foreach ($key in @($query.Keys)) {
        if ($key -notin @('showears', 'showLefEars', 'showHighLefEars', 'resize', 'flipX', 'renderMode', 'padX', 'padY')) {
            Throw-ImporterError 'INPUT_INVALID' 'maplestory.io 角色網址含有不支援的查詢參數。'
        }
    }
    $flags = [ordered]@{}
    foreach ($name in @('showears', 'showLefEars', 'showHighLefEars')) {
        $value = $query[$name]
        if ($null -eq $value -or [string]::IsNullOrWhiteSpace($value)) { $flags[$name] = $false; continue }
        if ($value -notin @('true', 'false')) { Throw-ImporterError 'INPUT_INVALID' "角色網址的 $name 不是 true 或 false。" }
        $flags[$name] = [System.Convert]::ToBoolean($value)
    }
    $expression = @($items | Where-Object { $null -ne $_.PSObject.Properties['animationName'] } | Select-Object -First 1 -ExpandProperty animationName)
    return [pscustomobject]@{
        sourceKind = 'maplestory-character-render'
        sourceUrl = $Uri.AbsoluteUri
        sourceId = $null
        title = '自訂造型'
        author = $null
        character = [pscustomobject]@{
            items = $items
            expression = if ($expression.Count -gt 0) { [string]$expression[0] } else { 'default' }
            earFlags = [pscustomobject]$flags
        }
    }
}

function Get-Utf8Json {
    param([Parameter(Mandatory = $true)][string]$Uri)
    $request = [System.Net.HttpWebRequest]::Create($Uri)
    $request.Method = 'GET'
    $request.Timeout = $script:DownloadTimeoutMs
    $request.ReadWriteTimeout = $script:DownloadTimeoutMs
    $request.AllowAutoRedirect = $false
    $request.UserAgent = 'Maple-Agent-Market/local-character-import'
    try {
        $response = $request.GetResponse()
        try {
            if ([int]$response.StatusCode -ne 200) { Throw-ImporterError 'SOURCE_UNAVAILABLE' "上游服務回傳 HTTP $([int]$response.StatusCode)。" }
            if ($response.ContentLength -gt $script:MaxDownloadBytes) { Throw-ImporterError 'SOURCE_UNAVAILABLE' '上游回應超過大小限制。' }
            $reader = New-Object System.IO.StreamReader($response.GetResponseStream(), [System.Text.Encoding]::UTF8, $true)
            try { return $reader.ReadToEnd() } finally { $reader.Dispose() }
        } finally { $response.Dispose() }
    } catch [System.Net.WebException] {
        if ($null -ne $_.Exception.Response -and [int]$_.Exception.Response.StatusCode -eq 404) {
            Throw-ImporterError 'OUTFIT_UNAVAILABLE' '找不到公開造型；該造型可能不存在或是私人作品。'
        }
        Throw-ImporterError 'SOURCE_UNAVAILABLE' '無法讀取 Maple Atelier 造型資料。'
    }
}

function ConvertFrom-AtelierPayload {
    param(
        [Parameter(Mandatory = $true)]$Payload,
        [Parameter(Mandatory = $true)][string]$SourceKind,
        [Parameter(Mandatory = $true)][string]$SourceUrl,
        [Parameter(Mandatory = $true)][int]$OutfitId,
        [string]$Title,
        [string]$Author
    )
    $slots = Get-ObjectProperty -Object $Payload -Name 'slots'
    if ($null -eq $slots) { Throw-ImporterError 'INPUT_INVALID' 'Maple Atelier 造型缺少 slots。' }
    $entries = New-Object 'System.Collections.Generic.List[object]'
    $skin = Get-ObjectProperty -Object $slots -Name 'skin'
    if ($null -eq $skin) { Throw-ImporterError 'INPUT_INVALID' 'Maple Atelier 造型缺少 skin。' }
    $skinId = Get-ObjectProperty -Object $skin -Name 'id'
    if ($null -eq $skinId -or -not ([string]$skinId -match '^[0-9]{1,10}$')) { Throw-ImporterError 'INPUT_INVALID' 'Maple Atelier 造型含有無效 skin。' }
    $skinRegion = Get-OptionalText -Value (Get-ObjectProperty -Object $skin -Name 'region') -Fallback $script:DefaultRegion
    $skinVersion = Get-OptionalText -Value (Get-ObjectProperty -Object $skin -Name 'version') -Fallback $script:DefaultVersion
    Assert-SafeRegionVersion -Region $skinRegion -Version $skinVersion
    [void]$entries.Add((ConvertTo-CharacterItem -Entry ([pscustomobject]@{ itemId = [int]$skinId; region = $skinRegion; version = $skinVersion })))
    [void]$entries.Add((ConvertTo-CharacterItem -Entry ([pscustomobject]@{ itemId = ([int]$skinId + 10000); region = $skinRegion; version = $skinVersion })))
    foreach ($property in $slots.PSObject.Properties) {
        if ($property.Name -in @('skin', 'ear') -or $null -eq $property.Value) { continue }
        $item = $property.Value
        $entry = [ordered]@{
            itemId = Get-ObjectProperty -Object $item -Name 'id'
            region = Get-OptionalText -Value (Get-ObjectProperty -Object $item -Name 'region') -Fallback $script:DefaultRegion
            version = Get-OptionalText -Value (Get-ObjectProperty -Object $item -Name 'version') -Fallback $script:DefaultVersion
        }
        if ($property.Name -eq 'face') { $entry.animationName = Get-OptionalText -Value (Get-ObjectProperty -Object $Payload -Name 'expression') -Fallback 'default' }
        [void]$entries.Add((ConvertTo-CharacterItem -Entry ([pscustomobject]$entry)))
    }
    if ($entries.Count -gt $script:MaxItems) { Throw-ImporterError 'INPUT_INVALID' 'Maple Atelier 造型物品數量超出可接受範圍。' }
    $ear = Get-ObjectProperty -Object $slots -Name 'ear'
    $earId = if ($null -ne $ear) { Get-ObjectProperty -Object $ear -Name 'id' } else { 90000 }
    if ([string]$earId -notmatch '^9000[0-3]$') { $earId = 90000 }
    return [pscustomobject]@{
        sourceKind = $SourceKind
        sourceUrl = $SourceUrl
        sourceId = $OutfitId
        title = Get-OptionalText -Value $Title -Fallback ('Atelier-{0}' -f $OutfitId)
        author = $Author
        character = [pscustomobject]@{
            items = $entries.ToArray()
            expression = Get-OptionalText -Value (Get-ObjectProperty -Object $Payload -Name 'expression') -Fallback 'default'
            earFlags = [pscustomobject]@{
                showears = ([int]$earId -eq 90001)
                showLefEars = ([int]$earId -eq 90002)
                showHighLefEars = ([int]$earId -eq 90003)
            }
        }
    }
}

function Get-AtelierOutfit {
    param([Parameter(Mandatory = $true)][int]$OutfitId, [Parameter(Mandatory = $true)][string]$SourceKind)
    $raw = Get-Utf8Json -Uri ("$($script:AtelierRoot)/api/outfits/$OutfitId")
    try { $outfit = $raw | ConvertFrom-Json } catch { Throw-ImporterError 'SOURCE_UNAVAILABLE' 'Maple Atelier 回傳了無法解析的造型資料。' }
    if ($null -eq $outfit -or -not [bool](Get-ObjectProperty -Object $outfit -Name 'isPublic')) {
        Throw-ImporterError 'OUTFIT_UNAVAILABLE' '找不到公開造型；該造型可能不存在或是私人作品。'
    }
    return ConvertFrom-AtelierPayload -Payload (Get-ObjectProperty -Object $outfit -Name 'payload') -SourceKind $SourceKind -SourceUrl ("$($script:AtelierRoot)/outfit/$OutfitId") -OutfitId $OutfitId -Title ([string](Get-ObjectProperty -Object $outfit -Name 'title')) -Author ([string](Get-ObjectProperty -Object $outfit -Name 'authorName'))
}

function ConvertFrom-AtelierInputUrl {
    param([Parameter(Mandatory = $true)][string]$Value)
    if ([string]::IsNullOrWhiteSpace($Value) -or $Value.Length -gt $script:MaxInputCharacters) { Throw-ImporterError 'INPUT_INVALID' '請貼上長度合理的 HTTPS 造型網址。' }
    try { $uri = [Uri]$Value } catch { Throw-ImporterError 'INPUT_INVALID' '造型網址無法解析。' }
    if (-not $uri.IsAbsoluteUri -or $uri.Scheme -ne 'https' -or -not [string]::IsNullOrEmpty($uri.UserInfo)) { Throw-ImporterError 'INPUT_INVALID' '只接受不含帳密的 HTTPS 造型網址。' }
    if ($uri.Host -eq 'maplestory.io') { return ConvertFrom-MapleIoCharacterRenderUrl -Uri $uri }
    if ($uri.Host -ne 'maple-atelier.org') { Throw-ImporterError 'INPUT_INVALID' '只接受 maple-atelier.org 或 maplestory.io 網址。' }
    $path = $uri.AbsolutePath.TrimEnd('/')
    if ($path -match '^/outfit/(?<id>[0-9]{1,10})$') {
        if (-not [string]::IsNullOrWhiteSpace($uri.Query)) { Throw-ImporterError 'INPUT_INVALID' 'Maple Atelier outfit 網址不可含查詢參數。' }
        return Get-AtelierOutfit -OutfitId (ConvertTo-AtelierOutfitId -Value $matches.id) -SourceKind 'maple-atelier-outfit'
    }
    if ($path -eq '/simulator') {
        $query = Get-UrlQueryMap -Uri $uri
        if (@($query.Keys | Where-Object { $_ -ne 'load' }).Count -gt 0 -or $query['load'] -notmatch '^[0-9]{1,10}$') {
            Throw-ImporterError 'INPUT_INVALID' '模擬器網址必須是 /simulator?load=公開造型ID。'
        }
        return Get-AtelierOutfit -OutfitId (ConvertTo-AtelierOutfitId -Value $query['load']) -SourceKind 'maple-atelier-simulator-load'
    }
    Throw-ImporterError 'INPUT_INVALID' 'Maple Atelier 網址必須是 /outfit/{id} 或 /simulator?load={id}。'
}

function Get-CharacterRenderUri {
    param([Parameter(Mandatory = $true)]$Character, [Parameter(Mandatory = $true)][string]$Stance)
    $json = ConvertTo-Json -InputObject ([object[]]@($Character.items)) -Depth 5 -Compress
    if ($json.Length -lt 2 -or $json[0] -ne '[' -or $json[$json.Length - 1] -ne ']') { Throw-ImporterError 'INPUT_INVALID' '角色物品資料無法轉換為渲染請求。' }
    $payload = [Uri]::EscapeDataString($json.Substring(1, $json.Length - 2))
    $flags = $Character.earFlags
    return ('{0}/api/character/{1}/{2}/animated?showears={3}&showLefEars={4}&showHighLefEars={5}&resize=1&flipX=false&renderMode=1&padX=30&padY=50' -f $script:MapleIoRoot, $payload, $Stance, ([bool]$flags.showears).ToString().ToLowerInvariant(), ([bool]$flags.showLefEars).ToString().ToLowerInvariant(), ([bool]$flags.showHighLefEars).ToString().ToLowerInvariant())
}

function Save-RemoteFile {
    param([Parameter(Mandatory = $true)][string]$Uri, [Parameter(Mandatory = $true)][string]$Path)
    $request = [System.Net.HttpWebRequest]::Create($Uri)
    $request.Method = 'GET'
    $request.Timeout = $script:DownloadTimeoutMs
    $request.ReadWriteTimeout = $script:DownloadTimeoutMs
    $request.AllowAutoRedirect = $false
    $request.UserAgent = 'Maple-Agent-Market/local-character-import'
    $response = $request.GetResponse()
    try {
        if ([int]$response.StatusCode -ne 200) { Throw-ImporterError 'SOURCE_UNAVAILABLE' "角色渲染服務回傳 HTTP $([int]$response.StatusCode)。" }
        if ($response.ContentLength -gt $script:MaxDownloadBytes) { Throw-ImporterError 'SOURCE_UNAVAILABLE' '角色動畫檔超過大小限制。' }
        $input = $response.GetResponseStream()
        $output = [System.IO.File]::Create($Path)
        try {
            $buffer = New-Object byte[] 81920
            $total = 0L
            while (($read = $input.Read($buffer, 0, $buffer.Length)) -gt 0) {
                $total += $read
                if ($total -gt $script:MaxDownloadBytes) { Throw-ImporterError 'SOURCE_UNAVAILABLE' '角色動畫檔超過大小限制。' }
                $output.Write($buffer, 0, $read)
            }
        } finally { $output.Dispose(); $input.Dispose() }
    } finally { $response.Dispose() }
}

function Export-NormalizedAnimationFrames {
    param([Parameter(Mandatory = $true)][string]$GifPath, [Parameter(Mandatory = $true)][string[]]$FrameNames, [Parameter(Mandatory = $true)][string]$Destination)
    Add-Type -AssemblyName System.Drawing
    $image = [System.Drawing.Image]::FromFile($GifPath)
    try {
        if ($image.Width -ne 96 -or $image.Height -ne 96) { Throw-ImporterError 'SOURCE_UNAVAILABLE' "$(Split-Path -Leaf $GifPath) 必須是 96 x 96 角色動畫。" }
        $dimension = New-Object System.Drawing.Imaging.FrameDimension($image.FrameDimensionsList[0])
        $frameCount = $image.GetFrameCount($dimension)
        if ($frameCount -lt 1) { Throw-ImporterError 'SOURCE_UNAVAILABLE' "$(Split-Path -Leaf $GifPath) 不含動畫影格。" }
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
                    $graphics.DrawImage($image, (New-Object System.Drawing.Rectangle 12, 0, 72, 72), (New-Object System.Drawing.Rectangle 0, 0, 96, 96), [System.Drawing.GraphicsUnit]::Pixel)
                } finally { $graphics.Dispose() }
                $canvas.Save((Join-Path $Destination ($FrameNames[$index] + '.png')), [System.Drawing.Imaging.ImageFormat]::Png)
            } finally { $canvas.Dispose() }
        }
    } finally { $image.Dispose() }
}

function Set-ImportedMetadata {
    param([Parameter(Mandatory = $true)]$Imported, [Parameter(Mandatory = $true)]$Source)
    $metadataPath = Join-Path $Imported.path 'metadata.json'
    $metadata = [System.IO.File]::ReadAllText($metadataPath, [System.Text.Encoding]::UTF8) | ConvertFrom-Json
    $metadata.rights = 'nexon-derived-local-evaluation-only'
    $metadata | Add-Member -NotePropertyName sourceType -NotePropertyValue ([string]$Source.sourceKind) -Force
    $metadata | Add-Member -NotePropertyName sourceUrl -NotePropertyValue ([string]$Source.sourceUrl) -Force
    $metadata | Add-Member -NotePropertyName sourceOutfitId -NotePropertyValue $Source.sourceId -Force
    $metadata | Add-Member -NotePropertyName sourceTitle -NotePropertyValue ([string]$Source.title) -Force
    $metadata | Add-Member -NotePropertyName sourceAuthor -NotePropertyValue $Source.author -Force
    $metadata | Add-Member -NotePropertyName assetProvider -NotePropertyValue $script:MapleIoRoot -Force
    [System.IO.File]::WriteAllText($metadataPath, (($metadata | ConvertTo-Json -Depth 10) + [Environment]::NewLine), (New-Object System.Text.UTF8Encoding($false)))
}

function Save-JsonAtomic {
    param([Parameter(Mandatory = $true)]$Value, [Parameter(Mandatory = $true)][string]$Path)
    $fullPath = [System.IO.Path]::GetFullPath($Path)
    $parent = Split-Path -Parent $fullPath
    [void][System.IO.Directory]::CreateDirectory($parent)
    $temporary = Join-Path $parent ('.j-' + [guid]::NewGuid().ToString('N').Substring(0, 16))
    [System.IO.File]::WriteAllText($temporary, (($Value | ConvertTo-Json -Depth 12) + [Environment]::NewLine), (New-Object System.Text.UTF8Encoding($false)))
    if (Test-Path -LiteralPath $fullPath -PathType Leaf) { [System.IO.File]::Replace($temporary, $fullPath, ($fullPath + '.bak'), $true) } else { [System.IO.File]::Move($temporary, $fullPath) }
}

function Get-RemovalAppearanceIndex {
    if ($RemoveAppearanceIndex -notmatch '^[0-9]{1,6}$') {
        Throw-ImporterError 'INPUT_INVALID' '刪除角色索引格式無效。'
    }
    $value = [int]$RemoveAppearanceIndex
    if ($value -lt 8) {
        Throw-ImporterError 'INPUT_INVALID' '內建角色是必要素材，不能刪除。'
    }
    return $value
}

function Invoke-CatalogRemoval {
    param([Parameter(Mandatory = $true)][int]$AppearanceIndex)
    if (-not (Test-Path -LiteralPath $WorkshopModule -PathType Leaf)) { Throw-ImporterError 'LOCAL_CONFIGURATION' "找不到角色工作坊模組：$WorkshopModule" }
    foreach ($path in @((Join-Path $BasePack 'pack.toml'), $ValidatorExe)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { Throw-ImporterError 'LOCAL_CONFIGURATION' "找不到本機必要檔案：$path" }
    }
    Import-Module -Name $WorkshopModule -Force
    $root = [System.IO.Path]::GetFullPath($SkinRoot)
    $catalogPath = Join-Path $root 'maple-atelier-catalog.json'
    if (-not (Test-Path -LiteralPath $catalogPath -PathType Leaf)) { Throw-ImporterError 'LOCAL_CONFIGURATION' '找不到可刪除的本機角色清單。' }
    $originalCatalogText = [System.IO.File]::ReadAllText($catalogPath, [System.Text.Encoding]::UTF8)
    $catalog = $originalCatalogText | ConvertFrom-Json
    $characters = @($catalog.characters)
    $catalogIndex = $AppearanceIndex - 8
    if ($catalogIndex -lt 0 -or $catalogIndex -ge $characters.Count) { Throw-ImporterError 'INPUT_INVALID' '指定的自訂角色已不存在，請重新開啟清單。' }
    $removed = $characters[$catalogIndex]
    $skinId = [string]$removed.skinId
    if ($skinId -notmatch '^user-[0-9a-f]{12}$') { Throw-ImporterError 'INPUT_INVALID' '只有本機匯入的自訂角色可以刪除。' }

    $importsRoot = [System.IO.Path]::GetFullPath((Join-Path $root 'imports'))
    $sourcePath = [System.IO.Path]::GetFullPath((Join-Path $importsRoot $skinId))
    $childPrefix = $importsRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
    if (-not $sourcePath.StartsWith($childPrefix, [StringComparison]::OrdinalIgnoreCase) -or -not (Test-Path -LiteralPath $sourcePath -PathType Container)) {
        Throw-ImporterError 'LOCAL_CONFIGURATION' '找不到自訂角色的本機素材目錄。'
    }

    $remaining = New-Object System.Collections.Generic.List[object]
    for ($index = 0; $index -lt $characters.Count; $index++) {
        if ($index -ne $catalogIndex) { [void]$remaining.Add($characters[$index]) }
    }
    $catalog.characters = @($remaining.ToArray())
    $catalog | Add-Member -NotePropertyName generatedUtc -NotePropertyValue ([DateTime]::UtcNow.ToString('o')) -Force

    # Move the imported source to a recovery folder before publishing the new
    # catalog. If either the JSON write or pack rebuild fails, restore both the
    # source folder and the original catalog so the UI can safely retry.
    $deletedRoot = Join-Path $root 'deleted'
    [void][System.IO.Directory]::CreateDirectory($deletedRoot)
    $deletedPath = Join-Path $deletedRoot (([DateTime]::UtcNow.ToString('yyyyMMdd-HHmmssfff')) + '-' + $skinId)
    [System.IO.Directory]::Move($sourcePath, $deletedPath)
    try {
        Save-JsonAtomic -Value $catalog -Path $catalogPath
        $catalogPack = New-MapleCatalogSkinPack -BasePack $BasePack -SkinRoot $root -CatalogPath $catalogPath -ValidatorExe $ValidatorExe
    } catch {
        try {
            Save-JsonAtomic -Value ($originalCatalogText | ConvertFrom-Json) -Path $catalogPath
        } finally {
            if ((Test-Path -LiteralPath $deletedPath -PathType Container) -and -not (Test-Path -LiteralPath $sourcePath)) {
                [System.IO.Directory]::Move($deletedPath, $sourcePath)
            }
        }
        throw
    }
    return [pscustomobject]@{
        ok = $true
        mode = 'remove'
        removed = $true
        removedAppearanceIndex = $AppearanceIndex
        skinId = $skinId
        title = [string]$removed.title
        catalogPath = $catalogPath
        catalogPack = [string]$catalogPack
        recoverablePath = $deletedPath
    }
}

function Invoke-Import {
    param([Parameter(Mandatory = $true)]$Source)
    if (-not (Test-Path -LiteralPath $WorkshopModule -PathType Leaf)) { Throw-ImporterError 'LOCAL_CONFIGURATION' "找不到角色工作坊模組：$WorkshopModule" }
    foreach ($path in @($PackToml, (Join-Path $BasePack 'pack.toml'), $ValidatorExe)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { Throw-ImporterError 'LOCAL_CONFIGURATION' "找不到本機必要檔案：$path" }
    }
    Import-Module -Name $WorkshopModule -Force
    [void][System.IO.Directory]::CreateDirectory([System.IO.Path]::GetFullPath($SkinRoot))
    [void][System.IO.Directory]::CreateDirectory([System.IO.Path]::GetFullPath($WorkRoot))
    $runRoot = Join-Path ([System.IO.Path]::GetFullPath($WorkRoot)) ('run-' + [guid]::NewGuid().ToString('N'))
    [void][System.IO.Directory]::CreateDirectory($runRoot)
    try {
        $safeTitle = ((Get-OptionalText -Value $DisplayName -Fallback ([string]$Source.title)) -replace '[\\/:*?"<>|]', '_').Trim()
        if ([string]::IsNullOrWhiteSpace($safeTitle)) { $safeTitle = '自訂角色' }
        if ($safeTitle.Length -gt 60) { $safeTitle = $safeTitle.Substring(0, 60) }
        $sourceFolder = Join-Path $runRoot $safeTitle
        [void][System.IO.Directory]::CreateDirectory($sourceFolder)
        $animations = @(
            [pscustomobject]@{ Stance = 'stand1'; Frames = @('stand-0', 'stand-1', 'stand-2') },
            [pscustomobject]@{ Stance = 'walk1'; Frames = @('walk-0', 'walk-1', 'walk-2', 'walk-3') },
            [pscustomobject]@{ Stance = 'ladder'; Frames = @('climb-0', 'climb-1') },
            [pscustomobject]@{ Stance = 'stand2'; Frames = @('stand2-0', 'stand2-1', 'stand2-2') },
            [pscustomobject]@{ Stance = 'alert'; Frames = @('alert-0', 'alert-1', 'alert-2') },
            [pscustomobject]@{ Stance = 'sit'; Frames = @('sit-0') }
        )
        foreach ($animation in $animations) {
            $gifPath = Join-Path $sourceFolder ($animation.Stance + '.gif')
            Save-RemoteFile -Uri (Get-CharacterRenderUri -Character $Source.character -Stance $animation.Stance) -Path $gifPath
            Export-NormalizedAnimationFrames -GifPath $gifPath -FrameNames $animation.Frames -Destination $sourceFolder
            Remove-Item -LiteralPath $gifPath -Force
        }
        $imported = Import-MapleSkinFolder -SourceFolder $sourceFolder -SkinRoot $SkinRoot -PackToml $PackToml
        Set-ImportedMetadata -Imported $imported -Source $Source
        $catalogPath = Join-Path ([System.IO.Path]::GetFullPath($SkinRoot)) 'maple-atelier-catalog.json'
        if (Test-Path -LiteralPath $catalogPath -PathType Leaf) { $catalog = [System.IO.File]::ReadAllText($catalogPath, [System.Text.Encoding]::UTF8) | ConvertFrom-Json } else { $catalog = $null }
        $existing = if ($null -ne $catalog) { @($catalog.characters) } else { @() }
        $existingEntry = @($existing | Where-Object { [string]$_.skinId -eq [string]$imported.id } | Select-Object -First 1)
        $added = $existingEntry.Count -eq 0
        if ($added) {
            $entry = [pscustomobject]@{
                outfitId = $Source.sourceId
                title = $safeTitle
                author = $Source.author
                skinId = [string]$imported.id
                previewPath = [string]$imported.previewPath
                sourceUrl = [string]$Source.sourceUrl
                sourceType = [string]$Source.sourceKind
            }
            $existing += $entry
            $catalog = [pscustomobject]@{
                schemaVersion = 1
                generatedUtc = [DateTime]::UtcNow.ToString('o')
                notice = 'NEXON-derived images for local evaluation only; never include in Git or public releases.'
                gallery = "$($script:AtelierRoot)/api/outfits/public?sort=popular&limit=100"
                assetProvider = $script:MapleIoRoot
                characters = @($existing)
            }
            Save-JsonAtomic -Value $catalog -Path $catalogPath
        }
        $catalogPack = New-MapleCatalogSkinPack -BasePack $BasePack -SkinRoot $SkinRoot -CatalogPath $catalogPath -ValidatorExe $ValidatorExe
        return [pscustomobject]@{
            ok = $true
            mode = 'import'
            added = $added
            sourceKind = [string]$Source.sourceKind
            sourceUrl = [string]$Source.sourceUrl
            skinId = [string]$imported.id
            title = $safeTitle
            catalogPath = $catalogPath
            catalogPack = [string]$catalogPack
        }
    } finally {
        if (-not $KeepWork -and (Test-Path -LiteralPath $runRoot -PathType Container)) { Remove-Item -LiteralPath $runRoot -Recurse -Force }
    }
}

try {
    [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
    if ($script:RemoveMode) {
        if (-not [string]::IsNullOrWhiteSpace($InputUrl) -or $script:ClipboardInjected) { Throw-ImporterError 'INPUT_INVALID' '刪除角色時不能同時提供匯入網址。' }
        $result = Invoke-CatalogRemoval -AppearanceIndex (Get-RemovalAppearanceIndex)
        $result | ConvertTo-Json -Depth 10 -Compress
        exit 0
    }
    $source = ConvertFrom-AtelierInputUrl -Value (Get-ImportInputUrl)
    $DisplayName = Get-ClipboardDisplayName -Source $source
    if ($ValidateOnly) {
        [pscustomobject]@{ ok = $true; mode = 'validate'; sourceKind = $source.sourceKind; sourceUrl = $source.sourceUrl; sourceId = $source.sourceId; character = $source.character } | ConvertTo-Json -Depth 10 -Compress
        exit 0
    }
    $result = Invoke-Import -Source $source
    $result | ConvertTo-Json -Depth 10 -Compress
    Show-ClipboardDialog -Title 'Maple Agent Market' -Text ("已加入本機角色清單：{0}" -f $result.title)
    exit 0
} catch {
    $message = $_.Exception.Message
    $parts = $message -split '\|', 2
    $code = if ($parts.Count -eq 2 -and $parts[0] -match '^[A-Z_]+$') { $parts[0] } else { 'IMPORT_FAILED' }
    $text = if ($parts.Count -eq 2) { $parts[1] } else { $message }
    [Console]::Error.WriteLine(([pscustomobject]@{ ok = $false; error = [pscustomobject]@{ code = $code; message = $text } } | ConvertTo-Json -Compress))
    Show-ClipboardDialog -Title 'Maple Agent Market' -Text $text -Error
    if ($code -eq 'IMPORT_CANCELLED') { exit 3 }
    if ($code -eq 'INPUT_INVALID') { exit 2 }
    if ($code -eq 'OUTFIT_UNAVAILABLE') { exit 4 }
    exit 5
}
