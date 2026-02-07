# Newton Test Results Report
Generated: 2026-02-07T14:04:31Z
Command: ./scripts/run-tests.sh
Output File: test_results.md
JSON File: test_results.json

## Overall Status
✅ **PASSED** - All tests completed successfully

## Test Statistics
- **Total Tests:** 455
- **Passed:** 447
- **Failed:** 
- **Skipped:** 8
- **Passing Rate:** 98%

## Progress Visualization
```
[█████████████████████████████░] 98% (447/455)
```

## Performance
- **Test Duration:** 1.623s

## Files
- **Raw Test Output:** `test_results.json`
- **Markdown Report:** `test_results.md`

## Raw Test Output
Complete test output is saved in: `test_results.json`

You can analyze it with standard Unix tools:
```bash
# Count total tests
grep -c 'PASS\|FAIL\|SKIP' test_results.json

# Show failed tests
grep -A 2 -B 2 'FAIL' test_results.json
```
