# Maple Agent Market 的常用開發指令。所有命令都可直接改用下方對應工具執行。

default:
    @just --list

# 編譯主程式。
build:
    cargo build --locked -p pixtuoid

# 啟動 Maple 浮動視窗。
run:
    cargo run --locked -p pixtuoid -- --theme maple floating

# 套用 Rust 格式。
fmt:
    cargo fmt --all

# 驗證格式但不修改檔案。
fmt-check:
    cargo fmt --all -- --check

# 檢查整個 workspace。
check:
    cargo check --locked --workspace --all-targets

# 將所有 Clippy 警告視為錯誤。
clippy:
    cargo clippy --locked --workspace --all-targets -- -D warnings

# 執行整個 workspace 的測試。
test:
    cargo test --locked --workspace

# 重拍 README 的公開安全版雙地圖畫面。
snapshot output="assets/readme/maple-agent-market-overview.png":
    cargo run --locked --release -p pixtuoid --example floating_snapshot -- "{{output}}" 1440 600 --theme maple --map both --agents 4 --label-px 15 --command-ms 700 --complete-ms 700

# 稽核 repo 中的素材、隱私與可疑產物。
audit:
    python scripts/public-release-audit.py --selftest
    python scripts/public-release-audit.py

# 提交前的完整本機 gate。
preflight: fmt-check check clippy test audit

# 建立經稽核的公開 release binary。
release-build:
    python scripts/build-public-release.py
