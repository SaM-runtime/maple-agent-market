# Fork 與非官方聲明

Maple Agent Market 是以 [Pixtuoid](https://github.com/IvanWng97/pixtuoid) 為基礎開發的非官方 agents orchestration visualizer。

Fork 基準為 Pixtuoid `v0.16.0` commit [`ac06cc00c3cf18f3f67eab730a37f0c7e5787fc8`](https://github.com/IvanWng97/pixtuoid/commit/ac06cc00c3cf18f3f67eab730a37f0c7e5787fc8)。本 repo 保留 Pixtuoid 原作者 Ivan Wang 的 MIT 版權與授權聲明；Maple Agent Market 貢獻者的聲明只涵蓋後續修改，不取代上游權利。

本專案未受 NEXON、遊戲橘子、OpenAI、Pixtuoid 維護者或其他第三方贊助、認可、核准或背書。MapleStory、NEXON 與相關名稱、角色及標誌屬其各自權利人。

## 不屬於 MIT repo 的本機素材

下列檔案不能被本專案的 MIT License 覆蓋，也不會隨 source、release、binary 或文件截圖發布：

- NEXON ranking／Open API 紙娃娃及生成的 `.sprite`；
- 自由市場、弓箭手村等遊戲截圖、重建或 WZ／client 衍生背景；
- NEXON／MapleStory 商店框、角色、怪物、傳送點、動作與技能影格；
- MapleStory 音樂、YouTube 下載內容與包含上述素材的 preview／QA 截圖；
- 使用者提供的 skin、cache、backup 與 active pack。

這些內容若供個人本機評估，只能留在 repo 外的 `private-assets` 或自訂 pack 位置。其暫存日期、來源 manifest 與 refresh／delete 規則由本機 pack 自己管理，不可加入公開 Git 歷史或 release artifact。

## 可公開內容

公開 repo 可包含：

- Pixtuoid MIT 原始碼與預設素材；
- Maple Agent Market 的原創程式碼、UI、狀態映射與程式化特效；
- Monaspace Neon OFL 字型；
- sprite pack 格式、驗證器與本機 skin workshop 程式。

使用者可在自己電腦以 `--pack-dir` 載入有權使用的素材；載入行為不會把該素材改成 MIT，也不代表本專案取得其再散布權。

## 本機 BGM

程式可循環播放使用者指定的 MP3、WAV、OGG 或 FLAC。專案不含 YouTube downloader、音訊擷取器或隱藏 web player，也不會把該音樂自動加入 Git。使用者須自行確認檔案的使用權。
