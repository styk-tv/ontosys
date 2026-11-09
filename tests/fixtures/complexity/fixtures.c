#define ADD1(x) ((x) + 1)
#define SLOW(s) strlen(s)
#define LOOPY(n) do { for (int _i = 0; _i < (n); _i++) {} } while (0)
int sum(int *a, int n) { int s = 0; for (int i = 0; i < n; i++) s += a[i]; return s; }
int bs(int *a, int n, int x) { int lo = 0, hi = n - 1; while (lo <= hi) { int mid = (lo + hi) / 2; if (a[mid] == x) return mid; if (a[mid] < x) lo = mid + 1; else hi = mid - 1; } return -1; }
static void merge(int *a, int *t, int n, int mid) { int i = 0, j = mid, k = 0; while (k < n) { t[k] = (j >= n || (i < mid && a[i] <= a[j])) ? a[i++] : a[j++]; k++; } memcpy(a, t, n * sizeof(int)); }
void msort(int *a, int *t, int n) { if (n < 2) return; int mid = n / 2; msort(a, t, mid); msort(a + mid, t, n - mid); merge(a, t, n, mid); }
void bubble(int *a, int n) { for (int i = 0; i < n; i++) for (int j = 0; j < n - i - 1; j++) if (a[j] > a[j + 1]) { int t = a[j]; a[j] = a[j + 1]; a[j + 1] = t; } }
long fib(int n) { if (n < 2) return n; return fib(n - 1) + fib(n - 2); }
long mfib(int n, long *memo) { if (memo[n]) return memo[n]; if (n < 2) return n; return memo[n] = mfib(n - 1, memo) + mfib(n - 2, memo); }
int count_a(const char *s) { int c = 0; for (int i = 0; i < strlen(s); i++) if (s[i] == 'a') c++; return c; }
int pairs(int *a, int n) { int c = 0; for (int i = 0; i < n; i++) for (int j = 0; j < n; j++) if (a[i] == a[j]) c++; return c; }
int ten(void) { int s = 0; for (int i = 0; i < 10; i++) s += i; return s; }
int first(int *a) { int i = 0; while (1) { if (a[i]) break; i++; } return i; }
static int cmp(const void *a, const void *b) { return *(const int *)a - *(const int *)b; }
void sorts(int **rows, int n, int m) { for (int i = 0; i < n; i++) qsort(rows[i], m, sizeof(int), cmp); }
int find(int *a, int n, int x) { for (int i = 0; i < n; i++) if (a[i] == x) return i; return -1; }
void grow(int n) { for (int i = 0; i < n; i++) { if (i == 3) n++; } }
int t3(int n) { if (n < 1) return 0; return t3(n / 2) + t3(n / 2) + t3(n / 2); }
int u(int n) { return frobnicate(n); }
int p(int n) { return u(n); }
int gt(int n) { again: n--; if (n > 0) goto again; return n; }
int fp(int (*f)(int), int n) { return f(n); }
void e(void) { }
int ma(int x) { return ADD1(x); }
int ms(const char *s) { return SLOW(s); }
int ml(int n) { LOOPY(n); return 0; }
