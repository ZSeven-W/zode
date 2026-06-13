"""Claude's direct solutions to the benchmark suite (the comparison baseline).

These were written by Claude solving each task directly (no Zode harness, no
DeepSeek). The runner scores them with the SAME hidden tests as the model
outputs, so the README table is apples-to-apples. They also validate that the
tests themselves are correct: a correct solution must pass.
"""

SOLUTIONS = {
    "two_sum": '''
def two_sum(nums, target):
    seen = {}
    for i, n in enumerate(nums):
        if target - n in seen:
            return [seen[target - n], i]
        seen[n] = i
''',
    "lis_length": '''
import bisect
def lis_length(nums):
    tails = []
    for x in nums:
        i = bisect.bisect_left(tails, x)
        if i == len(tails):
            tails.append(x)
        else:
            tails[i] = x
    return len(tails)
''',
    "search_insert": '''
import bisect
def search_insert(nums, target):
    return bisect.bisect_left(nums, target)
''',
    "merge_intervals": '''
def merge_intervals(intervals):
    if not intervals:
        return []
    res = []
    for s, e in sorted(intervals):
        if res and s <= res[-1][1]:
            res[-1][1] = max(res[-1][1], e)
        else:
            res.append([s, e])
    return res
''',
    "kth_largest": '''
def kth_largest(nums, k):
    return sorted(nums, reverse=True)[k - 1]
''',
    "is_palindrome": '''
def is_palindrome(s):
    t = [c.lower() for c in s if c.isalnum()]
    return t == t[::-1]
''',
    "top_k_frequent_words": '''
from collections import Counter
def top_k_frequent_words(text, k):
    c = Counter(text.split())
    ordered = sorted(c.items(), key=lambda kv: (-kv[1], kv[0]))
    return [w for w, _ in ordered][:k]
''',
    "roman_to_int": '''
def roman_to_int(s):
    vals = {'I':1,'V':5,'X':10,'L':50,'C':100,'D':500,'M':1000}
    total, prev = 0, 0
    for ch in reversed(s):
        v = vals[ch]
        total += v if v >= prev else -v
        prev = v
    return total
''',
    "longest_common_prefix": '''
def longest_common_prefix(strs):
    if not strs:
        return ""
    pre = strs[0]
    for s in strs[1:]:
        while not s.startswith(pre):
            pre = pre[:-1]
            if not pre:
                return ""
    return pre
''',
    "run_length_encode": '''
def run_length_encode(s):
    if not s:
        return ""
    out, prev, cnt = [], s[0], 1
    for ch in s[1:]:
        if ch == prev:
            cnt += 1
        else:
            out.append(f"{prev}{cnt}")
            prev, cnt = ch, 1
    out.append(f"{prev}{cnt}")
    return "".join(out)
''',
    "lru_cache": '''
from collections import OrderedDict
class LRUCache:
    def __init__(self, capacity):
        self.cap = capacity
        self.d = OrderedDict()
    def get(self, key):
        if key not in self.d:
            return -1
        self.d.move_to_end(key)
        return self.d[key]
    def put(self, key, value):
        if key in self.d:
            self.d.move_to_end(key)
        self.d[key] = value
        if len(self.d) > self.cap:
            self.d.popitem(last=False)
''',
    "min_stack": '''
class MinStack:
    def __init__(self):
        self.s = []
        self.m = []
    def push(self, x):
        self.s.append(x)
        self.m.append(x if not self.m else min(x, self.m[-1]))
    def pop(self):
        self.m.pop()
        return self.s.pop()
    def top(self):
        return self.s[-1]
    def get_min(self):
        return self.m[-1]
''',
    "valid_parentheses": '''
def valid_parentheses(s):
    pairs = {')':'(', ']':'[', '}':'{'}
    st = []
    for ch in s:
        if ch in '([{':
            st.append(ch)
        elif ch in pairs:
            if not st or st.pop() != pairs[ch]:
                return False
    return not st
''',
    "flatten": '''
def flatten(nested):
    out = []
    for x in nested:
        if isinstance(x, list):
            out.extend(flatten(x))
        else:
            out.append(x)
    return out
''',
    "group_anagrams": '''
def group_anagrams(words):
    groups = {}
    for w in words:
        groups.setdefault("".join(sorted(w)), []).append(w)
    res = [sorted(g) for g in groups.values()]
    res.sort(key=lambda g: g[0])
    return res
''',
    "gcd": '''
def gcd(a, b):
    while b:
        a, b = b, a % b
    return a
''',
    "primes_upto": '''
def primes_upto(n):
    if n < 2:
        return []
    sieve = [True] * (n + 1)
    sieve[0] = sieve[1] = False
    for i in range(2, int(n ** 0.5) + 1):
        if sieve[i]:
            for j in range(i * i, n + 1, i):
                sieve[j] = False
    return [i for i in range(2, n + 1) if sieve[i]]
''',
    "is_power_of_two": '''
def is_power_of_two(n):
    return n > 0 and (n & (n - 1)) == 0
''',
    "int_to_roman": '''
def int_to_roman(n):
    vals = [(1000,'M'),(900,'CM'),(500,'D'),(400,'CD'),(100,'C'),(90,'XC'),
            (50,'L'),(40,'XL'),(10,'X'),(9,'IX'),(5,'V'),(4,'IV'),(1,'I')]
    out = []
    for v, sym in vals:
        while n >= v:
            out.append(sym)
            n -= v
    return "".join(out)
''',
    "add_binary": '''
def add_binary(a, b):
    return bin(int(a, 2) + int(b, 2))[2:]
''',
    "parse_query": '''
def parse_query(qs):
    res = {}
    if not qs:
        return res
    for part in qs.split('&'):
        if '=' in part:
            k, v = part.split('=', 1)
        else:
            k, v = part, ''
        res.setdefault(k, []).append(v)
    return res
''',
    "eval_expr": '''
def eval_expr(s):
    s = s.replace(' ', '')
    pos = 0
    def peek():
        return s[pos] if pos < len(s) else ''
    def expr():
        nonlocal pos
        val = term()
        while peek() in ('+', '-'):
            op = s[pos]; pos += 1
            r = term()
            val = val + r if op == '+' else val - r
        return val
    def term():
        nonlocal pos
        val = factor()
        while peek() in ('*', '/'):
            op = s[pos]; pos += 1
            r = factor()
            val = val * r if op == '*' else int(val / r)
        return val
    def factor():
        nonlocal pos
        if peek() == '(':
            pos += 1
            val = expr()
            pos += 1  # skip ')'
            return val
        start = pos
        if peek() in ('+', '-'):
            pos += 1
        while pos < len(s) and s[pos].isdigit():
            pos += 1
        return int(s[start:pos])
    return expr()
''',
    "parse_ini": '''
def parse_ini(text):
    res = {}
    section = ''
    for raw in text.split('\\n'):
        line = raw.strip()
        if not line or line[0] in ';#':
            continue
        if line.startswith('[') and line.endswith(']'):
            section = line[1:-1].strip()
            res.setdefault(section, {})
        elif '=' in line:
            k, v = line.split('=', 1)
            res.setdefault(section, {})[k.strip()] = v.strip()
    return res
''',
    "csv_parse_row": '''
def csv_parse_row(line):
    fields, cur = [], []
    i, n = 0, len(line)
    while i < n:
        if line[i] == '"':
            i += 1
            while i < n:
                if line[i] == '"':
                    if i + 1 < n and line[i + 1] == '"':
                        cur.append('"'); i += 2
                    else:
                        i += 1; break
                else:
                    cur.append(line[i]); i += 1
        elif line[i] == ',':
            fields.append(''.join(cur)); cur = []; i += 1
        else:
            cur.append(line[i]); i += 1
    fields.append(''.join(cur))
    return fields
''',
    "tokenize": '''
def tokenize(s):
    tokens = []
    i, n = 0, len(s)
    while i < n:
        c = s[i]
        if c.isspace():
            i += 1
        elif c.isdigit():
            j = i
            while j < n and s[j].isdigit():
                j += 1
            tokens.append(s[i:j]); i = j
        elif c.isalpha() or c == '_':
            j = i
            while j < n and (s[j].isalnum() or s[j] == '_'):
                j += 1
            tokens.append(s[i:j]); i = j
        elif c in '+-*/()':
            tokens.append(c); i += 1
        else:
            i += 1
    return tokens
''',
    "my_atoi": '''
def my_atoi(s):
    i, n = 0, len(s)
    while i < n and s[i] == ' ':
        i += 1
    sign = 1
    if i < n and s[i] in '+-':
        sign = -1 if s[i] == '-' else 1
        i += 1
    start = i
    while i < n and s[i].isdigit():
        i += 1
    if start == i:
        return 0
    val = sign * int(s[start:i])
    return max(-2147483648, min(2147483647, val))
''',
    "spiral_order": '''
def spiral_order(matrix):
    if not matrix or not matrix[0]:
        return []
    res = []
    top, bottom = 0, len(matrix) - 1
    left, right = 0, len(matrix[0]) - 1
    while top <= bottom and left <= right:
        for c in range(left, right + 1):
            res.append(matrix[top][c])
        top += 1
        for r in range(top, bottom + 1):
            res.append(matrix[r][right])
        right -= 1
        if top <= bottom:
            for c in range(right, left - 1, -1):
                res.append(matrix[bottom][c])
            bottom -= 1
        if left <= right:
            for r in range(bottom, top - 1, -1):
                res.append(matrix[r][left])
            left += 1
    return res
''',
    "rotate_right": '''
def rotate_right(arr, k):
    n = len(arr)
    if n == 0:
        return []
    k %= n
    return arr[-k:] + arr[:-k] if k else arr[:]
''',
    "summarize_ranges": '''
def summarize_ranges(nums):
    res = []
    i, n = 0, len(nums)
    while i < n:
        start = nums[i]
        while i + 1 < n and nums[i + 1] == nums[i] + 1:
            i += 1
        res.append(str(start) if nums[i] == start else f"{start}->{nums[i]}")
        i += 1
    return res
''',
    "jump_game": '''
def can_jump(nums):
    reach = 0
    for i, x in enumerate(nums):
        if i > reach:
            return False
        reach = max(reach, i + x)
    return True
''',
    "merge_sorted": '''
def merge_sorted(a, b):
    i = j = 0
    out = []
    while i < len(a) and j < len(b):
        if a[i] <= b[j]:
            out.append(a[i]); i += 1
        else:
            out.append(b[j]); j += 1
    out.extend(a[i:])
    out.extend(b[j:])
    return out
''',
}
