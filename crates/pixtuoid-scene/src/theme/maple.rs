use pixtuoid_core::sprite::Rgb;

use super::*;

/// Warm palette for the removable local Free Market variant.
///
/// The custom pack supplies the complete scene plate; these colors keep
/// characters, status tones, tool glows, fallback geometry, and UI overlays
/// readable without coupling the shared theme model to that local asset.
pub static MAPLE: Theme = Theme {
    name: "maple",
    kind: ThemeKind::Light,
    surface: SurfaceColors {
        wall: Rgb {
            r: 77,
            g: 52,
            b: 39,
        },
        wall_trim: Rgb {
            r: 122,
            g: 81,
            b: 50,
        },
        carpet_base: Rgb {
            r: 124,
            g: 164,
            b: 75,
        },
        carpet_light: Rgb {
            r: 159,
            g: 196,
            b: 91,
        },
        carpet_dark: Rgb {
            r: 83,
            g: 120,
            b: 58,
        },
        window_frame: Rgb {
            r: 61,
            g: 44,
            b: 37,
        },
        bg_fallback: Rgb {
            r: 21,
            g: 44,
            b: 54,
        },
    },
    office: OfficeColors {
        room_wall_trim_light: Rgb {
            r: 209,
            g: 164,
            b: 95,
        },
        room_wall_trim_dark: Rgb {
            r: 93,
            g: 59,
            b: 37,
        },
        cubicle_divider: Rgb {
            r: 138,
            g: 90,
            b: 50,
        },
        runner_base: Rgb {
            r: 199,
            g: 137,
            b: 67,
        },
        runner_stripe: Rgb {
            r: 227,
            g: 179,
            b: 90,
        },
        runner_edge: Rgb {
            r: 114,
            g: 65,
            b: 38,
        },
        neon_panel_bg: Rgb {
            r: 27,
            g: 43,
            b: 37,
        },
        neon_frame_base: Rgb {
            r: 199,
            g: 157,
            b: 62,
        },
        building_dark: Rgb {
            r: 36,
            g: 79,
            b: 58,
        },
        building_light: Rgb {
            r: 63,
            g: 118,
            b: 80,
        },
        city_lit_windows: [
            Rgb {
                r: 255,
                g: 220,
                b: 112,
            },
            Rgb {
                r: 121,
                g: 232,
                b: 195,
            },
            Rgb {
                r: 255,
                g: 147,
                b: 94,
            },
        ],
        city_dark_window: Rgb {
            r: 24,
            g: 51,
            b: 39,
        },
        clock_rim: Rgb {
            r: 218,
            g: 176,
            b: 86,
        },
        clock_face: Rgb {
            r: 255,
            g: 246,
            b: 207,
        },
        clock_hand: Rgb {
            r: 63,
            g: 42,
            b: 28,
        },
        shadow: Rgb {
            r: 45,
            g: 31,
            b: 22,
        },
    },
    lighting: LightingColors {
        day_sky_a: Rgb {
            r: 104,
            g: 183,
            b: 214,
        },
        day_sky_b: Rgb {
            r: 177,
            g: 222,
            b: 232,
        },
        night_sky_a: Rgb {
            r: 20,
            g: 43,
            b: 68,
        },
        night_sky_b: Rgb {
            r: 39,
            g: 65,
            b: 91,
        },
        twilight_a: Rgb {
            r: 242,
            g: 134,
            b: 72,
        },
        twilight_b: Rgb {
            r: 255,
            g: 190,
            b: 104,
        },
        sun_spill: Rgb {
            r: 255,
            g: 224,
            b: 136,
        },
        ceiling_pool: Rgb {
            r: 255,
            g: 239,
            b: 184,
        },
        floor_lamp_halo: Rgb {
            r: 255,
            g: 190,
            b: 92,
        },
        night_tint: Rgb {
            r: 15,
            g: 32,
            b: 43,
        },
        // Matches the pre-Task-7 placeholder consts (visually approved) —
        // pinned by task-7's brief so the already-verified look survives.
        sun_core: Rgb {
            r: 255,
            g: 230,
            b: 134,
        },
        moon_core: Rgb {
            r: 210,
            g: 240,
            b: 255,
        },
    },
    furniture: FurnitureColors {
        wood_top: Rgb {
            r: 166,
            g: 100,
            b: 50,
        },
        wood_trim: Rgb {
            r: 91,
            g: 52,
            b: 32,
        },
        rug_field: Rgb {
            r: 132,
            g: 66,
            b: 45,
        },
        rug_trim: Rgb {
            r: 76,
            g: 44,
            b: 31,
        },
        rug_accent: Rgb {
            r: 230,
            g: 177,
            b: 74,
        },
        magazine: Rgb {
            r: 89,
            g: 139,
            b: 91,
        },
        magazine_trim: Rgb {
            r: 48,
            g: 78,
            b: 53,
        },
        chair_trim: Rgb {
            r: 83,
            g: 48,
            b: 29,
        },
        coffee_cup: Rgb {
            r: 237,
            g: 200,
            b: 116,
        },
        coffee_cup_shadow: Rgb {
            r: 192,
            g: 131,
            b: 73,
        },
        tank_water: Rgb {
            r: 0x2f,
            g: 0x86,
            b: 0x87,
        },
        tank_water_line: Rgb {
            r: 0x82,
            g: 0xeb,
            b: 0xd5,
        },
        tank_fish: Rgb {
            r: 0xf2,
            g: 0xa2,
            b: 0x37,
        },
        tank_fish_alt: Rgb {
            // NOT the lobster carapace reds (#d2402f/#e85540/#c83828/#9e2a20):
            // the mascot harness counts those RGBs as exclusive presence proof.
            r: 0xe8,
            g: 0x66,
            b: 0x32,
        },
        tank_plant: Rgb {
            r: 0x4f,
            g: 0xa3,
            b: 0x4e,
        },
        paper: Rgb {
            r: 0xff,
            g: 0xf3,
            b: 0xc6,
        },
        paper_shade: Rgb {
            r: 0xd8,
            g: 0xc3,
            b: 0x8c,
        },
    },
    effects: EffectColors {
        monitor_frame_lit: Rgb {
            r: 232,
            g: 199,
            b: 92,
        },
        sleep_z: Rgb {
            r: 177,
            g: 167,
            b: 214,
        },
        coffee_steam: Rgb {
            r: 217,
            g: 238,
            b: 224,
        },
        walking_dust: Rgb {
            r: 191,
            g: 147,
            b: 81,
        },
        waiting_bubble: Rgb {
            r: 255,
            g: 207,
            b: 68,
        },
    },
    tool_glow: ToolGlowColors {
        edit: Rgb {
            r: 100,
            g: 160,
            b: 255,
        },
        read: Rgb {
            r: 80,
            g: 220,
            b: 240,
        },
        bash: Rgb {
            r: 240,
            g: 170,
            b: 80,
        },
        agent: Rgb {
            r: 200,
            g: 140,
            b: 255,
        },
        grep: Rgb {
            r: 180,
            g: 220,
            b: 120,
        },
        default: Rgb {
            r: 140,
            g: 240,
            b: 170,
        },
    },
    ui: UiColors {
        label_active: Rgb {
            r: 92,
            g: 226,
            b: 102,
        },
        label_waiting: Rgb {
            r: 255,
            g: 210,
            b: 72,
        },
        label_idle: Rgb {
            r: 214,
            g: 199,
            b: 154,
        },
        label_exiting: Rgb {
            r: 126,
            g: 105,
            b: 112,
        },
        tooltip_bg: Rgb {
            r: 35,
            g: 29,
            b: 24,
        },
        tooltip_title: Rgb {
            r: 255,
            g: 246,
            b: 211,
        },
        tooltip_text: Rgb {
            r: 232,
            g: 218,
            b: 178,
        },
        tooltip_dim: Rgb {
            r: 175,
            g: 152,
            b: 119,
        },
        neon_brand: Rgb {
            r: 107,
            g: 228,
            b: 134,
        },
        neon_star: Rgb {
            r: 255,
            g: 190,
            b: 72,
        },
    },
    appliance: ApplianceColors {
        vending_body: Rgb {
            r: 71,
            g: 47,
            b: 31,
        },
        vending_panel: Rgb {
            r: 79,
            g: 151,
            b: 82,
        },
        vending_drinks: [
            Rgb {
                r: 222,
                g: 74,
                b: 67,
            },
            Rgb {
                r: 111,
                g: 207,
                b: 92,
            },
            Rgb {
                r: 92,
                g: 144,
                b: 224,
            },
            Rgb {
                r: 245,
                g: 188,
                b: 62,
            },
        ],
        vending_trim: Rgb {
            r: 221,
            g: 173,
            b: 73,
        },
        vending_dark: Rgb {
            r: 42,
            g: 31,
            b: 25,
        },
        printer_body: Rgb {
            r: 205,
            g: 180,
            b: 130,
        },
        printer_top: Rgb {
            r: 93,
            g: 60,
            b: 38,
        },
        printer_glass: Rgb {
            r: 100,
            g: 194,
            b: 178,
        },
        printer_paper: Rgb {
            r: 255,
            g: 245,
            b: 210,
        },
        printer_tray: Rgb {
            r: 157,
            g: 117,
            b: 77,
        },
        coats: [
            Rgb {
                r: 191,
                g: 73,
                b: 52,
            },
            Rgb {
                r: 67,
                g: 116,
                b: 82,
            },
            Rgb {
                r: 222,
                g: 180,
                b: 74,
            },
        ],
    },
    source: SourceColors {
        claude_code: Rgb {
            r: 0xc8,
            g: 0x6e,
            b: 0x12,
        }, // amber
        codex: Rgb {
            r: 0x1e,
            g: 0x80,
            b: 0xc0,
        }, // blue
        reasonix: Rgb {
            r: 0x9c,
            g: 0x3c,
            b: 0xc0,
        }, // violet
        antigravity: Rgb {
            r: 0x2e,
            g: 0x9e,
            b: 0x4a,
        }, // green
        codewhale: Rgb {
            r: 0x14,
            g: 0xb8,
            b: 0xb0,
        }, // teal
        opencode: Rgb {
            r: 0xd8,
            g: 0x3a,
            b: 0x3a,
        }, // red
        copilot: Rgb {
            r: 0xe0,
            g: 0x60,
            b: 0x9c,
        }, // copilot rose
        cursor: Rgb {
            r: 0x96,
            g: 0xa0,
            b: 0xaf,
        }, // cursor slate-blue (monochrome brand; distinct from all 7)
        openclaw: Rgb {
            r: 0xff,
            g: 0xaa,
            b: 0x30,
        }, // openclaw marigold (lobster; warm, clears claude-amber + opencode-red)
        hermes: Rgb {
            r: 0x68,
            g: 0x58,
            b: 0xe0,
        }, // Hermes/Nous indigo (brand-constant like openclaw marigold; clears reasonix-violet + cursor-slate + codex-blue on every theme, min Manhattan >=92)
        omp: Rgb {
            r: 0x8a,
            g: 0xc8,
            b: 0x32,
        }, // omp lime (brand-constant; clears antigravity-green + codewhale-teal + claude-amber on every theme)
        grok: Rgb {
            r: 0x30,
            g: 0xd0,
            b: 0xe8,
        }, // grok cyan (brand-constant; nearest neighbor codewhale-teal at Manhattan 78 ≥ the 60 floor, and the SITE's chip transforms clear WCAG AA in both themes: ×0.55-on-day-chip 5.3:1, ×0.8+white-on-night-chip 11.6:1 — the silver e6e6e6 failed the day chip at 3.83:1, caught by the site e2e badge sweep)
        kimi: Rgb {
            r: 0xc4,
            g: 0x46,
            b: 0xfe,
        }, // kimi electric violet (brand-constant like hermes/omp/grok; Kimi/Moonshot's purple, min Manhattan >=88 vs every source on every theme)
    },
};
