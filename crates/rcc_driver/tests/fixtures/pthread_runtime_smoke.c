#include <pthread.h>

struct worker_arg {
    int input;
    int output;
};

static void *worker(void *raw) {
    struct worker_arg *arg = raw;
    arg->output = arg->input + 5;
    return arg;
}

int main(void) {
    pthread_t thread;
    struct worker_arg arg;
    void *joined = 0;

    arg.input = 37;
    arg.output = 0;

    if (pthread_create(&thread, 0, worker, &arg) != 0)
        return 10;
    if (pthread_join(thread, &joined) != 0)
        return 11;
    if (joined != &arg)
        return 12;
    return arg.output == 42 ? 0 : 13;
}
