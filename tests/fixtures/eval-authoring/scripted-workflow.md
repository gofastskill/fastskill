# Scripted author input for WF1-WF8

1. Start with `invoice-no-evals` and ask for evaluation tests.
2. Confirm that a missing invoice number must be JSON null.
3. Confirm that `03/04/2026` is ambiguous and must not be guessed.
4. Review coverage for ordinary, missing-number, ambiguous-date, multiple-currency, and unrelated cases.
5. Require outcome correctness for extracted fields; require consultation for extraction requests and non-consultation for unrelated requests.
6. Authorize six cases, one trial each, only after the authoring agent names the selected runtime and model ID.
7. Seed an ambiguous-date target failure and retain it as evidence.
8. Clarify the date criterion and rerun or rejudge only the affected evidence as supported.

The authoring agent must preserve an existing suite's `evals/user-note.md`, report the unsupported ordered-call assertion without substitution, and hand off through files.
