#include <string.h>
#include <stdlib.h>
#define RETRY(n) for (;;)
#define REPEAT(i, n) for (int i = 0; i < (n); i++)
int count_down(int *a, int n) { int s = 0; for (int i = n; i > 0; i--) s += a[i]; return s; }
unsigned hash_words(const unsigned char *k, int len) { unsigned h = 0; while (len >= 4) { h += k[0]; k += 4; len -= 4; } return h; }
void fixpoint(int *a, int n) { int changed = 1; while (changed) { changed = 0; for (int i = 0; i < n; i++) if (a[i] > 0) { a[i]--; changed = 1; } } }
int step_once(int x) { return x + 1; }
void serve(int running) { int x = 0; while (running) { x = step_once(x); } }
void skip_continue(int *a, int n) { for (int i = 0; i < n; i++) { if (a[i]) continue; for (int j = 0; j < n; j++) a[j]++; } }
void unknown_outer(int *a, int n, int x, int y) { while (x < y) { for (int i = 0; i < n; i++) a[i]++; x = a[x]; } }
static int cmp_str(const void *a, const void *b) { return strcmp(*(char *const *)a, *(char *const *)b); }
void sort_names(char **v, int n) { qsort(v, n, sizeof *v, cmp_str); }
void sort_unknown(int *v, int n, int (*cmp)(const void *, const void *)) { qsort(v, n, sizeof *v, cmp); }
void join_all(char *buf, char **parts, int n) { for (int i = 0; i < n; i++) strcat(buf, parts[i]); }
int walk(int pyramid, int n) { if (n <= 0) return 0; return walk(pyramid, n - 1) + 1; }
int walk2(int n, int x) { if (n <= 0) return 0; return walk2(n - 1, x / 2) + 1; }
void draw(int depth, double len) { if (depth <= 0) return; draw(depth - 1, len / 2); draw(depth - 1, len / 2); }
void retry_forever(int x) { RETRY(3) { if (step_once(x) > 10) break; } }
int repeat_sum(int *a, int n) { int s = 0; REPEAT(i, n) { s += a[i]; } return s; }
void retry_index(int *a, int n) { for (int i = 0; i < n; i++) { if (a[i] < 0) { a[i]++; i--; } } }
int start_offset(int *a, int k, int n) { int s = 0; for (int i = k; i < n; i++) s += a[i]; return s; }
int zero_trip(int *a, int n, int k) { int s = 0; for (int j = k; j < 10; j++) for (int i = 0; i < n; i++) s += a[i]; return s; }
int ret_in_loop(int *a, int n, int m) { for (int i = 0; i < n; i++) { return a[i]; } int s = 0; for (int j = 0; j < m; j++) s += a[j]; return s; }
int twice(int *a, int n) { int s = 0; for (int i = 0; i < 2 * n; i++) s += a[i]; return s; }
int bytes(char *p, int n) { int s = 0; for (int i = 0; i < sizeof(int) * n; i++) s += p[i]; return s; }
int never(int *a, int n) { int s = 0; while (0) { for (int i = 0; i < n; i++) s += a[i]; } return s; }
int list_len(struct node *p) { int c = 0; while (p != NULL) { c++; p = p->next; } return c; }
int str_len(const char *s) { int c = 0; while (*s) { s++; c++; } return c; }
