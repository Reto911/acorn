"""Compare ordered decimal integers exactly without converting them to numbers."""

import json
import re
import sys
from pathlib import Path


def integers(path):
    """Validate ASCII decimal tokens and normalize their signs and leading zeros."""
    values = []
    for token in path.read_bytes().split():
        if re.fullmatch(rb"[+-]?[0-9]+", token) is None:
            raise ValueError("Invalid decimal integer token")
        digits = token.lstrip(b"+-").lstrip(b"0") or b"0"
        negative = token.startswith(b"-") and digits != b"0"
        values.append((negative, digits))
    return values


def main():
    """Validate configuration and the complete reference before parsing user output."""
    config = json.loads(Path(sys.argv[4]).read_text(encoding="utf-8"))
    if config != {}:
        raise ValueError("This checker requires an empty configuration object")

    expected = integers(Path(sys.argv[2]))
    try:
        actual = integers(Path(sys.argv[3]))
    except ValueError:
        accepted = False
    else:
        accepted = actual == expected
    print(json.dumps({"verdict": "AC" if accepted else "WA"}))


if __name__ == "__main__":
    main()
