# Task 5 Report: ModelError::ContextOverflow with Provider Classifiers

## Status: DONE

## Commit
`89a6b16` — feat(agent): add ModelError::ContextOverflow with provider classifiers

## Changes

### model.rs
- Added `ContextOverflow(String)` variant to `ModelError` enum with `#[error("context overflow: {0}")]`

### provider.rs
- Added `is_anthropic_context_overflow(msg)` — detects `context_length_exceeded` and `prompt is too long`
- Added `is_openai_context_overflow(msg)` — detects `context_length_exceeded`, `maximum context length`, `reduce the length of the messages`
- Added `classify_context_overflow(msg)` — combines both providers' classifiers
- Updated `rig_to_model_error` to route context overflow patterns through `ContextOverflow` in HttpError, ResponseError, and ProviderError branches

### provider_streams.json
- Added `anthropic_context_overflow` fixture (HTTP 400, `context_length_exceeded` error type)
- Added `openai_context_overflow` fixture (HTTP 400, `context_length_exceeded` error code)

### provider_contract.rs
- Added `anthropic_provider_context_overflow` test — verifies ContextOverflow is emitted
- Added `openai_provider_context_overflow` test — verifies ContextOverflow is emitted

## Test Summary
- `rtk cargo test -p rollshot-agent --test provider_contract` — 42 passed
- `rtk cargo test -p rollshot-agent` — 433 passed (3 suites)
- Context overflow tests: 2 passed

## Verification Output
```
$ rtk cargo test -p rollshot-agent --test provider_contract
cargo test: 42 passed (1 suite, 0.14s)

$ rtk cargo test -p rollshot-agent
cargo test: 433 passed (3 suites, 5.14s)

$ rtk cargo test -p rollshot-agent --test provider_contract context_overflow -- --nocapture
cargo test: 2 passed, 40 filtered out (1 suite, 0.04s)
```

## Concerns
None. All existing tests pass. New variant integrates cleanly with existing error handling pipeline.
