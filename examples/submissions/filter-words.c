#include <stdio.h>
#include <string.h>

int main(void) {
    int n, k;
    char word[17];
    if (scanf("%d%d", &n, &k) != 2) return 0;
    for (int i = 0; i < n; ++i) {
        scanf("%16s", word);
        if ((int)strlen(word) >= k) printf("%s\n", word);
    }
    return 0;
}
