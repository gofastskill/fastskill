---
name: invoice-extraction
description: Extract invoice fields from plain-text invoices into JSON when the user asks for invoice data.
metadata:
  id: invoice-extraction
---

# Extract invoices

Return JSON with `invoice_number`, `date`, `currency`, and `total`. Use JSON null for a missing invoice number. Do not activate for unrelated questions.
