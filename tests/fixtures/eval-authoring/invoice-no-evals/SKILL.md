---
name: invoice-no-evals
description: Extract known invoice fields into JSON when asked to process a plain-text invoice.
---

# Invoice extraction

Return `invoice_number`, `date`, `currency`, and `total`. If the invoice number is absent, use JSON null. An ambiguous numeric date must be returned as `ambiguous` rather than guessed. Do not activate for general questions about invoices.
