Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$modulePath = Join-Path (Split-Path -Parent $PSScriptRoot) 'MapleSkinWorkshop.psm1'
Import-Module -Name $modulePath -Force

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..\..')).Path
$installRoot = (Resolve-Path -LiteralPath (Join-Path $repoRoot '..\maple-agent-market-zh-TW')).Path
$basePack = if ($env:MAPLE_TEST_BASE_PACK) {
    (Resolve-Path -LiteralPath $env:MAPLE_TEST_BASE_PACK).Path
} else {
    Join-Path $installRoot 'sprites'
}
$validatorExe = if ($env:MAPLE_TEST_VALIDATOR_EXE) {
    (Resolve-Path -LiteralPath $env:MAPLE_TEST_VALIDATOR_EXE).Path
} else {
    Join-Path $installRoot 'bin\maple-agent-market.exe'
}
$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) ('maple-skin-workshop-tests-' + [guid]::NewGuid().ToString('N'))
[void][System.IO.Directory]::CreateDirectory($testRoot)

$script:passed = 0

function Assert-True {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) {
        throw "ASSERT FAILED: $Message"
    }
    $script:passed++
}

function Assert-Equal {
    param(
        [AllowNull()]$Expected,
        [AllowNull()]$Actual,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if ($Expected -ne $Actual) {
        throw "ASSERT FAILED: $Message (expected=$Expected actual=$Actual)"
    }
    $script:passed++
}

function New-TestPngBundle {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [int]$Width = 96,
        [int]$Height = 72
    )
    Add-Type -AssemblyName System.Drawing
    [void][System.IO.Directory]::CreateDirectory($Path)
    $names = @(
        'stand-0.png', 'stand-1.png', 'stand-2.png',
        'walk-0.png', 'walk-1.png', 'walk-2.png', 'walk-3.png',
        'climb-0.png', 'climb-1.png'
    )
    for ($i = 0; $i -lt $names.Count; $i++) {
        $bitmap = New-Object System.Drawing.Bitmap($Width, $Height)
        try {
            $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
            try {
                $graphics.Clear([System.Drawing.Color]::Transparent)
                $brush = New-Object System.Drawing.SolidBrush(
                    [System.Drawing.Color]::FromArgb(255, 40 + ($i * 15), 90 + ($i * 7), 140)
                )
                try {
                    $graphics.FillRectangle($brush, [Math]::Max(0, $Width - 22), [Math]::Max(0, $Height - 30), 16, 24)
                } finally {
                    $brush.Dispose()
                }
            } finally {
                $graphics.Dispose()
            }
            $bitmap.Save((Join-Path $Path $names[$i]), [System.Drawing.Imaging.ImageFormat]::Png)
        } finally {
            $bitmap.Dispose()
        }
    }
}

try {
    $first = New-MapleSkinSettings -Seed 4312
    $second = New-MapleSkinSettings -Seed 4312
    Assert-Equal 8 $first.assignments.Count 'new settings contain eight slots'
    Assert-Equal (($first.assignments.skinId) -join ',') (($second.assignments.skinId) -join ',') 'seeded random order is deterministic'
    Assert-Equal 8 (@($first.assignments.skinId | Sort-Object -Unique).Count) 'initial random order uses every built-in skin once'
    Assert-True (-not [bool]($first.assignments.locked -contains $true)) 'initial slots are unlocked'

    $first.assignments[2].locked = $true
    $first.assignments[2].skinId = 'builtin-7'
    $rerolled = Set-MapleRandomSkinAssignments -Settings $first -Seed 914 -RespectLocks
    Assert-Equal 'builtin-7' $rerolled.assignments[2].skinId 'reroll preserves a locked skin'
    Assert-True ([bool]$rerolled.assignments[2].locked) 'reroll preserves the lock state'

    $settingsPath = Join-Path $testRoot 'skin-settings.json'
    Save-MapleSkinSettings -Settings $rerolled -Path $settingsPath
    $roundTrip = Get-MapleSkinSettings -Path $settingsPath
    Assert-Equal (($rerolled.assignments.skinId) -join ',') (($roundTrip.assignments.skinId) -join ',') 'settings JSON round-trips assignments'
    Assert-Equal (($rerolled.assignments.locked) -join ',') (($roundTrip.assignments.locked) -join ',') 'settings JSON round-trips locks'

    $recoveryRoot = Join-Path $testRoot 'recovery-skins'
    $corruptSettingsPath = Join-Path $recoveryRoot 'skin-settings.json'
    [void][System.IO.Directory]::CreateDirectory($recoveryRoot)
    [System.IO.File]::WriteAllText($corruptSettingsPath, '{ broken json', (New-Object System.Text.UTF8Encoding($false)))
    $recoveredPack = Get-MapleActiveSkinPack -BasePack $basePack -SkinRoot $recoveryRoot -SettingsPath $corruptSettingsPath -ValidatorExe $validatorExe
    Assert-True (Test-Path -LiteralPath (Join-Path $recoveredPack 'pack.toml') -PathType Leaf) 'corrupt settings recover to a validated active pack'
    Assert-Equal 1 (@(Get-ChildItem -LiteralPath $recoveryRoot -File -Filter 'skin-settings.json.invalid-*').Count) 'corrupt settings are preserved before recovery'
    Assert-Equal 8 (@((Get-MapleSkinSettings -Path $corruptSettingsPath).assignments.skinId | Sort-Object -Unique).Count) 'recovery writes a valid non-repeating random roster'

    $skinRoot = Join-Path $testRoot 'skins'
    $previewRoot = Export-MapleBuiltinSkinPreviews -BasePack $basePack -SkinRoot $skinRoot
    Assert-Equal 8 (@(Get-ChildItem -LiteralPath $previewRoot -File -Filter 'builtin-*.png').Count) 'the workshop creates one preview for every built-in skin'
    $previewBitmap = New-Object System.Drawing.Bitmap((Join-Path $previewRoot 'builtin-0.png'))
    try {
        Assert-Equal 96 $previewBitmap.Width 'built-in preview retains the native paperdoll width'
        Assert-Equal 72 $previewBitmap.Height 'built-in preview retains the native paperdoll height'
    } finally {
        $previewBitmap.Dispose()
    }

    $validBundle = Join-Path $testRoot 'valid-bundle'
    New-TestPngBundle -Path $validBundle
    $imported = Import-MapleSkinFolder -SourceFolder $validBundle -SkinRoot $skinRoot -PackToml (Join-Path $basePack 'pack.toml')
    Assert-True ([bool]($imported.id -match '^user-[0-9a-f]{12}$')) 'import ID is content addressed'
    Assert-True (Test-Path -LiteralPath (Join-Path $imported.path 'stand-0.sprite') -PathType Leaf) 'import creates a stand sprite'
    Assert-True (Test-Path -LiteralPath (Join-Path $imported.path 'walk-3.sprite') -PathType Leaf) 'import creates every walk pose'
    Assert-True (Test-Path -LiteralPath (Join-Path $imported.path 'climb-1.sprite') -PathType Leaf) 'import creates every climb pose'

    $invalidBundle = Join-Path $testRoot 'invalid-bundle'
    New-TestPngBundle -Path $invalidBundle -Width 95
    $invalidRejected = $false
    try {
        Import-MapleSkinFolder -SourceFolder $invalidBundle -SkinRoot $skinRoot -PackToml (Join-Path $basePack 'pack.toml') | Out-Null
    } catch {
        $invalidRejected = $_.Exception.Message -match '96 x 72'
    }
    Assert-True $invalidRejected 'wrong-sized PNG bundle is rejected before publication'

    $resolvedSettings = New-MapleSkinSettings -Seed 1
    for ($slot = 0; $slot -lt 8; $slot++) {
        $resolvedSettings.assignments[$slot].skinId = 'builtin-{0}' -f (7 - $slot)
        $resolvedSettings.assignments[$slot].locked = $true
    }
    $generatedRoot = Join-Path $testRoot 'generated-skins'
    $baseHashBefore = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $basePack 'market_avatar_hires_7.sprite')).Hash
    $resolvedPack = New-MapleResolvedSkinPack -BasePack $basePack -SkinRoot $generatedRoot -Settings $resolvedSettings -ValidatorExe $validatorExe
    Assert-True (Test-Path -LiteralPath (Join-Path $resolvedPack 'pack.toml') -PathType Leaf) 'resolved pack is published only after validation'
    Assert-Equal $baseHashBefore ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $resolvedPack 'market_avatar_hires_0.sprite')).Hash) 'slot zero receives the selected built-in idle skin'
    Assert-Equal ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $basePack 'market_avatar_stand_hires_21.sprite')).Hash) ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $resolvedPack 'market_avatar_stand_hires_0.sprite')).Hash) 'stand animation follows the same selected skin'
    Assert-Equal ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $basePack 'market_avatar_walk_hires_28.sprite')).Hash) ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $resolvedPack 'market_avatar_walk_hires_0.sprite')).Hash) 'walk animation follows the same selected skin'
    Assert-Equal ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $basePack 'market_avatar_climb_hires_14.sprite')).Hash) ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $resolvedPack 'market_avatar_climb_hires_0.sprite')).Hash) 'climb animation follows the same selected skin'
    Assert-Equal ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $basePack 'market_avatar_stand2_hires_21.sprite')).Hash) ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $resolvedPack 'market_avatar_stand2_hires_0.sprite')).Hash) 'stand2 status animation follows the same selected skin'
    Assert-Equal ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $basePack 'market_avatar_sit_hires_7.sprite')).Hash) ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $resolvedPack 'market_avatar_sit_hires_0.sprite')).Hash) 'sit status pose follows the same selected skin'
    Assert-Equal ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $basePack 'market_avatar_alert_hires_21.sprite')).Hash) ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $resolvedPack 'market_avatar_alert_hires_0.sprite')).Hash) 'alert status animation follows the same selected skin'
    Assert-Equal ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $basePack 'training_avatar_attack_hires_21.sprite')).Hash) ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $resolvedPack 'training_avatar_attack_hires_0.sprite')).Hash) 'attack animation follows the same selected built-in skin'
    Assert-Equal ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $basePack 'training_skill_magic_claw_0.sprite')).Hash) ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $resolvedPack 'training_skill_magic_claw_0.sprite')).Hash) 'skill effect remains an independent shared layer while paperdolls are remapped'
    Assert-Equal $baseHashBefore ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $basePack 'market_avatar_hires_7.sprite')).Hash) 'building a private pack does not mutate the base pack'

    $resolvedSettings.assignments[0].skinId = $imported.id
    $customResolved = New-MapleResolvedSkinPack -BasePack $basePack -SkinRoot $skinRoot -Settings $resolvedSettings -ValidatorExe $validatorExe
    Assert-Equal ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $imported.path 'stand-0.sprite')).Hash) ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $customResolved 'market_avatar_stand_hires_0.sprite')).Hash) 'custom stand pose is mapped into the selected slot'
    Assert-Equal ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $imported.path 'walk-3.sprite')).Hash) ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $customResolved 'market_avatar_walk_hires_3.sprite')).Hash) 'custom walk cycle stays attached to the same slot'
    Assert-Equal ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $imported.path 'climb-1.sprite')).Hash) ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $customResolved 'market_avatar_climb_hires_1.sprite')).Hash) 'custom climb cycle stays attached to the same slot'
    Assert-Equal ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $imported.path 'stand-2.sprite')).Hash) ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $customResolved 'market_avatar_stand2_hires_2.sprite')).Hash) 'nine-frame custom skins fall back to their own stand1 frame for stand2'
    Assert-Equal ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $imported.path 'stand-0.sprite')).Hash) ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $customResolved 'market_avatar_sit_hires_0.sprite')).Hash) 'nine-frame custom skins fall back to their own stand1 frame for sit'
    Assert-Equal ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $imported.path 'stand-1.sprite')).Hash) ((Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $customResolved 'market_avatar_alert_hires_1.sprite')).Hash) 'nine-frame custom skins fall back to their own stand1 frame for alert'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $customResolved 'training_avatar_attack_hires_0.sprite') -PathType Leaf)) 'a custom skin disables the optional attack body instead of inheriting another built-in'
    Assert-True (-not ([System.IO.File]::ReadAllText((Join-Path $customResolved 'pack.toml')).Contains('[animations.training_avatar_attack_hires]'))) 'a custom pack manifest routes attack to the identity-preserving runtime fallback'

    $workshopPreview = Join-Path $testRoot 'workshop-preview.png'
    $previewResult = Show-MapleSkinWorkshop -WorkshopRequest ([pscustomobject]@{
        BasePack = $basePack
        SkinRoot = $skinRoot
        SettingsPath = $settingsPath
        ValidatorExe = $validatorExe
        PreviewPath = $workshopPreview
    })
    Assert-True (-not [bool]$previewResult) 'preview mode does not report a user apply action'
    Assert-True ((Test-Path -LiteralPath $workshopPreview -PathType Leaf) -and (Get-Item -LiteralPath $workshopPreview).Length -gt 20000) 'the real WinForms workshop renders a non-empty visual preview'

    Write-Host "PASS: $script:passed assertions" -ForegroundColor Green
} finally {
    if (Test-Path -LiteralPath $testRoot -PathType Container) {
        $resolvedTemp = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
        $resolvedTest = [System.IO.Path]::GetFullPath($testRoot)
        if (-not $resolvedTest.StartsWith($resolvedTemp, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw 'Refusing to remove a test directory outside the OS temp root.'
        }
        Remove-Item -LiteralPath $resolvedTest -Recurse -Force
    }
}
