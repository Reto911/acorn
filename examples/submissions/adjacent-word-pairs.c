#include <stdio.h>

int main(void) {
    int n;
    char words[100][17];
    if (scanf("%d", &n) != 1) return 0;
    for (int i = 0; i < n; ++i) scanf("%16s", words[i]);
    for (int i = 0; i + 1 < n; ++i) {
        printf("%s %s\n", words[i], words[i + 1]);
    }
    return 0;
}
