# CHD312 Windows 11 24H2+ Fix

CLASSIC PRO CHD312 の音声が Windows 11 で USB MIDI ドライバーに誤って割り当てられる問題を回避する、非公式の Rust 製ツールです。

USB 接続のイベントを受け取り、対象の構成が一致し、**現在のドライバーが `usbmidi2.inf` の場合だけ**、Windows 標準の `wdma_usb.inf` / `usbaudio.sys` に差し替えます。正常な USB Audio や別ドライバーは置き換えません。ファームウェアは変更しません。

## ダウンロード

[Releases](https://github.com/ryuya0124/chd312-win11-24h2-fix/releases) から、どちらかを選んでください。Windows x64 向けです。

| 配布物 | 使い方 |
|---|---|
| `chd312-portable-x64.exe` | 1 ファイルをダウンロードし、ターミナルから実行。インストール不要 |
| `chd312-setup-x64.exe` | Program Files への配置、自動起動タスク、スタートメニュー、アンインストーラーを登録 |
| `SHA256SUMS.txt` | 配布ファイルの SHA-256 |

単体版も常駐時に画面を出しません。常駐には、EXE に内蔵したバックグラウンド用モジュールをユーザーのローカルキャッシュに展開します。単体版では `start` の際に管理者権限の確認が出る場合があります。自動起動タスクや PATH は登録しません。

インストーラー版は初回に管理者権限が必要です。サインインの約5秒後、タスクスケジューラから画面を出さずに監視を開始します。空のターミナルも表示しません。以降の差し直しやインスタンス ID の変更は自動で再照合します。

## コマンド

インストール後、新しく開いたターミナルで使えます。既存ターミナルでは `"C:\Program Files\CHD312 Windows 11 Fix\chd312.exe"` をフルパスで実行してください。PowerShell ではパスの前に `&` を付けます。

```powershell
chd312 status
chd312 inspect
chd312 apply
chd312 apply --force
chd312 disable
chd312 enable
chd312 stop
chd312 start
chd312 logs
chd312 uninstall
```

| コマンド | 動作 |
|---|---|
| `status [--json]` | 常駐状態、照合結果、各音声ドライバーを確認。常駐していなくても読み取り可能 |
| `inspect [--json]` | 14条件の詳細を読み取り表示。ドライバーを変更しない |
| `apply` | 今すぐ確認し、MIDI に割り当てられた音声だけ修復 |
| `apply --force` | 自動修復が一時停止中でも、今回だけ確認・適用 |
| `disable` | 自動修復を一時停止。状態は再起動後も保持。現在の音声ドライバーは維持 |
| `enable` | 自動修復を再開し、接続中の機器も確認 |
| `stop` / `start` | 常駐プロセスを終了／起動。`stop` だけなら次回サインイン時は再び起動 |
| `logs` | ログのフォルダーを開く |
| `uninstall` | インストーラー版のアンインストーラーを起動 |
| `restore-original` | 自動修復を一時停止し、既知の元の USB MIDI 割り当てへ戻す。**音声が使えなくなる可能性あり** |

`--force` でも14条件を省略せず、すでに USB Audio の機能や未知のドライバーは差し替えません。デバイスの再起動や Windows の自動再起動も行いません。

## インストールと削除

インストール／アンインストールは**同じセットアップ EXE 1本**で扱います。インストール済みの PC で再実行すると、更新か削除を選択できます。配置される `Uninstall.exe` も同じ EXE のコピーです。

配置先は `C:\Program Files\CHD312 Windows 11 Fix` です。

スタートメニューの「CHD312 Windows 11 24H2+ Fix」には、監視を開始するショートカット、使い方、アンインストーラーを配置します。常駐開始のショートカットも画面を出しません。タスクマネージャーのスタートアップへの登録は行いません。

「設定 → アプリ → インストールされているアプリ」、スタートメニュー、または `chd312 uninstall` から削除できます。削除時は監視停止、タスク・ショートカット・このツールの PATH 登録・アプリ登録・実行ファイルを削除します。**修復済みの Windows 標準音声ドライバーとログは保持**します。

ログ・一時停止設定は `%LOCALAPPDATA%\CHD312Win11Fix` に保存します。ログは2MBごとにローテーションし、過去3世代を保持します。

## 対象の判定

以下をすべて必須にします。

- VID/PID `04B4:00F8`、`REV_0000`、既知の18バイトのデバイス記述子
- 1,436バイトの構成記述子全体の SHA-256
- 既知の22バイトの BOS 記述子と5件の USB 文字列
- 同じ USB 親に属する映像 `MI_00`、アナログ音声 `MI_02`、HID `MI_04`、デジタル音声 `MI_05`
- 各機能の名前・種類、音声 IAD と互換 ID

構成 SHA-256:

```text
E235F83B29245CF7272F1F733000F2B36A76E48B88A1160F8F42F1CDB1ED33A7
```

音声機能の差し替え直前にも、構成全体と親子関係を再取得して照合します。接続通知は短時間まとめ、機能がそろうまでの再確認は回数を制限します。修復失敗時は同じ接続に対して2分間に3回を上限とします。

これは販売モデル名や個体の一意性を保証する識別ではありません。同じ構成を返す OEM 別製品との区別は保証できません。また、USB 2.0 接続や別のファームウェア版が異なる構成を返した場合は修復を拒否します。

## 原因と確認範囲

調査した実機は、音声 IAD に `Class 01 / SubClass 03` を返し、実際の音声インターフェースは AudioControl / AudioStreaming でした。この互換 ID に新しい MIDI ドライバーが一致し、音声機能が開始できなくなっていました。標準 USB Audio への変更で映像と HDMI 音声の取り込みを確認しています。

[販売元は Windows 11 24H2 以降を非対応と案内](https://www.soundhouse.co.jp/products/detail/item/281686/)しています。このツールはメーカー公式の対応を意味しません。開発時の実機 OS は Windows 11 **26H2 / build 26300.9711**。24H2・25H2の全ビルド、他の個体、OS 再起動後の挙動は、それぞれの環境での検証が必要です。USB ID が変わった接続でも構成が一致すれば再判定できます。

参考:

- [Microsoft: USB interface collections と互換 ID](https://learn.microsoft.com/en-us/windows-hardware/drivers/usbcon/support-for-interface-collections)
- [Microsoft: MIDI class driver の切り替え](https://microsoft.github.io/MIDI/kb/how-to-switch-between-class-drivers/)
- [Microsoft: デバイス接続通知](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/registering-for-notification-of-device-interface-arrival-and-device-removal)

## ビルド

Windows x64、Rust MSVC、Visual Studio Build Tools の C++ ツールチェーンが必要です。.NET / WinUI は使いません。

```powershell
./scripts/build.ps1
```

単体版とインストーラーを `dist` に生成します。`cargo test --locked` で判定の正例・負例、ドライバー状態の制限を検証できます。GitHub Actions も同じビルド手順を実行します。

MIT License。
