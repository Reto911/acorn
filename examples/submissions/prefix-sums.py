import sys

items = list(map(int, sys.stdin.read().split()))
total = 0
for value in items[1 : items[0] + 1]:
    total += value
    print(f"{total:+020d}")
