import sys

items = sys.stdin.read().split()
words = items[1 : int(items[0]) + 1]
for left, right in reversed(list(zip(words, words[1:]))):
    print(left, right)
