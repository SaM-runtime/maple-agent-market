# Maple Agent Market 開發指令。
default:
    @just --list

# 編譯使用者執行檔 target/debug/maple-agent-market(.exe)。
build:
    cargo build --locked -p pixtuoid

# 直接開啟內建程序化 Maple 雙地圖視窗。
run:
    cargo run --locked -p pixtuoid

# 列出已由使用者匯入的本機素材包；乾淨安裝為空是正常狀態。
assets-list:
    cargo run --locked -p pixtuoid -- assets list

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

check:
    cargo check --locked --workspace --all-targets

clippy:
    cargo clippy --locked --workspace --all-targets -- -D warnings

test:
    cargo test --locked --workspace

# 產生 README 內建公開安全畫面，不載入本機 pack。
snapshot output="assets/readme/maple-agent-market-overview.png":
    cargo run --locked --release -p pixtuoid --example floating_snapshot -- "{{output}}" 1440x600 --map both --agents 4 --label-px 15 --command-ms 700 --complete-ms 700

audit:
    python scripts/public-release-audit.py --selftest
    python scripts/public-release-audit.py

preflight: fmt-check check clippy test audit

release-build:
    python scripts/build-public-release.py
