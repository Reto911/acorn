"""Compare finite numeric tokens using absolute and relative tolerances."""

import json
import math
import sys
from pathlib import Path


def main():
    """Validate private data before judging user output through the four-file protocol."""
    config = json.loads(Path(sys.argv[4]).read_text(encoding="utf-8"))
    if config.keys() - {"abs_tol", "rel_tol"}:
        raise ValueError("Unknown tolerance parameter")
    abs_tol = config.get("abs_tol", 1e-6)
    rel_tol = config.get("rel_tol", 1e-6)
    for value in (abs_tol, rel_tol):
        if (
            isinstance(value, bool)
            or not isinstance(value, (int, float))
            or not math.isfinite(value)
            or value < 0
        ):
            raise ValueError("Tolerances must be finite nonnegative numbers")
    if rel_tol >= 1:
        raise ValueError("Relative tolerance must be less than one")

    expected = [float(token) for token in Path(sys.argv[2]).read_bytes().split()]
    if not all(math.isfinite(value) for value in expected):
        raise ValueError("Reference values must be finite")

    try:
        actual = [float(token) for token in Path(sys.argv[3]).read_bytes().split()]
    except ValueError:
        accepted = False
    else:
        accepted = (
            len(actual) == len(expected)
            and all(math.isfinite(value) for value in actual)
            and all(
                math.isclose(value, reference, abs_tol=abs_tol, rel_tol=rel_tol)
                for value, reference in zip(actual, expected)
            )
        )
    print(json.dumps({"verdict": "AC" if accepted else "WA"}))


if __name__ == "__main__":
    main()
