"""Compare raw output bytes after trimming trailing ASCII whitespace."""

import json
import sys
from pathlib import Path

trailing = b"\t\n\v\f\r "
actual = Path(sys.argv[3]).read_bytes()
expected = Path(sys.argv[2]).read_bytes()

verdict = "AC" if actual.rstrip(trailing) == expected.rstrip(trailing) else "WA"
print(json.dumps({"verdict": verdict}))
