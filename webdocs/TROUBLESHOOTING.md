# Troubleshooting FastSkill Documentation

## 403 Errors for CDN Icons

If you're seeing 403 (Forbidden) errors for SVG icons from CloudFront CDN like:

```text
GET https://d3gk2c5xim1je2.cloudfront.net/v7.1.0/regular/cpu.svg 403 (Forbidden)
GET https://d3gk2c5xim1je2.cloudfront.net/v7.1.0/regular/refresh-cw.svg 403 (Forbidden)
```

### What This Means

These errors occur when Mintlify tries to load icon SVGs from their CDN. This is usually not a critical issue because:

1. **Icons Are Optional**: Mintlify will gracefully handle missing icons by showing fallbacks or omitting them
2. **Common Causes**:

   - Network/firewall blocking CloudFront CDN access
   - Development environment accessing CDN
   - CORS restrictions (less likely)
   - Mintlify CDN temporarily unavailable

### Solutions

#### Option 1: Use Standard Mintlify Icons (Recommended)

The icons used in the documentation (`cpu`, `refresh-cw`, `plug`, `zap`, `search`, `shield`) are valid Mintlify icon names. The 403 errors are likely environmental and won't affect production.

#### Option 2: Use Local SVG Icons (If Needed)

If CDN access is permanently blocked, you can use local SVG icons instead:

1. Replace icon names with file paths:

   ```markdown
   <Card title="Framework Agnostic" icon="/icons/cpu.svg" href="/architecture">
   ```

2. Store SVG icons in `/icons/` directory:

   ```bash
   mkdir -p webdocs/icons
   ```

3. Download or create the SVG files locally

#### Option 3: Use Icon Library Alternatives

You can also use emoji or remove icons:

```markdown
<Card title="Framework Agnostic" href="/architecture">
```

### Verification

To verify icons are correct, check:

- [Mintlify Icon Reference](https://mintlify.com/docs/components/icon)
- Icon names match exactly (case-sensitive)
- Documentation builds successfully despite 403 errors

### Production Notes

These 403 errors typically:

- **Don't affect documentation rendering** in production
- **Don't block deployment** via Mintlify
- **Are environment-specific** and may not occur in production

If icons are critical, consider hosting your own icon set or using Mintlify's icon system which may have different CDN access in production.
