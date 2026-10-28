# アーキテクチャ図

`architecture.d2` がソースです。GitHub は `.d2` を描かないので、ライト用とダーク用の SVG も同じディレクトリにコミットします。

描き直し（d2 は CI に入っていません）:

```bash
python ~/.claude/skills/arch-diagram/versions/v1.1/scripts/render.py docs/architecture/architecture.d2 --preview-dir /tmp/arch-preview
```

`render.py` は PATH に d2 が無ければ `~/.cache/arch-diagram/` へ取得します。`__MONO__` はライトで `000000`、ダークで `ffffff` に置き換わります。SVG に外部 URL が残っていると失敗します。

確認用 PNG を見て、線の交差とダークテーマで消えるロゴが無いかを見てから SVG をコミットします。
