"""Compare multisets of LF-separated records while retaining field order."""

import json
import sys
from collections import Counter
from pathlib import Path


def records(path):
    """Count nonempty raw-byte field tuples, accepting CRLF and ASCII whitespace."""
    counts = Counter()
    for line in path.read_bytes().split(b"\n"):
        fields = tuple(line.split())
        if fields:
            counts[fields] += 1
    return counts


def main():
    """Require empty configuration before comparing unordered record counts."""
    config = json.loads(Path(sys.argv[4]).read_text(encoding="utf-8"))
    if config != {}:
        raise ValueError("This checker requires an empty configuration object")

    expected = records(Path(sys.argv[2]))
    actual = records(Path(sys.argv[3]))
    verdict = "AC" if actual == expected else "WA"
    print(json.dumps({"verdict": verdict}))


if __name__ == "__main__":
    main()
