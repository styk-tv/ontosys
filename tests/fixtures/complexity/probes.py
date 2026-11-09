from abc import abstractmethod


def heavy(x):
    return sorted(x)


def map_named(xs):
    return list(map(heavy, xs))


def map_param(f, xs):
    return list(map(f, xs))


def sort_key(xs):
    return sorted(xs, key=lambda x: x.lower())


def gen(xs):
    for x in xs:
        yield x


def first_of(xs):
    return next(gen(xs))


async def fetch_all(xs):
    for x in xs:
        heavy(x)


def start_fetch(xs):
    return fetch_all(xs)


def walk_tree(root):
    stack = [root]
    while stack:
        n = stack.pop()
        stack.extend(n.children)


def drain(stack):
    c = 0
    while stack:
        stack.pop()
        c += 1
    return c


class A:
    def remove(self, x):
        return None


class B:
    def remove(self, x):
        return None


def purge(xs, ys):
    for y in ys:
        xs.remove(y)


class Bag:
    def __contains__(self, x):
        for y in self.items:
            for z in self.items:
                if y == x and z == x:
                    return True
        return False


def member(bag, x):
    return x in bag


def count_down_py(n):
    t = 0
    for i in range(n, 0, -1):
        t += i
    return t


def twice_py(n):
    t = 0
    for i in range(2 * n):
        t += i
    return t


def from_k(xs, k, n):
    for i in range(k, n):
        xs[i] += 1


class Base:
    @abstractmethod
    def render(self, items):
        raise NotImplementedError


def render_all(objs, items):
    for o in objs:
        o.render(items)


def bfs_py(root):
    q = [root]
    i = 0
    while i < len(q):
        n = q[i]
        q.append(n)
        i += 1


def comp(xs):
    return [heavy(x) for x in xs]


def gen_sum(xs):
    return sum(len(heavy(x)) for x in xs)


def nested_comp(xs, ys):
    return [x for x in xs for y in ys if x == y]


def filtered_comp(xs):
    return [heavy(x) for x in xs if x]


def plain_loop(xs):
    out = []
    for x in xs:
        out.append(heavy(x))
    return out


def skip_py(xs, n):
    for x in xs:
        if x:
            continue
        for i in range(n):
            pass


def poke(o):
    return o.frobnicate()
