# Cloud Run 構成メモ

`fmrs` は基本的に GitHub Pages 上の wasm だけで動きますが、長手数やメモリ消費の大きい問題向けに、`/solve` API だけを Cloud Run へ逃がす構成を取れます。

## 構成

- フロントエンド: GitHub Pages (`https://ogiekako.github.io/fmrs`)
- API: Cloud Run 上の `fmrs server`
- 通信:
  - `GET /fmrs_alive`
  - `POST /solve?solutions_upto=N`
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

既定では `CPU=2`, `MEMORY=8Gi`, `MAX_INSTANCES=3` です。必要なら環境変数で調整できます。

```bash
SERVICE_NAME=fmrs-api \
REGION=asia-northeast1 \
ALLOWED_ORIGINS=https://ogiekako.github.io \
MEMORY=8Gi \
CPU=2 \
TIMEOUT=3600 \
MAX_INSTANCES=3 \
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
- `max-instances=3`
  - 1 件の長い解図 (最大 60 分) が唯一のインスタンスを占有して、他の利用者が
    全員 wasm に回されるのを避ける
  - 課金の上限はインスタンス数に比例する。2 vCPU / 8Gi はリクエスト課金で
    約 $0.245/時間/インスタンスなので、休みなく叩かれ続けた最悪でも 3 台で月 $540 程度。
    実利用 (2026-11 時点で月 100 件弱・合計数分) は無料枠に収まる。
    認証なしの公開 API なので Cloud Billing の予算アラートを併用する
- `timeout=3600`
  - Cloud Run の上限まで使う
- `memory=8Gi`
  - ブラウザの 4GB 制限を超える問題を受ける最小ライン
- `cpu=2`
  - `8Gi` を使うために必要

## 制約

- Cloud Run の HTTP リクエストは最大 60 分
- 4 件目以降の同時リクエストはインスタンスの空き待ちになり、待ちきれないと 429 になる
  (フロントは wasm にフォールバックする)
- クライアントが切断 (キャンセル・タブを閉じる) すると解図を打ち切る。ただし切断に
  気づくのは進捗の書き込みが失敗したとき (切断後 2〜3 手目) なので、1 手が重い局面
  では十数秒遅れる
- 重い問題ではメモリ不足の可能性がある
- GitHub Pages にはリバースプロキシ機能がないため、API の絶対 URL をビルド時に埋め込む必要があります
