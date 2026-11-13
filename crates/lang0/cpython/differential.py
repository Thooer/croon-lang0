"""CPython 3.12 差分的程序与参照结果。

重跑：cargo test -p lang0 cpython -- --nocapture
缺少 python3.12 或版本不符时测试失败；经 croon.Driver 给两侧同一份结果。
"""
import json
import sys

import croon

if sys.implementation.name != "cpython" or sys.version_info[:2] != (3, 12):
    sys.exit("差分要 CPython 3.12")

HEADER = """from typing import Any
from croon import Port, Inbox, receive, kind, data, send, deliver, read_json, read_text
P = Port({'description':'p','in':{},'attach':{'additionalProperties':False},'out':{'value':{},'attach':{'additionalProperties':False}}})
Q = Port({'description':'q','in':{},'attach':{'additionalProperties':False}})
def main(p: P, inbox: Inbox(Q)) -> Any:
"""
PROGRAMS = {
    "text": """values = [None, True, False, 0, -123456789012345678901234567890, 1.0, -0.0, 1e-05, 0.0001, 1000000000000000.0, 1e+16, 1.2345678901234567, float('inf'), float('-inf'), float('nan')]
return [[str(x), f'{x}', str([x])] for x in values]""",
    "repr": """values = ["a'b", 'a"b', '\\n\\r\\t\\\\\\x00\\x1f\\x7f\\xa0\\u2028\\u200d\\ue000😀', [], {}, (), (1,), {'b': [1, 'x'], 'a': (2, 3)}]
return [[str(x), f'{x}', str([x])] for x in values]""",
    "dict_order": """d = {'b': 1, 'a': 2, 'b': 3}
d.update({'c': 4, 'a': 5})
d.pop('b')
d['b'] = 6
return [list(d), list(d.values()), str(list(d.items())), str({**d, 'a': 7, 'z': 8}), str({True: 1, 1: 2, 1.0: 3})]""",
    "builtin_forms": """return [str(), str(12), int(), int('12'), float(), float('1.5'), bool(), bool([]), list(), list(range(3)), dict(), dict(a=1), dict([('x', 1)]), isinstance(True, int), range(1, 3, 1)[1], min([1], key=None), min([], default=0), min(1, 2, 3, key=None), max(2, 1), max([2], default=0, key=None), sorted([2, 1], key=None, reverse=True), str(list(enumerate([1], start=1))), str(list(zip([1], [2], strict=True))), any([0, 1]), all([1, 2]), sum([1, 2]), sum([1], 2), abs(-2)]""",
    "method_forms": """d = dict(a=1)
x = [d.get('a'), d.get('z'), d.setdefault('b', 2), d.setdefault('y')]
d.update([('c', 3)])
d.update(e=4)
x += [list(d.keys()), list(d.values()), str(list(d.items())), d.pop('c'), d.pop('z', 0)]
d.clear()
x.append(len(d))
a = []
a.append(1)
a.extend([2])
x += [a.index(2), a.index(1, 0, 1), a.pop()]
a.clear()
x.append(a)
x += ['-'.join(['a', 'b']), 'a b'.split(), 'a,b'.split(',', 1), 'a b c'.split(None, 1), ' x '.strip(), 'xa'.strip('x'), 'abc'.startswith('a'), 'ab'.startswith('a', 0, 1), 'abc'.endswith('c'), 'ab'.endswith('b', None, None), 'abc'.replace('b', 'B'), 'aa'.replace('a', 'b', 1), '{} {x}'.format(1, x=2), 'abc'.index('b'), 'ab'.index('b', 0, 2), (1, 2).index(2), (1,).index(1, 0, 1)]
return x""",
    "selection_callbacks": """def key(x):
    return -x
return [sorted([2, 1, 3], key=key), min([2, 1, 3], key=key), max([], default=9)]""",
    "comprehension_order": """xs = [send(p, ['outer', i]).value for i in range(3) if send(p, ['filter', i]).value]
d = {str(send(p, ['key', i, j]).value): send(p, ['value', i, j]).value for i in range(2) for j in range(2) if send(p, ['keep', i, j]).value}
return [xs, str(d)]""",
    "division": """pairs = [(7, 3), (-7, 3), (7, -3), (-7, -3), (7.0, 3), (-7.0, 3), (7, -3.0), (-7.0, -3.0), (1.0, 0.1), (-1.0, 0.1), (0.0, -3.0), (-0.0, 3.0)]
return [[str(a // b), str(a % b)] for a, b in pairs]""",
    "short_circuit": """return [False and send(p, 'never').value, True or send(p, 'never').value, send(p, 'first').value and send(p, 'second').value, send(p, 'third').value or send(p, 'never').value, 3 < send(p, 'comparison').value < send(p, 'last').value, send(p, 'yes').value if send(p, 'condition').value else send(p, 'never').value]""",
    "exceptions": """values = [ValueError(), ValueError('x'), ValueError('a', 1), KeyError('a'), KeyError(1), KeyError('a', 2)]
texts = [[kind(e), str(e), str([e])] for e in values]
try:
    {}['missing']
except KeyError as e:
    texts.append([kind(e), str(e)])
try:
    1 / 0
except ZeroDivisionError as e:
    texts.append([kind(e), str(e)])
return texts""",
    "large_int": """n = int('1234567890123456789012345678901234567890')
return [str(n * n), str(-n // 7), str(-n % 7), str(n / n), str(int(str(n))), str(n + 1), n > 1e+30, n == float(str(n))]""",
    "unicode": """spaces = ['\\t', '\\n', '\\x1c', '\\x85', '\\xa0', '\\u1680', '\\u2000', '\\u2007', '\\u2028', '\\u2029', '\\u202f', '\\u205f', '\\u3000']
texts = [[s.strip(), ('a' + s + 'b').split(), ('a' + s + ' b c').split(None, 1)] for s in spaces]
nums = [[int(s), str(float(s))] for s in [' 12 ', '\\u2003-١_٢\\u3000', '+１２３', '1_234']]
return [texts, nums, [str(float(s)) for s in ['١٢.٥', '\\u2003-１.２５e+２\\u3000', '+Infinity', '-INF', 'NaN']]]""",
    "conversion_errors": """texts = []
for s in ['1__2', '_12', '12_', '1 2', '²', '１２a'] + [s + '12' for s in ['\\x1c', '\\x1d', '\\x1e', '\\x1f']] + ['12' + s for s in ['\\x1c', '\\x1d', '\\x1e', '\\x1f']]:
    for convert in [int, float]:
        try:
            convert(s)
        except ValueError as e:
            texts.append([kind(e), str(e)])
return texts""",
    "failure": """send(p, {'before': 'failure'}).value
raise ValueError('a', 1)""",
    "scripted_failure": """try:
    send(p, 'fail').value
except Exception as e:
    return [kind(e), str(e), data(e)]""", 
    "falsy_results": """return [send(p, 'zero').value and send(p, 'never').value, send(p, 'empty').value or send(p, 'fallback').value, 10 < send(p, 'comparison').value < send(p, 'never').value]""",
    "receive": """a = receive(inbox, sender='i-s', via=['h-a', 'h-b'], timeout=2)
b = receive([inbox], timeout=0)
return [a.value, a.sender, a.via, b]""",
}

cases = []
for name, body in PROGRAMS.items():
    source = HEADER + ''.join('    ' + line + '\n' for line in body.splitlines())
    results = {str(i): {'outcome': {'ok': True, 'value': i + 1, 'dependencies': []}, 'attach': {}, 'derived': {}} for i in range(40)}
    if name == 'scripted_failure':
        results['0'] = {'outcome': {'ok': False, 'error': {'kind': 'use', 'message': 'scripted failure', 'data': {'x': 1}}, 'dependencies': ['artifact']}, 'attach': {}, 'derived': {}}
    if name == 'falsy_results':
        results['0']['outcome']['value'] = 0
        results['1']['outcome']['value'] = []
    if name == 'receive':
        results = {'0': {'arrival': {'port': 'inbox', 'sender': 'i-s', 'via': 'h-a', 'delivery': {'value': {'x': 1}, 'dependencies': []}, 'attach': {}}}, '1': {'arrival': None}}
    croon.DRIVER = croon.Driver(results)
    scope = {}
    exec(compile(source, '<' + name + '>', 'exec'), scope)
    try:
        value = scope['main'](croon.Ref({'port': 'p'}), croon.InboxRef('inbox'))
        end = {'ok': True, 'value': value, 'dependencies': []}
    except BaseException as e:
        end = {'ok': False, 'error': {'kind': croon.kind(e), 'message': str(e), 'data': croon.data(e)}, 'dependencies': []}
    cases.append({'name': name, 'source': source, 'results': results, 'expected': {'atoms': croon.DRIVER.atoms, 'done': end}})
json.dump(cases, sys.stdout, ensure_ascii=False)
