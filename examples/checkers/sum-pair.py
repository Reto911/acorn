"""Judge two positive integers whose sum matches the input, without a reference answer."""

import json
import sys
from pathlib import Path


def main():
    """Read the task input and treat malformed candidate output as a normal wrong answer."""
    number = int(Path(sys.argv[1]).read_bytes())
    if number < 3:
        raise ValueError("Input must be an integer greater than or equal to three")
    tokens = Path(sys.argv[3]).read_bytes().split()
    try:
        values = [int(token) for token in tokens]
    except ValueError:
        accepted = False
    else:
        accepted = (
            len(values) == 2
            and all(value > 0 for value in values)
            and sum(values) == number
        )
    print(json.dumps({"verdict": "AC" if accepted else "WA"}))


if __name__ == "__main__":
    main()
