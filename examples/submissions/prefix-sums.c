#include <stdio.h>

int main(void) {
    int n;
    long long value, sum = 0;
    if (scanf("%d", &n) != 1) return 0;
    for (int i = 0; i < n; ++i) {
        scanf("%lld", &value);
        sum += value;
        printf("%lld%c", sum, i + 1 == n ? '\n' : ' ');
    }
    return 0;
}
