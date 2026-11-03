import sys

items = sys.stdin.read().split()
words = items[1 : int(items[0]) + 1]
print("\n".join(reversed(words)))
