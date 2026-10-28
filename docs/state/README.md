# 状態遷移

コードにある状態機械を Mermaid の `stateDiagram-v2` で書いた。名前は実装の識別子に合わせている。

| 図 | 対応するコード |
| --- | --- |
| [fetch-lifecycle](fetch-lifecycle/diagram.mmd) | `LoopState` の `due` / `succeed` / `fail` / `kick` と `backoff_delay` |
| [alert-latch](alert-latch/diagram.mmd) | `crossing_alerts` と `Shared.latches` |
| [tray-window](tray-window/diagram.mmd) | ウィンドウ `main`、`reveal`、`CloseRequested`、UI の `tab` |

oh-my-mermaid の要素は `description.md` と `diagram.mmd` の組です。`omm` は図を生成しないので、ここは手で書いています。
