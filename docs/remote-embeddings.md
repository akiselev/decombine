# Remote embeddings (OpenRouter)

decombine can embed through a remote API instead of a local ONNX model. It is
**never the default**: `embedding.backend` defaults to `fastembed` (local).

> **Privacy.** A remote backend sends every embedded function/method body
> off-machine — to OpenRouter and then to whichever upstream company serves
> the model (Voyage, Mistral, DeepInfra, Nebius, …). Upstreams have their own
> logging/training terms. You must choose a `data_policy`; there is no
> default and the config is rejected without one. For private or client code
> use `zdr`. Every run prints a notice naming the destination and policy, and
> `decombine check` shows it too.

## Config

```yaml
embedding:
  backend: openrouter
  model: qwen/qwen3-embedding-8b    # OpenRouter model id
  remote:
    data_policy: zdr                # required: zdr | no_training | provider_default
    dimensions: 4096                # required: output width, checked on every response
    providers: [deepinfra]          # strongly recommended: pin the upstream
    # quantizations: [bf16]         # keep routing off fp8/int8 endpoints
    # request_dimensions: true      # server-side Matryoshka truncation to `dimensions`
    # document_input_type: document # Voyage-style input_type, per side
    # query_input_type: query
    # query_prefix: "Instruct: ...\nQuery: "   # Qwen3 query instruction
    # max_input_tokens: 32768       # recorded in the identity; default 8192
    # max_inputs_per_request: 64
    # concurrency: 4
    # api_key_env: OPENROUTER_API_KEY   # the default
    # base_url: https://openrouter.ai/api/v1
```

The API key is read from `$OPENROUTER_API_KEY` (or `api_key_env`) and never
stored.

| `data_policy` | Sent to OpenRouter | Use for |
|---|---|---|
| `zdr` | `provider.zdr: true`, `data_collection: "deny"` | private code |
| `no_training` | `provider.data_collection: "deny"` | code that may be logged but not trained on |
| `provider_default` | nothing | public/OSS code only |

With `zdr`, a request that no zero-retention endpoint can serve **fails**
instead of falling back. The account-level privacy settings on openrouter.ai
apply in addition to these per-request settings.

### Identity and reproducibility

The model identity is `openrouter:<model id>`. Its revision records the
`providers` pin, `quantizations` and `document_input_type`, because all of
them change the vectors. A DB embedded with one routing refuses to mix in
another, as with any model change. If you leave `providers` unpinned,
OpenRouter may serve requests from different upstreams whose numerics
differ. The backend warns about this.

Inputs are not tokenized locally. `max_body_chars` (default 10 000 chars)
keeps bodies well under 8K-context models, `max_batch_token_area` is ignored,
and token stats are chars/4 estimates. The `embed` command prints the
request count, prompt tokens and the cost reported by OpenRouter.

### Raw cosine thresholds do not port

Recalibrate duplicate/comparison thresholds per model by their
background-relative position (see `docs/benchmarks.md`).

## Models (surveyed 2026-10-16)

Pulled from `https://openrouter.ai/api/v1/embeddings/models` (no key needed) and
the per-model `/endpoints` pages. Prices are per 1M input tokens. Re-check
before relying on them.

| id | upstreams | dims | ctx | $/1M | notes |
|---|---|---|---|---|---|
| `voyageai/voyage-code-4` | voyageai | 256–2048 Matryoshka, default 1024 | 32K | 0.12 | code-specialized; single upstream |
| `mistralai/codestral-embed-2505` | mistral (+zdr, eu) | 1536 default, up to 3072 | 8K | 0.15 | code-specialized |
| `qwen/qwen3-embedding-8b` | nebius, deepinfra ($0.01); siliconflow **fp8** ($0.04) | 4096, Matryoshka | 40K | 0.01 | MTEB-Code 80.68; pin away from fp8 |
| `qwen/qwen3-embedding-4b` | deepinfra | 2560, Matryoshka | 32K | 0.02 | |
| `openai/text-embedding-3-small` / `-large` | openai, azure | 1536 / 3072 | 8K | 0.02 / 0.13 | general-purpose baseline |
| `google/gemini-embedding-001` | vertex, ai-studio | 3072, Matryoshka | 20K | 0.15 | |
| `mistralai/mistral-embed-2312` | mistral | 1024 | 8K | 0.10 | |
| `baai/bge-m3` | — | 1024 | 8K | 0.01 | |

Not available on OpenRouter: voyage-code-3 (replaced by -code-4),
qwen3-embedding-0.6b (no upstreams), jina embeddings, nomic-embed-code. The
512-token models (bge-base/large, e5, gte, MiniLM) truncate typical function
bodies and are not worth using here.

**Candidates to evaluate** on `runs/altium-rebuilds/` and `runs/oss-eval/`:
`voyage-code-4` (quality), `qwen3-embedding-8b` pinned to deepinfra/nebius
(cost), and `codestral-embed-2505`. For a 50k-unit corpus of about 15M tokens,
that costs roughly $1.80, $0.15 and $2.25. Every option is cheap enough that
the choice should come from cluster quality. The quality claims above are
vendor self-reports; no independent head-to-head was found.

Before any private-code run, check which of these have ZDR endpoints
(`/api/v1/models/<id>/endpoints`). A `zdr` request to a model without one
fails loudly.

## Adding another provider

Remote execution is split in `codeindex-embedding::embed::remote`:

- `RemoteBackend` handles everything provider-independent: request
  chunking, concurrency, retries with `Retry-After`, halving requests on 413,
  width checks, normalization, input-order reassembly and usage metering.
- `RemoteProvider` (trait) is the provider-specific translator: `endpoint`,
  `headers`, `request_body(role, inputs)` (which must express the data
  policy), `interpret(status, body) -> Reply`, `revision` (settings that
  change vectors), `routing`, and `disclosure`.
- `openai_compat` holds the shared OpenAI-style body/response handling, so an
  OpenAI-compatible provider (OpenAI, Voyage direct, DeepInfra, a local
  TEI/vLLM server) is mostly a `RemoteProviderSpec` plus a policy mapping.

To register one, add its `RemoteProviderSpec` (backend name, default URL,
default key variable, constructor) to `REMOTE_PROVIDERS`. decombine's
`embedding.backend` validation and `embedding.remote` block pick it up
without changes.
