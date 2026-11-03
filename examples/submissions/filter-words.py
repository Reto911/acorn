import sys

items = sys.stdin.read().split()
n, k = map(int, items[:2])
words = [word for word in items[2 : n + 2] if len(word) >= k]
sys.stdout.write("".join(word + "\n" for word in reversed(words)))
