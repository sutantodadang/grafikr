#include "cJSON.h"
#include <assert.h>

int main(void) {
    const char valid[] = {'[', ' ', '\t', '\n', '\r', '1', ']', '\0'};
    const char invalid[] = {'[', '\v', '1', ']', '\0'};
    cJSON *parsed = cJSON_ParseWithLengthOpts(valid, sizeof(valid), NULL, 1);
    if (!parsed) return 1;
    cJSON_Delete(parsed);
    parsed = cJSON_ParseWithLengthOpts(invalid, sizeof(invalid), NULL, 1);
    if (parsed) {
        cJSON_Delete(parsed);
        return 2;
    }
    return 0;
}
