"""Zode capability benchmark suite.

Each task asks a model to implement one Python function with a fixed name.
Scoring is objective: the runner exec()s the candidate code, then exec()s the
`test` (a series of asserts) in the same namespace. Pass = no exception.

Tasks span six dimensions so the benchmark measures *various* capabilities,
not just one style of problem:
  algorithms, strings, data-structures, math, parsing, edge-cases.

`kind` is "humaneval" for short function-completion (HumanEval-style) and
"curated" for richer specs with adversarial tests.
"""

TASKS = [
    # ---------------------------------------------------------------- algorithms
    {
        "id": "two_sum",
        "dim": "algorithms",
        "kind": "humaneval",
        "fn": "two_sum",
        "prompt": "Write a Python function `two_sum(nums, target)` that returns a list of the two indices `[i, j]` (i < j) such that nums[i] + nums[j] == target. Exactly one solution exists. Respond with only the function in one ```python code block.",
        "test": """
assert sorted(two_sum([2,7,11,15], 9)) == [0,1]
assert sorted(two_sum([3,2,4], 6)) == [1,2]
assert sorted(two_sum([3,3], 6)) == [0,1]
assert sorted(two_sum([-1,-2,-3,-4,-5], -8)) == [2,4]
""",
    },
    {
        "id": "lis_length",
        "dim": "algorithms",
        "kind": "curated",
        "fn": "lis_length",
        "prompt": "Write a Python function `lis_length(nums)` returning the length of the longest strictly increasing subsequence of `nums`. Handle the empty list (return 0). Aim for better than O(n^2). Respond with only the function in one ```python code block.",
        "test": """
assert lis_length([]) == 0
assert lis_length([7]) == 1
assert lis_length([10,9,2,5,3,7,101,18]) == 4
assert lis_length([0,1,0,3,2,3]) == 4
assert lis_length([7,7,7,7]) == 1
assert lis_length(list(range(1000))) == 1000
""",
    },
    {
        "id": "search_insert",
        "dim": "algorithms",
        "kind": "humaneval",
        "fn": "search_insert",
        "prompt": "Write a Python function `search_insert(nums, target)` for a sorted ascending list `nums` that returns the index where `target` is, or where it would be inserted to keep the list sorted (leftmost). Respond with only the function in one ```python code block.",
        "test": """
assert search_insert([1,3,5,6], 5) == 2
assert search_insert([1,3,5,6], 2) == 1
assert search_insert([1,3,5,6], 7) == 4
assert search_insert([1,3,5,6], 0) == 0
assert search_insert([], 5) == 0
assert search_insert([1,1,1], 1) == 0
""",
    },
    {
        "id": "merge_intervals",
        "dim": "algorithms",
        "kind": "curated",
        "fn": "merge_intervals",
        "prompt": "Write a Python function `merge_intervals(intervals)` that takes a list of [start,end] intervals and returns the list of merged, non-overlapping intervals sorted by start. Touching intervals like [1,2],[2,3] merge into [1,3]. Respond with only the function in one ```python code block.",
        "test": """
assert merge_intervals([[1,3],[2,6],[8,10],[15,18]]) == [[1,6],[8,10],[15,18]]
assert merge_intervals([[1,4],[4,5]]) == [[1,5]]
assert merge_intervals([]) == []
assert merge_intervals([[1,4],[0,4]]) == [[0,4]]
assert merge_intervals([[1,4],[2,3]]) == [[1,4]]
""",
    },
    {
        "id": "kth_largest",
        "dim": "algorithms",
        "kind": "humaneval",
        "fn": "kth_largest",
        "prompt": "Write a Python function `kth_largest(nums, k)` returning the k-th largest element (1-indexed; k=1 is the maximum). Duplicates count by position, so [3,2,3,1,2,4,5,5,6] with k=4 returns 4. Respond with only the function in one ```python code block.",
        "test": """
assert kth_largest([3,2,1,5,6,4], 2) == 5
assert kth_largest([3,2,3,1,2,4,5,5,6], 4) == 4
assert kth_largest([1], 1) == 1
assert kth_largest([7,7,7], 2) == 7
""",
    },
    # ------------------------------------------------------------------- strings
    {
        "id": "is_palindrome",
        "dim": "strings",
        "kind": "humaneval",
        "fn": "is_palindrome",
        "prompt": "Write a Python function `is_palindrome(s)` that returns True if `s` is a palindrome considering only alphanumeric characters and ignoring case. An empty/punctuation-only string is a palindrome. Respond with only the function in one ```python code block.",
        "test": """
assert is_palindrome("A man, a plan, a canal: Panama") is True
assert is_palindrome("race a car") is False
assert is_palindrome("") is True
assert is_palindrome(".,") is True
assert is_palindrome("0P") is False
""",
    },
    {
        "id": "top_k_frequent_words",
        "dim": "strings",
        "kind": "curated",
        "fn": "top_k_frequent_words",
        "prompt": "Write a Python function `top_k_frequent_words(text, k)` that returns the k most frequent whitespace-separated words in `text`, most frequent first; ties broken alphabetically. Comparison is case-sensitive. Return a list of words. Respond with only the function in one ```python code block.",
        "test": """
assert top_k_frequent_words("the day is the night", 2) == ["the", "day"]
assert top_k_frequent_words("a a b b c", 2) == ["a", "b"]
assert top_k_frequent_words("z z y y x", 3) == ["y", "z", "x"]
assert top_k_frequent_words("", 3) == []
assert top_k_frequent_words("one two two three three three", 1) == ["three"]
""",
    },
    {
        "id": "roman_to_int",
        "dim": "strings",
        "kind": "humaneval",
        "fn": "roman_to_int",
        "prompt": "Write a Python function `roman_to_int(s)` converting a valid Roman numeral string (I,V,X,L,C,D,M with subtractive notation) to its integer value. Respond with only the function in one ```python code block.",
        "test": """
assert roman_to_int("III") == 3
assert roman_to_int("IV") == 4
assert roman_to_int("IX") == 9
assert roman_to_int("LVIII") == 58
assert roman_to_int("MCMXCIV") == 1994
assert roman_to_int("MMMCMXCIX") == 3999
""",
    },
    {
        "id": "longest_common_prefix",
        "dim": "strings",
        "kind": "humaneval",
        "fn": "longest_common_prefix",
        "prompt": "Write a Python function `longest_common_prefix(strs)` returning the longest common prefix string among a list of strings, or \"\" if none. Respond with only the function in one ```python code block.",
        "test": """
assert longest_common_prefix(["flower","flow","flight"]) == "fl"
assert longest_common_prefix(["dog","racecar","car"]) == ""
assert longest_common_prefix([]) == ""
assert longest_common_prefix(["a"]) == "a"
assert longest_common_prefix(["abc","abc"]) == "abc"
assert longest_common_prefix(["", "abc"]) == ""
""",
    },
    {
        "id": "run_length_encode",
        "dim": "strings",
        "kind": "curated",
        "fn": "run_length_encode",
        "prompt": "Write a Python function `run_length_encode(s)` that returns the run-length encoding of `s` as a string where each run is the character followed by its count (always include the count, even 1). E.g. 'aaabbc' -> 'a3b2c1'. Empty string -> ''. Respond with only the function in one ```python code block.",
        "test": """
assert run_length_encode("aaabbc") == "a3b2c1"
assert run_length_encode("") == ""
assert run_length_encode("a") == "a1"
assert run_length_encode("aAbB") == "a1A1b1B1"
assert run_length_encode("xxxxxxxxxx") == "x10"
""",
    },
    # ----------------------------------------------------------- data-structures
    {
        "id": "lru_cache",
        "dim": "data-structures",
        "kind": "curated",
        "fn": "LRUCache",
        "prompt": "Write a Python class `LRUCache` with `__init__(self, capacity)`, `get(self, key)` (returns the value or -1 if absent), and `put(self, key, value)`. It evicts the least-recently-used item when over capacity. get and put both count as a use. Respond with only the class in one ```python code block.",
        "test": """
c = LRUCache(2)
c.put(1,1); c.put(2,2)
assert c.get(1) == 1
c.put(3,3)            # evicts key 2
assert c.get(2) == -1
c.put(4,4)            # evicts key 1
assert c.get(1) == -1
assert c.get(3) == 3
assert c.get(4) == 4
""",
    },
    {
        "id": "min_stack",
        "dim": "data-structures",
        "kind": "curated",
        "fn": "MinStack",
        "prompt": "Write a Python class `MinStack` supporting `push(self, x)`, `pop(self)`, `top(self)`, and `get_min(self)` — all O(1). `get_min` returns the minimum element currently in the stack. Respond with only the class in one ```python code block.",
        "test": """
s = MinStack()
s.push(-2); s.push(0); s.push(-3)
assert s.get_min() == -3
s.pop()
assert s.top() == 0
assert s.get_min() == -2
s.push(-5)
assert s.get_min() == -5
""",
    },
    {
        "id": "valid_parentheses",
        "dim": "data-structures",
        "kind": "humaneval",
        "fn": "valid_parentheses",
        "prompt": "Write a Python function `valid_parentheses(s)` returning True iff every '(', '[', '{' is correctly closed by the matching ')',']','}' in the right order. Respond with only the function in one ```python code block.",
        "test": """
assert valid_parentheses("()[]{}") is True
assert valid_parentheses("(]") is False
assert valid_parentheses("([)]") is False
assert valid_parentheses("{[]}") is True
assert valid_parentheses("") is True
assert valid_parentheses("(") is False
""",
    },
    {
        "id": "flatten",
        "dim": "data-structures",
        "kind": "curated",
        "fn": "flatten",
        "prompt": "Write a Python function `flatten(nested)` that flattens an arbitrarily nested list of integers (lists within lists, any depth) into a single flat list, preserving left-to-right order. Non-list elements are kept as-is. Respond with only the function in one ```python code block.",
        "test": """
assert flatten([1,[2,[3,[4]],5]]) == [1,2,3,4,5]
assert flatten([]) == []
assert flatten([[],[1],[]]) == [1]
assert flatten([1,2,3]) == [1,2,3]
assert flatten([[[[1]]]]) == [1]
""",
    },
    {
        "id": "group_anagrams",
        "dim": "data-structures",
        "kind": "curated",
        "fn": "group_anagrams",
        "prompt": "Write a Python function `group_anagrams(words)` that groups anagrams together. Return a list of groups; each group is a list of words; sort each group's words ascending and sort the groups by their first word ascending. Respond with only the function in one ```python code block.",
        "test": """
assert group_anagrams(["eat","tea","tan","ate","nat","bat"]) == [["ate","eat","tea"],["bat"],["nat","tan"]]
assert group_anagrams([]) == []
assert group_anagrams(["a"]) == [["a"]]
assert group_anagrams(["ab","ba","abc"]) == [["ab","ba"],["abc"]]
""",
    },
    # ---------------------------------------------------------------------- math
    {
        "id": "gcd",
        "dim": "math",
        "kind": "humaneval",
        "fn": "gcd",
        "prompt": "Write a Python function `gcd(a, b)` returning the greatest common divisor of two non-negative integers (gcd(0,0)=0, gcd(0,n)=n). Respond with only the function in one ```python code block.",
        "test": """
assert gcd(12, 8) == 4
assert gcd(0, 5) == 5
assert gcd(5, 0) == 5
assert gcd(0, 0) == 0
assert gcd(17, 13) == 1
assert gcd(100, 80) == 20
""",
    },
    {
        "id": "primes_upto",
        "dim": "math",
        "kind": "curated",
        "fn": "primes_upto",
        "prompt": "Write a Python function `primes_upto(n)` returning the sorted list of all primes <= n using an efficient sieve. n may be < 2 (return []). Respond with only the function in one ```python code block.",
        "test": """
assert primes_upto(10) == [2,3,5,7]
assert primes_upto(1) == []
assert primes_upto(2) == [2]
assert primes_upto(0) == []
assert primes_upto(20) == [2,3,5,7,11,13,17,19]
assert len(primes_upto(1000)) == 168
""",
    },
    {
        "id": "is_power_of_two",
        "dim": "math",
        "kind": "humaneval",
        "fn": "is_power_of_two",
        "prompt": "Write a Python function `is_power_of_two(n)` returning True iff n is a positive power of two (1,2,4,8,...). Zero and negatives are False. Respond with only the function in one ```python code block.",
        "test": """
assert is_power_of_two(1) is True
assert is_power_of_two(16) is True
assert is_power_of_two(0) is False
assert is_power_of_two(3) is False
assert is_power_of_two(-4) is False
assert is_power_of_two(1024) is True
""",
    },
    {
        "id": "int_to_roman",
        "dim": "math",
        "kind": "curated",
        "fn": "int_to_roman",
        "prompt": "Write a Python function `int_to_roman(n)` converting an integer 1..3999 to its Roman numeral string with standard subtractive notation. Respond with only the function in one ```python code block.",
        "test": """
assert int_to_roman(3) == "III"
assert int_to_roman(4) == "IV"
assert int_to_roman(9) == "IX"
assert int_to_roman(58) == "LVIII"
assert int_to_roman(1994) == "MCMXCIV"
assert int_to_roman(3999) == "MMMCMXCIX"
""",
    },
    {
        "id": "add_binary",
        "dim": "math",
        "kind": "humaneval",
        "fn": "add_binary",
        "prompt": "Write a Python function `add_binary(a, b)` that takes two binary number strings and returns their sum as a binary string (no '0b' prefix, no leading zeros except '0' itself). Respond with only the function in one ```python code block.",
        "test": """
assert add_binary("11", "1") == "100"
assert add_binary("1010", "1011") == "10101"
assert add_binary("0", "0") == "0"
assert add_binary("1", "111") == "1000"
""",
    },
    # ------------------------------------------------------------------- parsing
    {
        "id": "parse_query",
        "dim": "parsing",
        "kind": "curated",
        "fn": "parse_query",
        "prompt": "Write a Python function `parse_query(qs)` that parses a URL query string like 'a=1&b=2&a=3' into a dict mapping each key to a list of its values in order: {'a':['1','3'],'b':['2']}. A key with no '=' maps to ['']. Empty input -> {}. Do not URL-decode. Respond with only the function in one ```python code block.",
        "test": """
assert parse_query("a=1&b=2&a=3") == {"a":["1","3"],"b":["2"]}
assert parse_query("") == {}
assert parse_query("x") == {"x":[""]}
assert parse_query("k=") == {"k":[""]}
assert parse_query("a=1&a=2&a=3") == {"a":["1","2","3"]}
""",
    },
    {
        "id": "eval_expr",
        "dim": "parsing",
        "kind": "curated",
        "fn": "eval_expr",
        "prompt": "Write a Python function `eval_expr(s)` that evaluates an arithmetic expression string with + - * / and parentheses over integers, honoring precedence and using truncating integer division toward zero for '/'. Whitespace is ignored. Do NOT use eval(). Return an int. Respond with only the function in one ```python code block.",
        "test": """
assert eval_expr("1 + 2 * 3") == 7
assert eval_expr("(1 + 2) * 3") == 9
assert eval_expr("10 / 3") == 3
assert eval_expr("7 - 2 - 3") == 2
assert eval_expr("2 * (3 + (4 - 1))") == 12
assert eval_expr("100") == 100
assert eval_expr("(8 / (2 + 2))") == 2
""",
    },
    {
        "id": "parse_ini",
        "dim": "parsing",
        "kind": "curated",
        "fn": "parse_ini",
        "prompt": "Write a Python function `parse_ini(text)` parsing a simple INI: lines like '[section]' start a section; 'key = value' lines add to the current section (strip whitespace around key and value); blank lines and lines starting with ';' or '#' are ignored; keys before any section go under section ''. Return a dict of dicts. Respond with only the function in one ```python code block.",
        "test": """
ini = "g=0\\n[a]\\nx = 1\\n; comment\\ny=2\\n\\n[b]\\nz =  three  "
assert parse_ini(ini) == {"":{"g":"0"},"a":{"x":"1","y":"2"},"b":{"z":"three"}}
assert parse_ini("") == {}
assert parse_ini("# only comment") == {}
""",
    },
    {
        "id": "csv_parse_row",
        "dim": "parsing",
        "kind": "curated",
        "fn": "csv_parse_row",
        "prompt": "Write a Python function `csv_parse_row(line)` that parses ONE CSV line into a list of field strings, supporting double-quoted fields that may contain commas and escaped quotes (\"\" inside a quoted field means a literal \"). Surrounding quotes are removed. Do not use the csv module. Respond with only the function in one ```python code block.",
        "test": '''
assert csv_parse_row('a,b,c') == ['a','b','c']
assert csv_parse_row('a,"b,c",d') == ['a','b,c','d']
assert csv_parse_row('"he said ""hi""",x') == ['he said "hi"','x']
assert csv_parse_row('') == ['']
assert csv_parse_row('a,,c') == ['a','','c']
''',
    },
    {
        "id": "tokenize",
        "dim": "parsing",
        "kind": "curated",
        "fn": "tokenize",
        "prompt": "Write a Python function `tokenize(s)` that splits a simple expression into tokens: integer literals (one or more digits), identifiers (letter or _ then letters/digits/_), and single-char operators among + - * / ( ). Whitespace separates but is not a token. Return a list of token strings in order. Respond with only the function in one ```python code block.",
        "test": """
assert tokenize("x1 + 23*foo") == ["x1","+","23","*","foo"]
assert tokenize("(a)") == ["(","a",")"]
assert tokenize("") == []
assert tokenize("  12   ") == ["12"]
assert tokenize("a_b/c") == ["a_b","/","c"]
""",
    },
    # ---------------------------------------------------------------- edge-cases
    {
        "id": "my_atoi",
        "dim": "edge-cases",
        "kind": "curated",
        "fn": "my_atoi",
        "prompt": "Write a Python function `my_atoi(s)` implementing string-to-int like C atoi: skip leading whitespace, optional single +/- sign, read digits until a non-digit, ignore the rest. No digits -> 0. Clamp the result to the 32-bit signed range [-2147483648, 2147483647]. Respond with only the function in one ```python code block.",
        "test": """
assert my_atoi("42") == 42
assert my_atoi("   -42") == -42
assert my_atoi("4193 with words") == 4193
assert my_atoi("words and 987") == 0
assert my_atoi("-91283472332") == -2147483648
assert my_atoi("2147483648") == 2147483647
assert my_atoi("+1") == 1
assert my_atoi("") == 0
""",
    },
    {
        "id": "spiral_order",
        "dim": "edge-cases",
        "kind": "curated",
        "fn": "spiral_order",
        "prompt": "Write a Python function `spiral_order(matrix)` returning all elements of an m x n matrix (list of lists) in clockwise spiral order, starting top-left. Handle empty and non-square matrices. Respond with only the function in one ```python code block.",
        "test": """
assert spiral_order([[1,2,3],[4,5,6],[7,8,9]]) == [1,2,3,6,9,8,7,4,5]
assert spiral_order([[1,2,3,4],[5,6,7,8],[9,10,11,12]]) == [1,2,3,4,8,12,11,10,9,5,6,7]
assert spiral_order([]) == []
assert spiral_order([[1]]) == [1]
assert spiral_order([[1,2,3]]) == [1,2,3]
assert spiral_order([[1],[2],[3]]) == [1,2,3]
""",
    },
    {
        "id": "rotate_right",
        "dim": "edge-cases",
        "kind": "humaneval",
        "fn": "rotate_right",
        "prompt": "Write a Python function `rotate_right(arr, k)` that returns a NEW list with `arr` rotated to the right by k positions. k may be larger than len(arr) or negative (negative rotates left). Do not mutate the input. Respond with only the function in one ```python code block.",
        "test": """
assert rotate_right([1,2,3,4,5], 2) == [4,5,1,2,3]
assert rotate_right([1,2,3,4,5], 7) == [4,5,1,2,3]
assert rotate_right([1,2,3], 0) == [1,2,3]
assert rotate_right([1,2,3], -1) == [2,3,1]
assert rotate_right([], 3) == []
assert rotate_right([1], 100) == [1]
""",
    },
    {
        "id": "summarize_ranges",
        "dim": "edge-cases",
        "kind": "curated",
        "fn": "summarize_ranges",
        "prompt": "Write a Python function `summarize_ranges(nums)` taking a sorted list of distinct integers and returning a list of strings summarizing consecutive ranges: a single number 'a', a run 'a->b'. E.g. [0,1,2,4,5,7] -> ['0->2','4->5','7']. Empty -> []. Respond with only the function in one ```python code block.",
        "test": """
assert summarize_ranges([0,1,2,4,5,7]) == ['0->2','4->5','7']
assert summarize_ranges([]) == []
assert summarize_ranges([0]) == ['0']
assert summarize_ranges([0,2,3,4,6,8,9]) == ['0','2->4','6','8->9']
assert summarize_ranges([-3,-2,-1,1]) == ['-3->-1','1']
""",
    },
    {
        "id": "jump_game",
        "dim": "edge-cases",
        "kind": "humaneval",
        "fn": "can_jump",
        "prompt": "Write a Python function `can_jump(nums)` where nums[i] is the max jump length from index i. Return True iff you can reach the last index starting from index 0. Respond with only the function in one ```python code block.",
        "test": """
assert can_jump([2,3,1,1,4]) is True
assert can_jump([3,2,1,0,4]) is False
assert can_jump([0]) is True
assert can_jump([1,0,1,0]) is False
assert can_jump([2,0,0]) is True
""",
    },
    {
        "id": "merge_sorted",
        "dim": "algorithms",
        "kind": "humaneval",
        "fn": "merge_sorted",
        "prompt": "Write a Python function `merge_sorted(a, b)` that merges two ascending-sorted lists into one ascending-sorted list (stable, keeps duplicates). Respond with only the function in one ```python code block.",
        "test": """
assert merge_sorted([1,3,5],[2,4,6]) == [1,2,3,4,5,6]
assert merge_sorted([],[1,2]) == [1,2]
assert merge_sorted([1,1],[1,1]) == [1,1,1,1]
assert merge_sorted([5],[]) == [5]
""",
    },
]


def by_dimension():
    dims = {}
    for t in TASKS:
        dims.setdefault(t["dim"], []).append(t)
    return dims


if __name__ == "__main__":
    print(f"{len(TASKS)} tasks across {len(by_dimension())} dimensions:")
    for dim, tasks in sorted(by_dimension().items()):
        print(f"  {dim}: {len(tasks)}")
