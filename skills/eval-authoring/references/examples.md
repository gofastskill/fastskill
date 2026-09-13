# Authoring examples

The executable example is in [`../examples/invoice-extraction`](../examples/invoice-extraction). It is compatible with FastSkill 0.9.230. Run `fastskill eval validate` from that directory without credentials; select a runtime only when intentionally checking its availability and observability.

## Review table

| Case | Intent | Expected result | Measurement |
| --- | --- | --- | --- |
| `invoice-basic` | Ordinary invoice | Correct number, date, currency, and total | outcome judge; trigger required by `should_trigger=true` |
| `invoice-missing-number` | Missing field | `invoice_number` is null | outcome judge; trigger required |
| `invoice-unrelated` | Unrelated request | Skill is not consulted | adherence/selection through `should_trigger=false` |

An optional procedure can be recorded without gating:

```toml
[[check]]
name = "command_contains"
pattern = "normalize_currency"
required = false
cases = ["invoice-basic"]
```

Do not describe the case as outcome-only while `should_trigger=true`: the implicit consultation check remains required. If outcome-only scoring is intended and the current format cannot disable the implicit check without contradicting the CSV, record that policy as unsupported rather than changing `should_trigger` deceptively.

## Contrasting judge outcomes

Criterion: “The response represents a missing invoice number as JSON null.”

- Acceptable: `{"invoice_number":null,"date":"2026-09-01"}`. Label reason: the confirmed missing-number policy is null.
- Unacceptable: `{"invoice_number":"UNKNOWN","date":"2026-09-01"}`. Label reason: the sentinel string violates the confirmed policy.

These labels support review. They are executed calibration evidence only if passed through a supported real judge interface and the resulting artifact is retained.

## Unsupported expectation

“Verify that the agent called OCR before parsing and then called currency normalization, in that order.” The current checks can inspect substring presence and aggregate tool-call count, but do not provide a structured ordered-call assertion. Do not substitute two `command_contains` checks and claim order was verified. Keep the intended assertion in review notes as unsupported and propose the engine feature separately.
