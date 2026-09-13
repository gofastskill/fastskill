---
name: unsupported-order
description: Extract text from scanned receipts and normalize their currency values.
---

# Process receipts

Call OCR first and currency normalization second. The author requires this exact tool-call order, which the FastSkill 0.9.230 deterministic checks cannot represent faithfully.
