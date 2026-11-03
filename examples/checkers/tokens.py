"""Compare ordered raw-byte tokens, ignoring ASCII whitespace differences."""

import json
import sys
from pathlib import Path


def main():
    """Require empty configuration before comparing the two token sequences."""
    config = json.loads(Path(sys.argv[4]).read_text(encoding="utf-8"))
    if config != {}:
        raise ValueError("This checker requires an empty configuration object")

    expected = Path(sys.argv[2]).read_bytes().split()
    actual = Path(sys.argv[3]).read_bytes().split()
    verdict = "AC" if actual == expected else "WA"
    print(json.dumps({"verdict": verdict}))


if __name__ == "__main__":
    main()
