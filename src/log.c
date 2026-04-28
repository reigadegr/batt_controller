#include "log.h"

#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

void get_timestamp(char *buf, int bufsz)
{
    time_t now = time(NULL);
    struct tm *t = localtime(&now);
    strftime(buf, bufsz, "[%Y-%m-%d-%H:%M:%S]", t);
}

void log_write(const char *msg)
{
    printf("%s", msg);
    fflush(stdout);

    int fd = open(LOG_PATH, O_WRONLY | O_APPEND | O_CREAT | O_CLOEXEC, 0644);
    if (fd >= 0) {
        write(fd, msg, strlen(msg));
        close(fd);
    }
}
