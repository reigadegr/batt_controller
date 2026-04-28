CC      = cc
CFLAGS  = -Wall -Wextra -Werror -O2 -std=c11
LDFLAGS = -lpthread

SRCS = main.c config.c sysfs.c log.c charging.c charging_phase.c charging_loop.c monitor.c cli.c
OBJS = $(SRCS:.c=.o)
BIN  = payload

.PHONY: all clean

all: $(BIN)

$(BIN): $(OBJS)
	$(CC) $(CFLAGS) -o $@ $^ $(LDFLAGS)

%.o: %.c
	$(CC) $(CFLAGS) -c -o $@ $<

clean:
	rm -f $(OBJS) $(BIN)

# 依赖关系
main.o:          config.h sysfs.h charging.h monitor.h cli.h
config.o:        config.h
sysfs.o:         sysfs.h
log.o:           log.h
charging.o:      charging.h charging_internal.h sysfs.h
charging_phase.o: charging_internal.h sysfs.h
charging_loop.o: charging_internal.h sysfs.h
monitor.o:       monitor.h sysfs.h config.h
cli.o:           cli.h config.h sysfs.h
