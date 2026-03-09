# shogi-ai

Rust ベースの将棋 AI エンジンです。Phase1 では BFF から呼ばれる最小推論 API を実装しています。

## Run

```bash
cargo run
```

デフォルト待受:

- `0.0.0.0:8080`

環境変数:

- `AI_ENGINE_BIND` (例: `127.0.0.1:8080`)

## API

- `GET /health`
- `POST /v1/ai/move`

契約定義:

- `docs/phase1-contract.md`
