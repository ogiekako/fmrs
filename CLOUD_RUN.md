# Cloud Run 構成メモ

`fmrs` は基本的に GitHub Pages 上の wasm だけで動きますが、長手数やメモリ消費の大きい問題向けに、`/solve` API だけを Cloud Run へ逃がす構成を取れます。

## 構成

- フロントエンド: GitHub Pages (`https://ogiekako.github.io/fmrs`)
- API: Cloud Run 上の `fmrs server`
- 通信:
  - `GET /fmrs_alive`
  - `POST /solve?solutions_upto=N` (協力詰)
  - `POST /solve_futsuu` (普通詰将棋: 既定でハッシュ 2048MB・上限 600 秒・並列スレッド数は CPU 数に自動追従)
- フロントは `FMRS_API_BASE_URL` が設定されていれば Cloud Run を優先し、未設定なら wasm にフォールバックします

## このリポジトリに入っているもの

- `Dockerfile`
  - Rust バイナリをビルドして Cloud Run に載せる
- `rust/scripts/deploy-cloud-run.sh`
  - `gcloud run deploy --source .` をラップする最小スクリプト
- CORS 対応
  - `FMRS_ALLOWED_ORIGINS` で許可 origin を指定

## 初回だけ手でやること

1. Google Cloud プロジェクトを作る
2. `gcloud` を入れてログインする
3. Cloud Run / Cloud Build / Artifact Registry の API を有効化する
4. 対象 project を `gcloud config set project ...` で選ぶ

例:

```bash
gcloud auth login
gcloud config set project YOUR_PROJECT_ID
gcloud services enable run.googleapis.com cloudbuild.googleapis.com artifactregistry.googleapis.com
```

## GitHub Actions で自動デプロイする場合

`.github/workflows/cloud-run.yaml` を入れてあります。`main` への push で、`rust/` や `Dockerfile` に変更があれば Cloud Run を再デプロイします。
この workflow は API 有効化までは行いません。`run.googleapis.com`, `cloudbuild.googleapis.com`, `artifactregistry.googleapis.com` は初回だけ手動で有効化しておく前提です。

事前に GitHub repository secret `GCP_SA_KEY` を設定してください。値は Cloud Run と Cloud Build を実行できる service account の JSON キーです。

最低限必要な権限の目安:

- `roles/run.admin`
- `roles/cloudbuild.builds.editor`
- `roles/artifactregistry.admin`
- `roles/iam.serviceAccountUser`

より厳密には、既存の運用ルールに合わせて絞ってください。

## デプロイ

デフォルトは `asia-northeast1`、サービス名は `fmrs-api` です。

```bash
./rust/scripts/deploy-cloud-run.sh
```

既定では `CPU=4`, `MEMORY=8Gi`, `MAX_INSTANCES=2` です。必要なら環境変数で調整できます。

```bash
SERVICE_NAME=fmrs-api \
REGION=asia-northeast1 \
ALLOWED_ORIGINS=https://ogiekako.github.io \
MEMORY=8Gi \
CPU=4 \
TIMEOUT=3600 \
MAX_INSTANCES=2 \
./rust/scripts/deploy-cloud-run.sh
```

## GitHub Pages 側の接続先

Cloud Run の URL が `https://fmrs-api-xxxxx-an.a.run.app` だった場合、Pages ビルド時に以下を指定します。

```bash
FMRS_API_BASE_URL=https://fmrs-api-xxxxx-an.a.run.app npm run build
```

これでフロントは Cloud Run を優先して使います。未設定なら従来通りブラウザ wasm で解きます。

## 推奨設定

- `concurrency=1`
  - 重い探索同士を 1 インスタンスに同居させない (1 件で 8Gi を使い切ることがあり、
    同居させると両方メモリ不足で落ちる)
- `max-instances=2`
  - 1 件の長い解図が唯一のインスタンスを占有して、他の利用者が全員 wasm に回されるのを避けつつ、4 vCPU 化に伴う最悪課金上限を抑える
  - 課金の上限はインスタンス数に比例する。4 vCPU / 8Gi はリクエスト課金で
    約 $0.418/時間/インスタンスなので、休みなく叩かれ続けた最悪でも 2 台で月 $600 程度。
    無料枠（月 180,000 vCPU秒・360,000 GiB秒）は 4 vCPU / 8Gi でちょうど両方 12.5 時間/月に一致するため、
    実利用（月 100 件弱・合計数分）は無料枠に収まる。
    認証なしの公開 API なので Cloud Billing の予算アラートを併用する
- `timeout=3600`
  - Cloud Run の上限まで使う（普通詰将棋 `/solve_futsuu` は既定で `FMRS_FUTSUU_TIME_LIMIT_SECS=600` 秒の上限を独立設定）
- `memory=8Gi`
  - ブラウザの 4GB 制限を超える問題を受ける最小ライン（普通詰将棋の既定ハッシュは `FMRS_FUTSUU_HASH_MB=2048`）
- `cpu=4`
  - 普通詰将棋の 4 スレッド並列解図を活かしつつ、8GiB メモリの無料枠比率 (1:2) と一致させる

## 制約

- Cloud Run の HTTP リクエストは最大 60 分
- 4 件目以降の同時リクエストはインスタンスの空き待ちになり、待ちきれないと 429 になる
  (フロントは wasm にフォールバックする)
- Cloud Run 経由では、クライアントの切断 (キャンセル・タブを閉じる) がコンテナに
  伝わらない (2026-11-11 に確認: 切断後もリクエストは処理中のまま)。解図は完了か
  タイムアウトまで続き、その間インスタンスを 1 台占有し課金も続く。
  サーバーは進捗の書き込みが失敗した時点で解図を打ち切るが、これが効くのは
  直結 (ローカル) やタイムアウトで接続が閉じられた場合
- 重い問題ではメモリ不足の可能性がある
- GitHub Pages にはリバースプロキシ機能がないため、API の絶対 URL をビルド時に埋め込む必要があります
