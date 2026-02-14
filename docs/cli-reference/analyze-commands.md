# Analyze Commands

The `analyze` command group provides diagnostic tools for understanding skill relationships and index quality.

## Overview

```bash
fastskill analyze <SUBCOMMAND>
```

### Available Subcommands

- `matrix` - Show pairwise similarity matrix between all indexed skills
- `cluster` - (Coming soon) Group skills by semantic similarity
- `duplicates` - (Coming soon) Find potential duplicate skills

---

## Matrix Command

Show pairwise similarity matrix between all indexed skills.

### Usage

```bash
fastskill analyze matrix [OPTIONS]
```

### Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `--json` | flag | false | Output in JSON format |
| `--threshold <T>` | float | 0.0 | Minimum similarity to display (0.0 to 1.0) |
| `--limit <N>` | integer | 10 | Number of similar skills to show per skill |
| `--full` | flag | false | Show all similar skills instead of top-N |
| `--duplicate-threshold <T>` | float | 0.95 | Threshold for flagging potential duplicates |

### Examples

**Basic usage** - Show top 10 similar skills per skill:
```bash
fastskill analyze matrix
```

**Filter by threshold** - Only show pairs with similarity ≥ 0.8:
```bash
fastskill analyze matrix --threshold 0.8
```

**Limit results** - Show top 5 per skill:
```bash
fastskill analyze matrix --limit 5
```

**Full matrix** - Show all pairs above threshold:
```bash
fastskill analyze matrix --full
```

**JSON output** - For programmatic processing:
```bash
fastskill analyze matrix --json
```

**Combined options**:
```bash
fastskill analyze matrix --threshold 0.9 --limit 3 --json
```

### Output Format

#### Default (Human-Readable)

```
Calculating pairwise similarities for 5 skills...

================================================================================
Similarity Matrix
================================================================================

Skill One (skill-one)
  Top similar: skill-two (0.923), skill-three (0.876), skill-four (0.834)

Skill Two (skill-two)
  Top similar: skill-one (0.923), skill-four (0.856), skill-three (0.821)

================================================================================

================================================================================
Potential duplicates (similarity >= 0.95):
================================================================================
  skill-a <-> skill-b (similarity: 0.967)
  skill-c <-> skill-d (similarity: 0.945)

Consider reviewing these pairs for consolidation.
```

#### JSON Format

```json
[
  {
    "skill_id": "skill-one",
    "name": "Skill One",
    "similar_skills": [
      ["skill-two", 0.923],
      ["skill-three", 0.876],
      ["skill-four", 0.834]
    ]
  },
  {
    "skill_id": "skill-two",
    "name": "Skill Two",
    "similar_skills": [
      ["skill-one", 0.923],
      ["skill-four", 0.856]
    ]
  }
]
```

### Use Cases

#### 1. Skill Discovery
Find related skills you didn't know about:

```bash
fastskill analyze matrix | grep "data-processing"
# Shows all skills related to data processing
```

#### 2. Deduplication
Identify overly similar skills:

```bash
fastskill analyze matrix --threshold 0.95
# Find potential duplicates for review
```

#### 3. Quality Verification
Ensure embeddings produce meaningful similarities:

```bash
fastskill analyze matrix --limit 5
# Review top matches for sanity check
```

#### 4. Export for Analysis
Generate JSON for external analysis:

```bash
fastskill analyze matrix --json > similarity-matrix.json
# Process with jq, pandas, or other tools
```

### Requirements

- Skills must be indexed with embeddings
- Run `fastskill reindex` first if no index exists
- Requires OpenAI API key for initial indexing (embedding generation)

### Performance Notes

The command calculates pairwise similarities, which has O(N²) complexity:

- 10 skills: ~45 comparisons (instant)
- 50 skills: ~1,225 comparisons (< 1 second)
- 100 skills: ~4,950 comparisons (1-2 seconds)
- 500 skills: ~124,750 comparisons (5-10 seconds)

For large collections (> 100 skills), use `--threshold` to filter results and speed up processing.

### Technical Details

#### Similarity Calculation

The command uses **cosine similarity** to measure semantic relatedness:

```
cosine_similarity(A, B) = (A · B) / (||A|| × ||B||)
```

Where:
- `A` and `B` are embedding vectors
- `·` is dot product
- `||A||` is L2 norm (Euclidean magnitude)
- Result ranges from -1.0 (opposite) to 1.0 (identical)

For normalized embeddings (like OpenAI's), values typically range from 0.0 to 1.0.

#### Interpretation

| Similarity | Interpretation |
|------------|----------------|
| 0.95 - 1.0 | Nearly identical (potential duplicates) |
| 0.80 - 0.95 | Very similar (closely related) |
| 0.60 - 0.80 | Moderately similar (related) |
| 0.40 - 0.60 | Somewhat similar (loosely related) |
| < 0.40 | Dissimilar (unrelated) |

### Error Handling

| Error | Cause | Solution |
|-------|-------|----------|
| "Vector index not available" | No index initialized | Run `fastskill reindex` |
| "No skills indexed" | Index exists but empty | Add skills and run `fastskill reindex` |
| "threshold must be between 0.0 and 1.0" | Invalid threshold value | Use value in valid range |
| "Failed to get indexed skills" | Database error | Check index file permissions |

---

## Future Commands

### Cluster (Planned)

Group skills by semantic similarity to identify natural categories:

```bash
fastskill analyze cluster --num-clusters 5
```

Will use k-means or hierarchical clustering on embeddings.

### Duplicates (Planned)

Dedicated command for finding and managing duplicates:

```bash
fastskill analyze duplicates --threshold 0.95 --auto-merge
```

Will provide interactive duplicate resolution workflow.

---

## See Also

- `fastskill search` - Search for skills by semantic similarity
- `fastskill reindex` - Rebuild the vector index
- `fastskill show` - Display skill details
