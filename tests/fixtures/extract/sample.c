#include <stddef.h>

typedef struct {
    const char *name;
    int score;
} Item;

int count_positive(const Item *items, size_t len) {
    int count = 0;
    for (size_t i = 0; i < len; i++) {
        if (items[i].score > 0) {
            count++;
        }
    }
    return count;
}

static void normalize_scores(Item *items, size_t len) {
    for (size_t i = 0; i < len; i++) {
        if (items[i].score < 0) {
            items[i].score = 0;
        } else if (items[i].score > 100) {
            items[i].score = 100;
        }
    }
}

int tiny(void) { return 1; }
