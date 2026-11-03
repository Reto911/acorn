#include <stdio.h>

int main(void) {
    int n;
    char words[100][17];
    if (scanf("%d", &n) != 1) return 0;
    for (int i = 0; i < n; ++i) scanf("%16s", words[i]);
    for (int i = n - 1; i >= 0; --i) {
        printf("%s%c", words[i], i == 0 ? '\n' : ' ');
    }
    return 0;
}
