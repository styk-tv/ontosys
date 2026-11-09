def total(xs):
    s = 0
    for x in xs:
        s += x
    return s

def bsearch(xs, x):
    lo, hi = 0, len(xs) - 1
    while lo <= hi:
        mid = (lo + hi) // 2
        if xs[mid] == x:
            return mid
        if xs[mid] < x:
            lo = mid + 1
        else:
            hi = mid - 1
    return -1

def merge(a, b):
    out = [0] * (len(a) + len(b))
    for i in range(len(a)):
        out[i] = a[i]
    for j in range(len(b)):
        out[len(a) + j] = b[j]
    return out

def msort(xs):
    if len(xs) < 2:
        return xs
    mid = len(xs) // 2
    return merge(msort(xs[:mid]), msort(xs[mid:]))

def bubble(a, n):
    for i in range(n):
        for j in range(n - i - 1):
            if a[j] > a[j + 1]:
                a[j], a[j + 1] = a[j + 1], a[j]

def fib(n):
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)

@lru_cache(maxsize=None)
def mfib(n):
    if n < 2:
        return n
    return mfib(n - 1) + mfib(n - 2)

def pairs(xs):
    c = 0
    for x in xs:
        for y in xs:
            if x == y:
                c += 1
    return c

def ten():
    s = 0
    for i in range(10):
        s += i
    return s

def first(xs):
    i = 0
    while True:
        if xs[i]:
            break
        i += 1
    return i

def dup(xs, ys):
    c = 0
    for x in xs:
        if x in ys:
            c += 1
    return c

def sorts(rows):
    for row in rows:
        row.sort()

def find(xs, x):
    for i in range(len(xs)):
        if xs[i] == x:
            return i
    return -1

def grow(n):
    i = 0
    while i < n:
        if i == 3:
            n += 1
        i += 1

def t3(n):
    if n < 1:
        return 0
    return t3(n // 2) + t3(n // 2) + t3(n // 2)

def u(n):
    return frobnicate(n)

def p(n):
    return u(n)

def e():
    pass
