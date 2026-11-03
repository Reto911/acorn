"""Compare raw-byte token multisets, preserving the count of every token."""

import json
import sys
from collections import Counter
from pathlib import Path


def main():
    """Require empty configuration before comparing unordered token counts."""
    config = json.loads(Path(sys.argv[4]).read_text(encoding="utf-8"))
    if config != {}:
        raise ValueError("This checker requires an empty configuration object")

    expected = Counter(Path(sys.argv[2]).read_bytes().split())
    actual = Counter(Path(sys.argv[3]).read_bytes().split())
    verdict = "AC" if actual == expected else "WA"
    print(json.dumps({"verdict": verdict}))


if __name__ == "__main__":
    main()
