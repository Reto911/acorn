#include <stdio.h>

int main(void) {
    long long a, b;
    if (scanf("%lld%lld", &a, &b) == 2) {
        printf("%.12f\n", (double)a / (double)b);
    }
    return 0;
}
