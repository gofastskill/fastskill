# Invoice extraction eval review

Compatibility: authored for FastSkill 0.9.230; validate against another installed release before use.

Coverage: ordinary extraction, missing invoice number, and unrelated-request non-triggering. The author confirmed that a missing number must be JSON null. Test-owned expected facts are supplied through the `expected` CSV column and rendered into the judge prompt. Field correctness is an outcome expectation. Skill consultation/non-consultation is an adherence/selection expectation imposed by `should_trigger` semantics. Currency-normalization command text is advisory only.

Calibration contrasts: JSON null is acceptable for the missing number; the string `"UNKNOWN"` is unacceptable. These labels are based on the confirmed criterion. They have not been executed against a judge in this example.

Unsupported: ordered OCR-then-normalization calls cannot be asserted by the current deterministic check types. No weaker substitute is claimed.

Status: example files are syntax-validated without contacting the placeholder judge endpoint. No runtime pilot or judge calibration is claimed.
