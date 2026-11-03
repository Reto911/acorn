# 判题脚本编写材料

这里的单文件 Python 脚本使用标准库，可作为题库脚本的编写材料。运行时由后端从 MySQL 装配源码，Worker 执行任务中的副本；修改这里的文件不会自动更新数据库，也不会改变已经装配的任务。正式新库初始化使用 [002-example-problem.sql](../../sql/002-example-problem.sql) 中的源码。维护仓库预设时须同步该 SQL 中的对应源码，数据库读回检查会核对它们的字节一致性。

## 脚本与示例题

| 文件                       | 判断规则与参数                                                                         | 使用位置                                 |
| -------------------------- | -------------------------------------------------------------------------------------- | ---------------------------------------- |
| [trim.py](trim.py)         | 删除整个字节串末尾的 HT、LF、VT、FF、CR、SPACE 后逐字节比较；内部字节不变，参数为 `{}` | 脚本 ID 1；题号 1 两数求和，104 例       |
| [float.py](float.py)       | 按连续 ASCII 空白分词，使用 Python `float` 解析有限数值并逐项比较；项数须相同          | 脚本 ID 2；题号 2 计算两个整数的商，8 例 |
| [sum-pair.py](sum-pair.py) | 输入整数 n >= 3；输出恰好两个正整数，和为 n。忽略参考文件与参数，使用空参考和 `{}`     | 专用开发夹具，不写入正式种子             |

商题的输入为 -1000000 <= a <= 1000000、1 <= b <= 1000000，输出一个有限数值 a / b。C／Python 的 CPU、墙钟及内存限额与两数求和相同，checker 默认采用题目版本的 Python 限额。可提交 [C 样例](../submissions/quotient.c)或 [Python 样例](../submissions/quotient.py)：前者输出固定小数，后者输出科学记数法。

float 支持 `abs_tol`、`rel_tol`，缺省均为 1e-6，按以下条件接受每一项：

```text
abs(actual - expected) <= max(abs_tol, rel_tol * max(abs(actual), abs(expected)))
```

参数必须是有限非负数值，布尔值不作为数值；rel_tol 须小于 1，未知参数报错。允许零容差。正式商题显式使用 `{"abs_tol":1e-6,"rel_tol":1e-6}`。参考数据和参数先校验，错误以非零退出报告；用户项数不符、无法解析、NaN 或无穷大形成正常 WA。

sum-pair 对 n = 5 接受 `1 4` 和 `2 3`，拒绝 `1 3`、非正数、格式错误或多余数值；无需为不同合法输出保存唯一参考答案。

## 文件协议与本地运行

Worker 使用配置的 Python 解释器，命令固定为：

```text
python3 -I -B -S checker.py input.bin answer.bin output.bin config.json
```

四个参数依次是原始题目输入、参考输出或辅助数据、用户 stdout 原始字节、版本 JSON 对象参数。stdin 为空。用户程序仍使用标准输入和标准输出；脚本、参考和私有参数进入独立 checker 环境。用户输出超限时先形成 OLE，截断数据不会继续送入 checker。

判断完成后，脚本以退出码 0 输出唯一 UTF-8 JSON 对象 `{"verdict":"AC"}` 或 `{"verdict":"WA"}`。调试信息写 stderr；stdout 不混入其他内容。异常、非零退出、独立超限或结果协议错误形成 CheckerFailed。Worker 对结果和诊断使用独立预算，原始 checker 输出不进入公开结果或 LLM 输入；维护者可从内部日志查看有界 stderr 摘要。

在仓库根目录可用四个文件检查 float 材料：

```sh
ACORN_CHECKER_EXAMPLE=$(mktemp -d)
printf '1 3\n' > "$ACORN_CHECKER_EXAMPLE/input.bin"
printf '0.3333333333333333\n' > "$ACORN_CHECKER_EXAMPLE/answer.bin"
printf '3.333333333333e-1\n' > "$ACORN_CHECKER_EXAMPLE/output.bin"
printf '{}\n' > "$ACORN_CHECKER_EXAMPLE/config.json"
python3 -I -B -S examples/checkers/float.py \
  "$ACORN_CHECKER_EXAMPLE/input.bin" "$ACORN_CHECKER_EXAMPLE/answer.bin" \
  "$ACORN_CHECKER_EXAMPLE/output.bin" "$ACORN_CHECKER_EXAMPLE/config.json" < /dev/null
rm -r -- "$ACORN_CHECKER_EXAMPLE"
```

这会输出 AC。该本地命令验证脚本逻辑；隔离和限额检查使用 [Worker 的真实 Isolate 入口](../../crates/worker/README.md#isolate-阶段检查)。

## 数据库维护

脚本和题目数据通过直接 SQL 维护，完整建表及空库初始化步骤见 [SQL 说明](../../sql/README.md)。新增脚本写入 checker_scripts 的 name、source，再在新题目版本中引用其 ID、设置参数与可选 checker 限额，按正式种子的顺序写入语言、用例并发布。name 供维护识别，Worker 不用它查找本地文件。

源码是 UTF-8 字节。使用 SQL 字符串时须处理引号和反斜杠；也可用十六进制字面量保留原始文件。下面在仓库根目录生成更新脚本 ID 2 的 SQL，再用具备写权限的账户执行：

```sh
ACORN_CHECKER_SQL=$(mktemp)
python3 - > "$ACORN_CHECKER_SQL" <<'PY'
from pathlib import Path

source = Path("examples/checkers/float.py").read_bytes().hex()
print("UPDATE checker_scripts SET source = X'" + source + "' WHERE id = 2;")
PY
mysql --host=127.0.0.1 --port=3306 --user=example_initializer --password \
  --ssl-mode=DISABLED --database=appdb < "$ACORN_CHECKER_SQL"
rm -- "$ACORN_CHECKER_SQL"
```

同一脚本可被多个题目版本引用，更新源码会影响它们之后重新装配的任务；已装配的源码副本保持不变。参数和 checker 限额属于题目版本，新增行为需要新的参数或引用时，创建新版本。新提交、queued 重交接和启动装入都会按固定题目版本读取当时的脚本。脚本限额逐项采用版本覆盖值、同版本 Python 语言限额、未设限的顺序；Job 总期限、输出及其他沙箱预算始终生效。
