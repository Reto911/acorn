-- Six built-in examples retained in release distributions.
-- A + B keeps 104 original cases; quotient has eight; problems 3-6 have four each.
-- Run once after 001-initial-schema.sql against a newly initialized database.
-- This is initial data, not a schema migration. It does not create a user.

SET NAMES utf8mb4 COLLATE utf8mb4_0900_bin;
SET SESSION sql_mode = 'STRICT_ALL_TABLES,NO_ENGINE_SUBSTITUTION';

START TRANSACTION;

INSERT INTO checker_scripts (id, name, source)
VALUES
    (1, 'trim_trailing_ascii_whitespace', '"""Compare raw output bytes after trimming trailing ASCII whitespace."""

import json
import sys
from pathlib import Path

trailing = b"\\t\\n\\v\\f\\r "
actual = Path(sys.argv[3]).read_bytes()
expected = Path(sys.argv[2]).read_bytes()

verdict = "AC" if actual.rstrip(trailing) == expected.rstrip(trailing) else "WA"
print(json.dumps({"verdict": verdict}))
'),
    (2, 'finite_float_tolerance', '"""Compare finite numeric tokens using absolute and relative tolerances."""

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
');

INSERT INTO problems (id, current_version_no, created_at_ms)
VALUES (1, NULL, CAST(UNIX_TIMESTAMP(CURRENT_TIMESTAMP(3)) * 1000 AS SIGNED));

INSERT INTO problem_versions (
    problem_id, version_no, title, statement, checker_script_id, checker_config_json,
    published_at_ms
)
VALUES (
    1,
    1,
    '两数求和（A + B）',
    '给定两个整数 a 和 b，输出它们的和。

## 输入格式

一行两个以空格分隔的整数 a 和 b，满足 -1000000000 <= a, b <= 1000000000。

## 输出格式

输出一个整数，表示 a + b，并换行。

## 样例输入

```text
1 2
```

## 样例输出

```text
3
```',
    1,
    '{}',
    CAST(UNIX_TIMESTAMP(CURRENT_TIMESTAMP(3)) * 1000 AS SIGNED)
);

INSERT INTO problem_version_languages (
    problem_id, version_no, language_id, cpu_time_ms, wall_time_ms, memory_kib
)
VALUES
    (1, 1, 'c', 1000, 3000, 262144),
    (1, 1, 'python', 2000, 5000, 262144);

INSERT INTO test_cases (
    problem_id, version_no, case_index, input, expected_output
)
VALUES
    (1, 1, 1, X'3120320A', X'330A'),
    (1, 1, 2, X'3020300A', X'300A'),
    (1, 1, 3, X'2D3320350A', X'320A'),
    (1, 1, 4, X'2D37202D31310A', X'2D31380A'),
    (1, 1, 5, X'2D31303030303030303030202D313030303030303030300A', X'2D323030303030303030300A'),
    (1, 1, 6, X'3130303030303030303020313030303030303030300A', X'323030303030303030300A'),
    (1, 1, 7, X'2D3130303030303030303020313030303030303030300A', X'300A'),
    (1, 1, 8, X'31303030303030303030202D313030303030303030300A', X'300A'),
    (1, 1, 9, X'2D3130303030303030303020300A', X'2D313030303030303030300A'),
    (1, 1, 10, X'30202D313030303030303030300A', X'2D313030303030303030300A'),
    (1, 1, 11, X'3130303030303030303020300A', X'313030303030303030300A'),
    (1, 1, 12, X'3020313030303030303030300A', X'313030303030303030300A'),
    (1, 1, 13, X'2D3130303030303030303020310A', X'2D3939393939393939390A'),
    (1, 1, 14, X'31202D313030303030303030300A', X'2D3939393939393939390A'),
    (1, 1, 15, X'31303030303030303030202D310A', X'3939393939393939390A'),
    (1, 1, 16, X'2D3120313030303030303030300A', X'3939393939393939390A'),
    (1, 1, 17, X'2D31303030303030303030202D310A', X'2D313030303030303030310A'),
    (1, 1, 18, X'2D31202D313030303030303030300A', X'2D313030303030303030310A'),
    (1, 1, 19, X'3130303030303030303020310A', X'313030303030303030310A'),
    (1, 1, 20, X'3120313030303030303030300A', X'313030303030303030310A'),
    (1, 1, 21, X'2D31303030303030303030203939393939393939390A', X'2D310A'),
    (1, 1, 22, X'393939393939393939202D313030303030303030300A', X'2D310A'),
    (1, 1, 23, X'31303030303030303030202D3939393939393939390A', X'310A'),
    (1, 1, 24, X'2D39393939393939393920313030303030303030300A', X'310A'),
    (1, 1, 25, X'2D373434313931393838202D3331333931323236340A', X'2D313035383130343235320A'),
    (1, 1, 26, X'37393134343336352039393133303930390A', X'3137383237353237340A'),
    (1, 1, 27, X'333839383437343939203834363833373139320A', X'313233363638343639310A'),
    (1, 1, 28, X'2D373739353839303534203839383633323830350A', X'3131393034333735310A'),
    (1, 1, 29, X'2D353230353030303739203932313538333638310A', X'3430313038333630320A'),
    (1, 1, 30, X'323930373033353032203333343632363934360A', X'3632353333303434380A'),
    (1, 1, 31, X'313935323031333936202D39363733363237310A', X'39383436353132350A'),
    (1, 1, 32, X'363832323831353330203232393538323135310A', X'3931313836333638310A'),
    (1, 1, 33, X'313736343136313339203830393034333934320A', X'3938353436303038310A'),
    (1, 1, 34, X'353638383230353430203636373637363931360A', X'313233363439373435360A'),
    (1, 1, 35, X'3634393531353130332035343230393137320A', X'3730333732343237350A'),
    (1, 1, 36, X'363132383135393636203636303136313134390A', X'313237323937373131350A'),
    (1, 1, 37, X'323539353433393233202D35323735353730370A', X'3230363738383231360A'),
    (1, 1, 38, X'2D343834373435343837202D3939343535363437310A', X'2D313437393330313935380A'),
    (1, 1, 39, X'333138393933363534202D3832363634373234300A', X'2D3530373635333538360A'),
    (1, 1, 40, X'2D373632313935303738202D3338333135363334340A', X'2D313134353335313432320A'),
    (1, 1, 41, X'373533393831363534202D3738393433353734370A', X'2D33353435343039330A'),
    (1, 1, 42, X'2D3334333733353931202D3937353332373932340A', X'2D313030393730313531350A'),
    (1, 1, 43, X'373439363839353437203931333332393932330A', X'313636333031393437300A'),
    (1, 1, 44, X'3437313430383638382035323838323734370A', X'3532343239313433350A'),
    (1, 1, 45, X'343538393736363438202D3332353138393439370A', X'3133333738373135310A'),
    (1, 1, 46, X'2D353438353435393834202D3134363938323132320A', X'2D3639353532383130360A'),
    (1, 1, 47, X'2D343539383739373134202D3235333431303436300A', X'2D3731333239303137340A'),
    (1, 1, 48, X'393935363933373032202D3233343132393931310A', X'3736313536333739310A'),
    (1, 1, 49, X'373239303136313736202D3139313830363336330A', X'3533373230393831330A'),
    (1, 1, 50, X'363031393538313936203130323932303835380A', X'3730343837393035340A'),
    (1, 1, 51, X'333639343837383134202D3833363130343236330A', X'2D3436363631363434390A'),
    (1, 1, 52, X'353532383731313438202D3236393030383231340A', X'3238333836323933340A'),
    (1, 1, 53, X'2D383037343435393532203139373131353931310A', X'2D3631303333303034310A'),
    (1, 1, 54, X'313535343835313938202D3337323637333035360A', X'2D3231373138373835380A'),
    (1, 1, 55, X'2D333832333739353338202D31373633383333350A', X'2D3430303031373837330A'),
    (1, 1, 56, X'2D363937343434353634203838323231353331340A', X'3138343737303735300A'),
    (1, 1, 57, X'333834363836383133203531313737353936320A', X'3839363436323737350A'),
    (1, 1, 58, X'353338343930323438203234303030373232340A', X'3737383439373437320A'),
    (1, 1, 59, X'2D333336343330333232202D3934363837323735370A', X'2D313238333330333037390A'),
    (1, 1, 60, X'383230363234333234203532363536353233320A', X'313334373138393535360A'),
    (1, 1, 61, X'2D323037393132323437202D3231393239383939340A', X'2D3432373231313234310A'),
    (1, 1, 62, X'2D39383534333033202D39333239343832380A', X'2D3130333134393133310A'),
    (1, 1, 63, X'2D383035323537313539202D3134333634303831330A', X'2D3934383839373937320A'),
    (1, 1, 64, X'393133333430373734203235373831393433320A', X'313137313136303230360A'),
    (1, 1, 65, X'3139303637303232362037303432343134300A', X'3236313039343336360A'),
    (1, 1, 66, X'383632333532313132202D3734393932393934360A', X'3131323432323136360A'),
    (1, 1, 67, X'2D38363336373433372038373339373132320A', X'313032393638350A'),
    (1, 1, 68, X'373233343631333736203631303932373936350A', X'313333343338393334310A'),
    (1, 1, 69, X'383631333431363136203238383834353034380A', X'313135303138363636340A'),
    (1, 1, 70, X'3837393937393033392035343438303432320A', X'3933343435393436310A'),
    (1, 1, 71, X'2D313434393832373938203936383731353539370A', X'3832333733323739390A'),
    (1, 1, 72, X'313236363833313239202D3434343536373436390A', X'2D3331373838343334300A'),
    (1, 1, 73, X'383133353032303732202D39383132393536340A', X'3731353337323530380A'),
    (1, 1, 74, X'323131313831303431203938393037303437350A', X'313230303235313531360A'),
    (1, 1, 75, X'3239303532363233203130313535303439350A', X'3133303630333131380A'),
    (1, 1, 76, X'313132333639313134203732393039363137320A', X'3834313436353238360A'),
    (1, 1, 77, X'2D393435353035313534203233353931313731320A', X'2D3730393539333434320A'),
    (1, 1, 78, X'2D353137383336353237203634343539393531340A', X'3132363736323938370A'),
    (1, 1, 79, X'2D373232393737303733203735363932353533340A', X'33333934383436310A'),
    (1, 1, 80, X'2D383935303134343631203531333235313439380A', X'2D3338313736323936330A'),
    (1, 1, 81, X'313233353938393630203930343434313434320A', X'313032383034303430320A'),
    (1, 1, 82, X'343339383531333939202D3737393731383839370A', X'2D3333393836373439380A'),
    (1, 1, 83, X'353430343036313833203333363339393839330A', X'3837363830363037360A'),
    (1, 1, 84, X'2D373738353030353920393430333931340A', X'2D36383434363134350A'),
    (1, 1, 85, X'2D3639343137313336322033353333303031390A', X'2D3635383834313334330A'),
    (1, 1, 86, X'383939303332393937202D3530383338323433360A', X'3339303635303536310A'),
    (1, 1, 87, X'383234353633383834203930323530313332310A', X'313732373036353230350A'),
    (1, 1, 88, X'2D373337393738353335203334353736343636300A', X'2D3339323231333837350A'),
    (1, 1, 89, X'353034373537353138202D31393033373331380A', X'3438353732303230300A'),
    (1, 1, 90, X'34343830353530323320383034313738310A', X'3435363039363830340A'),
    (1, 1, 91, X'353334343237313330203939363031303232310A', X'313533303433373335310A'),
    (1, 1, 92, X'313731373336333731202D3436303936373638390A', X'2D3238393233313331380A'),
    (1, 1, 93, X'323032313532383731202D3133363533393039330A', X'36353631333737380A'),
    (1, 1, 94, X'343338323835363131202D3333393337353434360A', X'39383931303136350A'),
    (1, 1, 95, X'313737313430303135203731373536313731380A', X'3839343730313733330A'),
    (1, 1, 96, X'3338393132373738392037373832313230380A', X'3436363934383939370A'),
    (1, 1, 97, X'3439393633383431372032383431343637380A', X'3532383035333039350A'),
    (1, 1, 98, X'373238343937333437203839393334323131370A', X'313632373833393436340A'),
    (1, 1, 99, X'2D313336383233313439202D36393939363035320A', X'2D3230363831393230310A'),
    (1, 1, 100, X'2D353635303637323630202D3336373833363334320A', X'2D3933323930333630320A'),
    (1, 1, 101, X'373836333637303935202D3432323234393731390A', X'3336343131373337360A'),
    (1, 1, 102, X'2D393830303738303138202D3932383839383339300A', X'2D313930383937363430380A'),
    (1, 1, 103, X'2D363738323133303033203430373938313939320A', X'2D3237303233313031310A'),
    (1, 1, 104, X'383934313730313030203238323633343435390A', X'313137363830343535390A');

INSERT INTO problems (id, current_version_no, created_at_ms)
VALUES (2, NULL, CAST(UNIX_TIMESTAMP(CURRENT_TIMESTAMP(3)) * 1000 AS SIGNED));

INSERT INTO problem_versions (
    problem_id, version_no, title, statement, checker_script_id, checker_config_json,
    published_at_ms
)
VALUES (
    2,
    1,
    '计算两个整数的商',
    '给定两个整数 a 和 b，输出它们的商 a / b。

## 输入格式

一行两个以空格分隔的整数 a 和 b，满足 -1000000 <= a <= 1000000，1 <= b <= 1000000。

## 输出格式

输出一个有限数值，表示 a / b。允许小数和科学记数法，忽略数值前后的 ASCII 空白，不单独限制小数位数。

绝对容差 abs_tol 和相对容差 rel_tol 均为 1e-6。设实际输出为 actual，参考值为 expected，满足以下条件即通过：

```text
abs(actual - expected) <= max(abs_tol, rel_tol * max(abs(actual), abs(expected)))
```

输出必须恰好包含一个数值；无法解析的数值、NaN 或无穷大均不通过。

## 样例输入

```text
1 3
```

## 样例输出

```text
0.333333333333
```',
    2,
    '{"abs_tol":1e-6,"rel_tol":1e-6}',
    CAST(UNIX_TIMESTAMP(CURRENT_TIMESTAMP(3)) * 1000 AS SIGNED)
);

INSERT INTO problem_version_languages (
    problem_id, version_no, language_id, cpu_time_ms, wall_time_ms, memory_kib
)
VALUES
    (2, 1, 'c', 1000, 3000, 262144),
    (2, 1, 'python', 2000, 5000, 262144);

INSERT INTO test_cases (
    problem_id, version_no, case_index, input, expected_output
)
VALUES
    (2, 1, 1, X'3020310A', X'300A'),
    (2, 1, 2, X'3120330A', X'302E333333333333333333333333333333330A'),
    (2, 1, 3, X'2D3120330A', X'2D302E333333333333333333333333333333330A'),
    (2, 1, 4, X'3120313030303030300A', X'302E3030303030310A'),
    (2, 1, 5, X'2D3120313030303030300A', X'2D302E3030303030310A'),
    (2, 1, 6, X'3130303030303020310A', X'313030303030300A'),
    (2, 1, 7, X'2D3130303030303020310A', X'2D313030303030300A'),
    (2, 1, 8, X'3130303030303020330A', X'3333333333332E333333333333333333330A');

INSERT INTO checker_scripts (id, name, source)
VALUES
    (3, 'tokens', '"""Compare ordered raw-byte tokens, ignoring ASCII whitespace differences."""

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
'),
    (4, 'integers', '"""Compare ordered decimal integers exactly without converting them to numbers."""

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
'),
    (5, 'unordered-tokens', '"""Compare raw-byte token multisets, preserving the count of every token."""

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
'),
    (6, 'unordered-lines', '"""Compare multisets of LF-separated records while retaining field order."""

import json
import sys
from collections import Counter
from pathlib import Path


def records(path):
    """Count nonempty raw-byte field tuples, accepting CRLF and ASCII whitespace."""
    counts = Counter()
    for line in path.read_bytes().split(b"\\n"):
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
');

INSERT INTO problems (id, current_version_no, created_at_ms)
VALUES (3, NULL, CAST(UNIX_TIMESTAMP(CURRENT_TIMESTAMP(3)) * 1000 AS SIGNED));

INSERT INTO problem_versions (
    problem_id, version_no, title, statement, checker_script_id, checker_config_json,
    published_at_ms
)
VALUES (
    3, 1, '单词逆序',
    '给定 n 个单词，按输入顺序的逆序输出所有单词。

## 输入格式

第一行是整数 n，1 <= n <= 100。随后是 n 个以 ASCII 空白分隔的单词，每个单词仅含小写 ASCII 字母 a-z，长度为 1 至 16。

## 输出格式

输出逆序后的 n 个单词。词项数量、顺序及字母必须一致，词项之间的 ASCII 空白数量和换行位置不限。

## 样例输入

```text
3
red blue green
```

## 样例输出

```text
green
blue
red
```',
    3, '{}', CAST(UNIX_TIMESTAMP(CURRENT_TIMESTAMP(3)) * 1000 AS SIGNED)
);

INSERT INTO problem_version_languages (
    problem_id, version_no, language_id, cpu_time_ms, wall_time_ms, memory_kib
)
VALUES
    (3, 1, 'c', 1000, 3000, 262144),
    (3, 1, 'python', 2000, 5000, 262144);

INSERT INTO test_cases (
    problem_id, version_no, case_index, input, expected_output
)
VALUES
    (3, 1, 1, X'310A6F616B0A', X'6F616B0A'),
    (3, 1, 2, X'330A72656420626C756520677265656E0A', X'677265656E20626C7565207265640A'),
    (3, 1, 3, X'340A612062206120630A', X'632061206220610A'),
    (3, 1, 4, X'330A61206162636465666768696A6B6C6D6E6F70207A0A', X'7A206162636465666768696A6B6C6D6E6F7020610A');

INSERT INTO problems (id, current_version_no, created_at_ms)
VALUES (4, NULL, CAST(UNIX_TIMESTAMP(CURRENT_TIMESTAMP(3)) * 1000 AS SIGNED));

INSERT INTO problem_versions (
    problem_id, version_no, title, statement, checker_script_id, checker_config_json,
    published_at_ms
)
VALUES (
    4, 1, '整数前缀和',
    '给定 n 个整数，输出每个位置的前缀和。第 i 个结果为前 i 个输入整数之和。

## 输入格式

第一行是整数 n，1 <= n <= 100。随后是 n 个整数 ai，-1000000000000000 <= ai <= 1000000000000000。所有前缀和均在有符号 64 位整数范围内。

## 输出格式

按顺序输出 n 个十进制整数，以 ASCII 空白分隔。允许显式正号、前导零和负零，按整数值精确比较。小数、科学记数法和下划线不合法。结果可能超过 2^53，请使用精确整数运算。

## 样例输入

```text
4
3 -5 0 4
```

## 样例输出

```text
+0003 -0002 -0002 +0002
```',
    4, '{}', CAST(UNIX_TIMESTAMP(CURRENT_TIMESTAMP(3)) * 1000 AS SIGNED)
);

INSERT INTO problem_version_languages (
    problem_id, version_no, language_id, cpu_time_ms, wall_time_ms, memory_kib
)
VALUES
    (4, 1, 'c', 1000, 3000, 262144),
    (4, 1, 'python', 2000, 5000, 262144);

INSERT INTO test_cases (
    problem_id, version_no, case_index, input, expected_output
)
VALUES
    (4, 1, 1, X'310A300A', X'300A'),
    (4, 1, 2, X'340A33202D35203020340A', X'33202D32202D3220320A'),
    (4, 1, 3, X'310A2D313030303030303030303030303030300A', X'2D313030303030303030303030303030300A'),
    (4, 1, 4, X'31300A313030303030303030303030303030302031303030303030303030303030303030203130303030303030303030303030303020313030303030303030303030303030302031303030303030303030303030303030203130303030303030303030303030303020313030303030303030303030303030302031303030303030303030303030303030203130303030303030303030303030303020373139393235343734303939330A', X'313030303030303030303030303030302032303030303030303030303030303030203330303030303030303030303030303020343030303030303030303030303030302035303030303030303030303030303030203630303030303030303030303030303020373030303030303030303030303030302038303030303030303030303030303030203930303030303030303030303030303020393030373139393235343734303939330A');

INSERT INTO problems (id, current_version_no, created_at_ms)
VALUES (5, NULL, CAST(UNIX_TIMESTAMP(CURRENT_TIMESTAMP(3)) * 1000 AS SIGNED));

INSERT INTO problem_versions (
    problem_id, version_no, title, statement, checker_script_id, checker_config_json,
    published_at_ms
)
VALUES (
    5, 1, '筛选单词',
    '给定 n 个单词与长度下界 k，输出所有长度至少为 k 的单词，保留每次出现。

## 输入格式

第一行是整数 n 和 k，1 <= n <= 100，1 <= k <= 16。随后是 n 个单词，每个单词仅含小写 ASCII 字母 a-z，长度为 1 至 16。

## 输出格式

以 ASCII 空白分隔输出所有符合要求的单词，顺序任意。相同单词的出现次数必须与输入中符合要求的次数一致；没有符合要求的单词时，输出为空或仅含 ASCII 空白。

## 样例输入

```text
4 3
a bear cat to
```

## 样例输出

```text
cat bear
```',
    5, '{}', CAST(UNIX_TIMESTAMP(CURRENT_TIMESTAMP(3)) * 1000 AS SIGNED)
);

INSERT INTO problem_version_languages (
    problem_id, version_no, language_id, cpu_time_ms, wall_time_ms, memory_kib
)
VALUES
    (5, 1, 'c', 1000, 3000, 262144),
    (5, 1, 'python', 2000, 5000, 262144);

INSERT INTO test_cases (
    problem_id, version_no, case_index, input, expected_output
)
VALUES
    (5, 1, 1, X'3220350A612062620A', X''),
    (5, 1, 2, X'3320310A63617420646F67206361740A', X'63617420646F67206361740A'),
    (5, 1, 3, X'3420330A6120626561722063617420746F0A', X'62656172206361740A'),
    (5, 1, 4, X'332031360A6162636465666768696A6B6C6D6E6F702073686F7274206162636465666768696A6B6C6D6E6F700A', X'6162636465666768696A6B6C6D6E6F70206162636465666768696A6B6C6D6E6F700A');

INSERT INTO problems (id, current_version_no, created_at_ms)
VALUES (6, NULL, CAST(UNIX_TIMESTAMP(CURRENT_TIMESTAMP(3)) * 1000 AS SIGNED));

INSERT INTO problem_versions (
    problem_id, version_no, title, statement, checker_script_id, checker_config_json,
    published_at_ms
)
VALUES (
    6, 1, '相邻单词对',
    '给定 n 个单词，对每两个相邻单词生成一条记录，输出全部 n-1 条记录。

## 输入格式

第一行是整数 n，2 <= n <= 100。随后是 n 个单词，每个单词仅含小写 ASCII 字母 a-z，长度为 1 至 16。

## 输出格式

每条记录占一行，包含前一个单词和后一个单词，以 ASCII 空白分隔，行内字段顺序必须保持。记录的行序任意，重复记录必须保留。以 LF 分行，兼容 CRLF，忽略空白行。

## 样例输入

```text
4
a b a b
```

## 样例输出

```text
b a
a b
a b
```',
    6, '{}', CAST(UNIX_TIMESTAMP(CURRENT_TIMESTAMP(3)) * 1000 AS SIGNED)
);

INSERT INTO problem_version_languages (
    problem_id, version_no, language_id, cpu_time_ms, wall_time_ms, memory_kib
)
VALUES
    (6, 1, 'c', 1000, 3000, 262144),
    (6, 1, 'python', 2000, 5000, 262144);

INSERT INTO test_cases (
    problem_id, version_no, case_index, input, expected_output
)
VALUES
    (6, 1, 1, X'320A6120620A', X'6120620A'),
    (6, 1, 2, X'340A612062206120620A', X'6120620A6220610A6120620A'),
    (6, 1, 3, X'330A78207820780A', X'7820780A7820780A'),
    (6, 1, 4, X'340A612062206320640A', X'6120620A6220630A6320640A');

UPDATE problems SET current_version_no = 1 WHERE id IN (1, 2, 3, 4, 5, 6);

COMMIT;
