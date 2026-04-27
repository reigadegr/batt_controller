CC      = cc
CFLAGS  = -Wall -Wextra -Werror -O2 -std=c11
LDFLAGS = -lpthread

SRCS = main.c config.c sysfs.c charging.c monitor.c
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
main.o:     config.h sysfs.h charging.h monitor.h
config.o:   config.h
sysfs.o:    sysfs.h
charging.o: charging.h sysfs.h config.h
monitor.o:  monitor.h sysfs.h config.h
