# GitHub environments and secret storage

The repositories are owned by **WCKDai**. Shared CI configuration is stored once at organization
level, selected only for `polycore`, `polykrom` and `platform`. Infrastructure control lives in
the private [`WCKDai/platform`](https://github.com/WCKDai/platform) repository because GitHub
environments themselves cannot be organization-wide.

## Shared organization configuration

| Store | URL |
|---|---|
| Organization Actions variables | https://github.com/organizations/WCKDai/settings/actions/variables |
| Organization Actions secrets | https://github.com/organizations/WCKDai/settings/actions/secrets |

Variables present:

| Variable | Initial value | Meaning |
|---|---|---|
| `R2_CONFIGURED` | `false` | Safety gate; set `true` only after credentials work. |
| `R2_BUCKET` | `SET_ME` | Private cache bucket. |
| `R2_ENDPOINT` | `https://SET_ME.r2.cloudflarestorage.com` | R2 S3 endpoint. |
| `SCCACHE_REGION` | `auto` | R2 S3 region. |
| `SCCACHE_S3_USE_SSL` | `true` | TLS. |
| `AGENT_MODEL_*` | model aliases | Shared agent routing defaults. |
| `AGENT_MONTHLY_USD_BUDGET` | `100` | Initial hard budget. |
| `AGENT_MAX_CONCURRENT` | `2` | Current Mac-safe worker concurrency. |

Organization secrets present:

```text
R2_ACCESS_KEY_ID
R2_SECRET_ACCESS_KEY
```

All are selected only for `platform`, `polycore` and `polykrom`.

Each repository keeps only its non-secret prefix as a repository variable:

```text
polycore → SCCACHE_S3_KEY_PREFIX=polycore
polykrom → SCCACHE_S3_KEY_PREFIX=polykrom
```

The duplicate repository environments have been removed.

## R2 setup

1. Create a private bucket, for example `wckdboy-sccache`.
2. Create a dedicated R2 API token with **Object Read & Write**, limited to that bucket.
   Do not use an account-wide admin token.
3. Update the two organization variables `R2_BUCKET` and `R2_ENDPOINT`.
4. Replace organization secrets `R2_ACCESS_KEY_ID` and `R2_SECRET_ACCESS_KEY` (they are
   placeholders now).
5. Set `R2_CONFIGURED=true`.
6. Run full CI twice and verify sccache saves and restores entries.

The workflow maps values as:

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

Pull-request jobs do not install sccache or reference R2. Full, merge and nightly jobs do.

## Infrastructure and agent control

These protected environments are in `WCKDai/platform`:

| Environment | Purpose | Access |
|---|---|---|
| [`infra`](https://github.com/WCKDai/platform/settings/environments/infra) | Coolify, Hetzner, runner registration | `main` only |
| [`network`](https://github.com/WCKDai/platform/settings/environments/network) | Tailscale tailnet and OAuth credentials | `main` only |
| [`agents`](https://github.com/WCKDai/platform/settings/environments/agents) | LiteLLM/model keys and budgets | trusted workflows |

All currently carry `INFRA_CONFIGURED=false` until real values replace the `SET_ME` placeholders.

Provider keys should live once in the Coolify-hosted LiteLLM service. GitHub workers should
receive only project-scoped LiteLLM virtual keys, never the upstream Anthropic, OpenAI, Gemini or
DeepSeek credentials.

See the platform repository's [environment guide](https://github.com/WCKDai/platform/blob/main/docs/github-environments.md)
for every field.
