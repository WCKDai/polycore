# GitHub environments and secret storage

Two environments exist in PolyCore and Polykrom:

- **`ci`** for the narrow-scope R2 credential used by `sccache`.
- **`agents`** for model/provider credentials when agents run on GitHub runners.

Environment variables are readable configuration. Secrets are encrypted and are never committed
or printed. Enter secret values in the GitHub UI; do not place them in variables.

## CI environment

| Repository | Settings |
|---|---|
| PolyCore | https://github.com/WCKDai/polycore/settings/environments/ci |
| Polykrom | https://github.com/WCKDai/polykrom/settings/environments/ci |

Current variables:

| Variable | Initial value | Meaning |
|---|---|---|
| `R2_CONFIGURED` | `false` | Safety gate. Set `true` only after credentials work. |
| `R2_BUCKET` | `SET_ME` | Private cache bucket name. |
| `R2_ENDPOINT` | `https://SET_ME.r2.cloudflarestorage.com` | R2 S3 endpoint for the account. |
| `SCCACHE_REGION` | `auto` | R2's S3 region. |
| `SCCACHE_S3_USE_SSL` | `true` | TLS for cache traffic. |
| `SCCACHE_S3_KEY_PREFIX` | `polycore` / `polykrom` | Keeps repository cache namespaces separate. |

Add these **encrypted secrets** in each `ci` environment:

```text
R2_ACCESS_KEY_ID
R2_SECRET_ACCESS_KEY
```

Recommended R2 setup:

1. Create a private bucket, for example `wckdboy-sccache`.
2. Create a dedicated R2 API token with **Object Read & Write**, limited to that bucket.
   Do not use an account-wide admin token.
3. Set `R2_BUCKET` and `R2_ENDPOINT` in each repository's `ci` environment.
4. Enter the token's access key ID and secret access key as the two secrets above.
5. Set `R2_CONFIGURED=true` in both environments.
6. Run one full CI job and confirm it saves and restores cache entries before relying on it.

Rotate the R2 token independently of model-provider keys. If it leaks, only the cache bucket is
exposed. Bucket lifecycle rules may expire stale cache objects after 30–90 days.

The workflow maps these values to sccache's S3 variables:

```yaml
RUSTC_WRAPPER: sccache
SCCACHE_BUCKET: ${{ vars.R2_BUCKET }}
SCCACHE_ENDPOINT: ${{ vars.R2_ENDPOINT }}
SCCACHE_REGION: ${{ vars.SCCACHE_REGION }}
SCCACHE_S3_USE_SSL: ${{ vars.SCCACHE_S3_USE_SSL }}
SCCACHE_S3_KEY_PREFIX: ${{ vars.SCCACHE_S3_KEY_PREFIX }}
AWS_ACCESS_KEY_ID: ${{ secrets.R2_ACCESS_KEY_ID }}
AWS_SECRET_ACCESS_KEY: ${{ secrets.R2_SECRET_ACCESS_KEY }}
```

Pull-request jobs intentionally do not reference the `ci` environment. They use GitHub's local
Rust cache and cannot read the R2 credentials. Full/merge/nightly jobs opt into the environment.

## Agent environment

| Repository | Settings |
|---|---|
| PolyCore | https://github.com/WCKDai/polycore/settings/environments/agents |
| Polykrom | https://github.com/WCKDai/polykrom/settings/environments/agents |

Optional secrets, only needed when workers run on GitHub rather than the current local OpenCode:

```text
DEEPSEEK_API_KEY
ANTHROPIC_API_KEY
OPENAI_API_KEY
GEMINI_API_KEY
TEMPORAL_API_KEY
PLANE_API_TOKEN
```

Current model routing defaults:

| Variable | Value |
|---|---|
| `AGENT_MODEL_CHEAP` | `deepseek-flash` |
| `AGENT_MODEL_STANDARD` | `claude-sonnet-5-5` |
| `AGENT_MODEL_STRONG` | `claude-opus-5-5` |
| `AGENT_MODEL_REVIEW` | `gpt-6.1-sol` |
| `AGENT_MAX_CONCURRENT` | `2` on the current Mac; raise on dedicated runners |
| `AGENT_MONTHLY_USD_BUDGET` | `100`; a hard cap, not a target |

The API keys already attached to OpenCode are sufficient for local agents. Do not duplicate them
into GitHub until a GitHub-hosted worker needs them.

## Runner recommendation

Start with:

1. the existing Apple Silicon Mac for local agents and macOS/OCCT validation;
2. one self-hosted Linux 16–32 core, 64 GB RAM, NVMe runner for Rust CI and 2–4 workers;
3. GitHub-hosted Linux for PR fallback;
4. an NVIDIA GPU runner only for nightly CFD/GPU/Omniverse work, not every PR.

Register self-hosted runners with labels such as `self-hosted`, `linux`, `x64`, `rust-32c`,
`macos`, `arm64`, and `nvidia-gpu`. Use ephemeral runners and GitHub OIDC.
